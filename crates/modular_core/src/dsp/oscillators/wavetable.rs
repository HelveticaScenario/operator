//! `$wavetable` — band-limited wavetable oscillator with mipmap-based
//! anti-aliasing.
//!
//! The `wav` parameter is a reference to a loaded WAV file. On the main
//! thread, `prepare_resources` pre-computes an FFT-based mipmap pyramid
//! stored in `params.prepared` (a [`PreparedWavetable`]). The audio-thread
//! `update()` loop reads from that table using `params.pitch` (V/Oct),
//! `params.position` (frame index in 0–5V normalized to 0–1), and an
//! optional `params.phase` [`Table`] that warps the raw phase before
//! sampling.
//!
//! If the WAV isn't loaded or the prepared table is empty, the oscillator
//! outputs silence (no panic, no alloc).

use deserr::Deserr;
use schemars::JsonSchema;
use std::collections::HashMap;
use std::sync::Arc;

use std::ops::Range;

use crate::{
    block_input::{InputScratch, OptionBlockInput},
    dsp::{
        oscillators::{
            FmMode, apply_fm, sync_blep, sync_edge_fraction, wavetable_prep::PreparedWavetable,
        },
        utils::{SchmittTrigger, sanitize},
    },
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
    types::{Table, Wav, WavData},
};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
pub(crate) struct WavetableOscParams {
    /// Loaded WAV reference containing the wavetable data.
    pub(crate) wav: Wav,
    /// Pitch in V/Oct (0V = C4).
    #[signal(type = pitch)]
    pub(crate) pitch: PolySignal,
    /// Frame position as a signal. 0V maps to the first frame, 5V maps to
    /// the last frame. Values are clamped to [0, 5] before being normalized.
    #[signal(range = (0.0, 5.0))]
    #[deserr(default)]
    pub(crate) position: Option<PolySignal>,
    /// FM input signal (pre-scaled by user).
    #[deserr(default)]
    pub(crate) fm: Option<PolySignal>,
    /// FM mode: throughZero (default), lin, or exp.
    #[serde(default)]
    #[deserr(default)]
    pub(crate) fm_mode: FmMode,
    /// hard sync source — rising edges reset the oscillator phase.
    #[deserr(default)]
    pub(crate) sync: Option<PolySignal>,
    /// Optional phase-warp table applied before sampling.
    #[deserr(default)]
    pub(crate) phase: Option<Table>,
    /// phase offset in [0, 1) added to the internal phase before warping/sampling
    #[signal(default = 0.0, range = (0.0, 1.0))]
    #[deserr(default)]
    pub(crate) phase_offset: Option<PolySignal>,
    /// Pre-computed mipmap pyramid, populated by `prepare_resources` on the
    /// main thread. Skipped from serialization and deserialization.
    #[serde(skip)]
    #[deserr(skip)]
    #[schemars(skip)]
    pub(crate) prepared: Option<PreparedWavetable>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct WavetableOscOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Per-channel oscillator state.
#[derive(Clone, Copy)]
struct ChannelState {
    /// Phase accumulator in `[0, 1)`.
    phase: f64,
    /// Edge detector for the sync input.
    sync_schmitt: SchmittTrigger,
    /// Previous sync-input sample, for subsample edge interpolation.
    sync_prev: f32,
    /// PolyBLEP residual carried into the next sample from a sync reset.
    blep_carry: f32,
    /// The pitch and FM inputs `freq` and `level` were derived from. Compared
    /// exactly, so the cached values are always the ones a fresh computation
    /// would give; `NaN` never matches, forcing the first computation.
    last_pitch: f32,
    last_fm: f32,
    /// Playback frequency in Hz.
    freq: f32,
    /// Mipmap level for `freq`.
    level: usize,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            phase: 0.0,
            sync_schmitt: SchmittTrigger::default(),
            sync_prev: 0.0,
            blep_carry: 0.0,
            last_pitch: f32::NAN,
            last_fm: f32::NAN,
            freq: 0.0,
            level: 0,
        }
    }
}

/// Derive the channel count from the maximum of `pitch` and (optional)
/// `position`. Clamped to `[1, PORT_MAX_CHANNELS]`.
impl ChannelState {
    /// Track `pitch` and `fm`, recomputing the frequency and mipmap level only
    /// when they change.
    #[inline(always)]
    fn tune(&mut self, prepared: &PreparedWavetable, pitch: f32, fm: f32, fm_mode: FmMode) {
        if pitch != self.last_pitch || fm != self.last_fm {
            self.freq = apply_fm(pitch, fm, fm_mode);
            self.level = prepared.mipmap_level_for_freq(self.freq);
            self.last_pitch = pitch;
            self.last_fm = fm;
        }
    }

    /// One output sample (±5 V) at `position` (0–5 V across the frames), then
    /// advance the phase. `warp` maps the read phase; `sync` is this sample's
    /// sync input, if connected.
    #[inline(always)]
    fn step(
        &mut self,
        prepared: &PreparedWavetable,
        inv_sr: f64,
        position: f32,
        offset: f32,
        warp: impl Fn(f32) -> f32,
        sync: Option<f32>,
    ) -> f32 {
        let frame_scale = if prepared.frame_count > 1 {
            (prepared.frame_count - 1) as f32
        } else {
            0.0
        };
        // Frame index: 0–5V → 0..=frame_count-1.
        let frame_f = (position.clamp(0.0, 5.0) / 5.0) * frame_scale;

        // The phase offset shifts the read position without altering the
        // accumulator, so it never drifts. The current phase is read before
        // it advances, so the first sample reads the starting phase.
        let read_offset = offset.rem_euclid(1.0);
        let raw_phase = (self.phase as f32 + read_offset).rem_euclid(1.0);
        let before = prepared.read_sample(self.level, frame_f, warp(raw_phase));

        // Naive read plus any residual carried from a sync reset on the
        // previous sample.
        let mut sample = before + self.blep_carry;
        self.blep_carry = 0.0;

        // Advance phase, wrap to [0, 1). rem_euclid in f64.
        let increment = self.freq as f64 * inv_sr;
        let mut next = self.phase + increment;
        // Fast wrap for the common case; rem_euclid handles the rest.
        if next >= 1.0 {
            next -= 1.0;
            if next >= 1.0 {
                next = next.rem_euclid(1.0);
            }
        } else if next < 0.0 {
            next = next.rem_euclid(1.0);
        }
        if !next.is_finite() {
            next = 0.0;
        }

        // Hard sync: a rising edge resets the phase so the next sample reads
        // from the table's start. A PolyBLEP at the subsample crossing
        // band-limits the resulting step.
        if let Some(v) = sync {
            if self.sync_schmitt.process(v) {
                let frac = sync_edge_fraction(self.sync_prev, v);
                // After a reset the accumulator is 0, so the effective read
                // phase is the offset itself.
                let after = prepared.read_sample(self.level, frame_f, warp(read_offset));
                let (now, carry) = sync_blep(after - before, frac);
                sample += now;
                self.blep_carry = carry;
                next = 0.0;
            }
            self.sync_prev = v;
        }

        self.phase = next;
        // Wavetables are normalized in [-1, 1] after the FFT round-trip;
        // scale to the synth's audio range.
        sample * 5.0
    }
}

#[allow(private_interfaces)]
pub fn wavetable_derive_channel_count(params: &WavetableOscParams) -> usize {
    let pitch_ch = params.pitch.channels();
    let pos_ch = params.position.as_ref().map(|p| p.channels()).unwrap_or(0);
    let fm_ch = params.fm.as_ref().map(|f| f.channels()).unwrap_or(0);
    let phase_ch = params.phase.as_ref().map(|t| t.channels()).unwrap_or(0);
    let sync_ch = params.sync.as_ref().map(|s| s.channels()).unwrap_or(0);
    let offset_ch = params
        .phase_offset
        .as_ref()
        .map(|o| o.channels())
        .unwrap_or(0);
    pitch_ch
        .max(pos_ch)
        .max(fm_ch)
        .max(phase_ch)
        .max(sync_ch)
        .max(offset_ch)
        .clamp(1, PORT_MAX_CHANNELS)
}

/// A band-limited wavetable oscillator.
///
/// Reads a pre-built mipmap pyramid (FFT-filtered frame copies) and
/// selects a level appropriate for the playback frequency to suppress
/// aliasing. Frame position can be swept across multi-frame tables for
/// classic wavetable timbral sweeps, and an optional phase-warp `Table`
/// reshapes the read phase before sampling.
///
/// ## Example
///
/// ```js
/// $wavetable('c4', $wavs().tables.pad).out()
/// $wavetable('c2', $wavs().tables.pad, $sine('0.2hz').range(0, 5)).out()
/// ```
#[module(
    name = "$wavetable",
    channels_derive = wavetable_derive_channel_count,
    args(pitch, wav, position),
    has_prepare_resources,
    patch_update,
    block,
)]
pub struct WavetableOsc {
    params: WavetableOscParams,
    outputs: WavetableOscOutputs,
    channel_state: Box<[ChannelState]>,
}

impl WavetableOsc {
    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        let Some(prepared) = self.params.prepared.as_ref().filter(|p| p.is_playable()) else {
            // Silent fallback — prepared mipmap is missing (wav unloaded / empty).
            for ch in 0..channels {
                self.outputs.sample.set(ch, 0.0);
            }
            return;
        };
        let inv_sr = 1.0 / sample_rate as f64;
        let p = &self.params;
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            state.tune(
                prepared,
                p.pitch.get_value(ch),
                p.fm.value_or(ch, 0.0),
                p.fm_mode,
            );
            let warp = |x: f32| match &p.phase {
                Some(table) => table.evaluate(x, ch),
                None => x,
            };
            let sample = state.step(
                prepared,
                inv_sr,
                p.position.value_or(ch, 0.0),
                p.phase_offset.value_or(ch, 0.0),
                warp,
                p.sync.as_ref().map(|sync| sync.get_value(ch)),
            );
            self.outputs.sample.set(ch, sample);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut WavetableOscBlockOutputs,
        range: Range<usize>,
        sample_rate: f32,
    ) -> Option<()> {
        let p = &self.params;
        // A warp table reads its own signals per sample: leave it to `update`.
        if p.phase.is_some() {
            return None;
        }
        let out = &mut outputs.sample;
        let Some(prepared) = p.prepared.as_ref().filter(|p| p.is_playable()) else {
            for i in range {
                out.slot_mut(i).fill(0.0);
            }
            return Some(());
        };
        let channels = self.channel_count();
        let mut scratch: [InputScratch; 5] = std::array::from_fn(|_| InputScratch::new());
        let [s0, s1, s2, s3, s4] = &mut scratch;
        let pitch = p.pitch.block_input(range.clone(), channels, s0)?;
        let fm = p.fm.block_input_or(range.clone(), channels, 0.0, s1)?;
        let position = p
            .position
            .block_input_or(range.clone(), channels, 0.0, s2)?;
        let offset = p
            .phase_offset
            .block_input_or(range.clone(), channels, 0.0, s3)?;
        let sync = match &p.sync {
            Some(sync) => Some(sync.block_input(range.clone(), channels, s4)?),
            None => None,
        };
        let inv_sr = 1.0 / sample_rate as f64;
        for i in range {
            let (pitch, fm, position, offset) =
                (pitch.row(i), fm.row(i), position.row(i), offset.row(i));
            let sync = sync.as_ref().map(|sync| sync.row(i));
            let slots = out
                .slot_mut(i)
                .iter_mut()
                .zip(self.channel_state.iter_mut());
            for (ch, (out, state)) in slots.enumerate() {
                state.tune(prepared, pitch[ch], fm[ch], p.fm_mode);
                let sample = state.step(
                    prepared,
                    inv_sr,
                    position[ch],
                    offset[ch],
                    |x| x,
                    sync.map(|sync| sync[ch]),
                );
                *out = sanitize(sample);
            }
        }
        Some(())
    }

    fn prepare_resources_impl(
        &mut self,
        wav_data: &HashMap<String, Arc<WavData>>,
        sample_rate: f32,
    ) {
        if let Some(wd) = wav_data.get(self.params.wav.path()) {
            self.params.prepared = Some(PreparedWavetable::from_wav_data(wd, sample_rate));
        } else {
            self.params.prepared = None;
        }
    }
}

impl crate::types::PatchUpdateHandler for WavetableOsc {
    fn on_patch_update(&mut self) {
        // The cached frequency and level also depend on `fmMode` and the table,
        // either of which a patch update can change.
        for state in self.channel_state.iter_mut() {
            state.last_pitch = f32::NAN;
        }
    }
}

message_handlers!(impl WavetableOsc {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, SampleBuffer, Signal};
    use std::f32::consts::TAU;

    /// Build a WavData containing `n_frames` sine-wave frames of `size`
    /// samples each.
    fn make_wav(size: usize, n_frames: usize) -> Arc<WavData> {
        let mut ch = Vec::with_capacity(size * n_frames);
        for f in 0..n_frames {
            // Vary the waveform per frame: frame 0 = sine, frame 1 = cosine,
            // frame 2 = inverted sine, etc.
            let phase_offset = f as f32 * TAU * 0.25;
            for i in 0..size {
                let p = TAU * (i as f32 / size as f32) + phase_offset;
                ch.push(p.sin());
            }
        }
        Arc::new(WavData::new(
            SampleBuffer::from_samples(vec![ch], 48000.0),
            Some(size),
        ))
    }

    fn make_osc(params: WavetableOscParams) -> WavetableOsc {
        let channels = wavetable_derive_channel_count(&params);
        let mut outputs = WavetableOscOutputs::default();
        outputs.set_all_channels(channels);
        let osc = WavetableOsc {
            params,
            outputs,
            channel_state: vec![ChannelState::default(); channels].into_boxed_slice(),
            _channel_count: channels,
            _block_index: Default::default(),
        };
        osc
    }

    fn base_params() -> WavetableOscParams {
        WavetableOscParams {
            wav: Wav::new("test.wav".to_string(), 1),
            pitch: PolySignal::mono(Signal::Volts(0.0)),
            position: None,
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
            phase: None,
            phase_offset: None,
            prepared: None,
        }
    }

    /// The cached frequency follows a patch update that changes `fmMode` while
    /// the pitch and FM inputs stay the same.
    #[test]
    fn fm_mode_change_refreshes_cached_frequency() {
        use crate::types::PatchUpdateHandler;

        let mut params = base_params();
        params.fm = Some(PolySignal::mono(Signal::Volts(0.5)));
        params.fm_mode = FmMode::Exp;
        params.prepared = Some(PreparedWavetable::from_wav_data(&make_wav(256, 1), 48000.0));
        let mut osc = make_osc(params);
        osc.update(48000.0);
        assert_eq!(osc.channel_state[0].freq, apply_fm(0.0, 0.5, FmMode::Exp));

        osc.params.fm_mode = FmMode::Lin;
        osc.on_patch_update();
        osc.update(48000.0);
        assert_eq!(osc.channel_state[0].freq, apply_fm(0.0, 0.5, FmMode::Lin));
        assert_ne!(
            apply_fm(0.0, 0.5, FmMode::Lin),
            apply_fm(0.0, 0.5, FmMode::Exp)
        );
    }

    #[test]
    fn silent_when_not_prepared() {
        let mut osc = make_osc(base_params());
        for _ in 0..128 {
            osc.update(48000.0);
        }
        // No panic, no audio.
        assert_eq!(osc.outputs.sample.get(0), 0.0);
    }

    #[test]
    fn produces_sine_shaped_output_at_c4() {
        let size = 2048;
        let wav = make_wav(size, 1);
        let mut params = base_params();
        params.prepared = Some(PreparedWavetable::from_wav_data(&wav, 48000.0));
        let mut osc = make_osc(params);

        // At C4 (0V), freq ≈ 261.63 Hz; at 48kHz that's ~183 samples/cycle.
        // Run ~4 cycles and track min/max plus RMS.
        let n = 800;
        let mut samples = Vec::with_capacity(n);
        for _ in 0..n {
            osc.update(48000.0);
            samples.push(osc.outputs.sample.get(0));
        }
        let max = samples.iter().cloned().fold(f32::MIN, f32::max);
        let min = samples.iter().cloned().fold(f32::MAX, f32::min);
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();

        // A unit-amplitude sine scaled by 5 should oscillate in about ±5V.
        assert!(max > 4.0, "expected peak near +5V, got {max}");
        assert!(min < -4.0, "expected trough near -5V, got {min}");
        // RMS of a ±5V sine is 5/sqrt(2) ≈ 3.54V.
        assert!(
            (rms - 5.0 / 2.0_f32.sqrt()).abs() < 0.5,
            "expected RMS near 3.54, got {rms}"
        );
    }

    #[test]
    fn position_selects_different_frames() {
        let size = 2048;
        let wav = make_wav(size, 2);
        // Frame 0 = sin(phase); frame 1 = sin(phase + 0.25*TAU) = cos(phase).
        // Play a fixed phase (freq ≈ 0) and sweep position between frames.

        let mut p0 = base_params();
        p0.pitch = PolySignal::mono(Signal::Volts(-10.0)); // very low freq
        p0.position = Some(PolySignal::mono(Signal::Volts(0.0))); // frame 0
        p0.prepared = Some(PreparedWavetable::from_wav_data(&wav, 48000.0));
        let mut osc0 = make_osc(p0);

        let mut p1 = base_params();
        p1.pitch = PolySignal::mono(Signal::Volts(-10.0));
        p1.position = Some(PolySignal::mono(Signal::Volts(5.0))); // frame 1
        p1.prepared = Some(PreparedWavetable::from_wav_data(&wav, 48000.0));
        let mut osc1 = make_osc(p1);

        // Run one sample each (phase still ~0, so frame 0 ≈ 0, frame 1 ≈ 1*5).
        osc0.update(48000.0);
        osc1.update(48000.0);

        let s0 = osc0.outputs.sample.get(0);
        let s1 = osc1.outputs.sample.get(0);
        // Frame 0 sine at phase 0 → ~0. Frame 1 cosine at phase 0 → ~1*5V.
        assert!(s0.abs() < 0.5, "frame 0 start ~0, got {s0}");
        assert!((s1 - 5.0).abs() < 0.5, "frame 1 start ~5V, got {s1}");
    }

    #[test]
    fn phase_warp_changes_output() {
        let size = 2048;
        let wav = make_wav(size, 1);

        let mut p_plain = base_params();
        p_plain.pitch = PolySignal::mono(Signal::Volts(0.0));
        p_plain.prepared = Some(PreparedWavetable::from_wav_data(&wav, 48000.0));
        let mut osc_plain = make_osc(p_plain);

        let mut p_warped = base_params();
        p_warped.pitch = PolySignal::mono(Signal::Volts(0.0));
        p_warped.phase = Some(Table::Mirror {
            amount: PolySignal::mono(Signal::Volts(3.0)),
        });
        p_warped.prepared = Some(PreparedWavetable::from_wav_data(&wav, 48000.0));
        let mut osc_warped = make_osc(p_warped);

        // Run enough samples for phase to drift across cycles.
        let n = 400;
        let mut plain = Vec::with_capacity(n);
        let mut warped = Vec::with_capacity(n);
        for _ in 0..n {
            osc_plain.update(48000.0);
            osc_warped.update(48000.0);
            plain.push(osc_plain.outputs.sample.get(0));
            warped.push(osc_warped.outputs.sample.get(0));
        }
        let diff: f32 = plain
            .iter()
            .zip(warped.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        // If the warp had no effect, diff would be ~0.
        assert!(
            diff > 50.0,
            "expected warped output to differ; diff = {diff}"
        );
    }

    #[test]
    fn prepare_resources_noop_when_wav_missing() {
        let mut osc = make_osc(base_params());
        let wav_data: HashMap<String, Arc<WavData>> = HashMap::new();
        osc.prepare_resources_impl(&wav_data, 48000.0);
        assert!(osc.params.prepared.is_none());
        osc.update(48000.0);
        assert_eq!(osc.outputs.sample.get(0), 0.0);
    }

    #[test]
    fn prepare_resources_populates_when_wav_present() {
        let size = 2048;
        let wav = make_wav(size, 1);
        let mut osc = make_osc(base_params());
        let mut wav_data: HashMap<String, Arc<WavData>> = HashMap::new();
        wav_data.insert("test.wav".to_string(), wav);
        osc.prepare_resources_impl(&wav_data, 48000.0);
        assert!(osc.params.prepared.is_some());
        // Processing should now produce non-zero samples.
        let mut any_nonzero = false;
        for _ in 0..400 {
            osc.update(48000.0);
            if osc.outputs.sample.get(0).abs() > 0.1 {
                any_nonzero = true;
            }
        }
        assert!(any_nonzero, "expected non-zero output once prepared");
    }
}
