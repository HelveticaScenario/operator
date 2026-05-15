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

use crate::{
    dsp::oscillators::{FmMode, apply_fm, wavetable_prep::PreparedWavetable},
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
    types::{Connect, Table, Wav, WavData},
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
    /// Optional phase-warp table applied before sampling.
    #[deserr(default)]
    pub(crate) phase: Option<Table>,
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
}

impl Default for ChannelState {
    fn default() -> Self {
        Self { phase: 0.0 }
    }
}

#[derive(Default)]
struct WavetableOscState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// Derive the channel count from the maximum of `pitch` and (optional)
/// `position`. Clamped to `[1, PORT_MAX_CHANNELS]`.
#[allow(private_interfaces)]
pub fn wavetable_derive_channel_count(params: &WavetableOscParams) -> usize {
    let pitch_ch = params.pitch.channels();
    let pos_ch = params.position.as_ref().map(|p| p.channels()).unwrap_or(0);
    let fm_ch = params.fm.as_ref().map(|f| f.channels()).unwrap_or(0);
    let phase_ch = params.phase.as_ref().map(|t| t.channels()).unwrap_or(0);
    pitch_ch
        .max(pos_ch)
        .max(fm_ch)
        .max(phase_ch)
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
/// $wavetable($wavs().tables.pad, 'c4').out()
/// $wavetable(wav, 'c2', { position: lfo }).out()
/// ```
#[module(
    name = "$wavetable",
    channels_derive = wavetable_derive_channel_count,
    args(wav, pitch, position),
    has_prepare_resources
)]
pub struct WavetableOsc {
    params: WavetableOscParams,
    outputs: WavetableOscOutputs,
    state: WavetableOscState,
}

impl WavetableOsc {
    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();

        // Silent fallback — prepared mipmap is missing (wav unloaded / empty).
        let Some(prepared) = self.params.prepared.as_ref() else {
            for ch in 0..channels {
                self.outputs.sample.set(ch, 0.0);
            }
            return;
        };
        if prepared.frame_count == 0 || prepared.frame_size == 0 {
            for ch in 0..channels {
                self.outputs.sample.set(ch, 0.0);
            }
            return;
        }

        let inv_sr = 1.0 / sample_rate as f64;
        let frame_scale = if prepared.frame_count > 1 {
            (prepared.frame_count - 1) as f32
        } else {
            0.0
        };

        for ch in 0..channels {
            let state = &mut self.state.channels[ch];

            let pitch_v = self.params.pitch.get_value(ch);
            let fm = self.params.fm.value_or(ch, 0.0);
            let freq = apply_fm(pitch_v, fm, self.params.fm_mode);

            // Frame index: 0–5V → 0..=frame_count-1.
            let pos_v = self.params.position.value_or(ch, 0.0).clamp(0.0, 5.0);
            let frame_f = (pos_v / 5.0) * frame_scale;

            // Advance raw phase first, then apply warp. Using the pre-advance
            // phase is equivalent here but we advance after reading so the
            // very first sample uses the state's starting phase (0.0 by
            // default).
            let raw_phase = state.phase as f32;
            let warped_phase = match &self.params.phase {
                Some(table) => table.evaluate(raw_phase, ch),
                None => raw_phase,
            };

            let level = prepared.mipmap_level_for_freq(freq);
            let sample = prepared.read_sample(level, frame_f, warped_phase);

            // Output ±5V — wavetables are normalized in [-1, 1] after the
            // FFT round-trip; scale to the synth's audio range.
            self.outputs.sample.set(ch, sample * 5.0);

            // Advance phase, wrap to [0, 1). rem_euclid in f64.
            let increment = freq as f64 * inv_sr;
            let mut next = state.phase + increment;
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
            state.phase = next;
        }
    }

    /// Main-thread resource preparation. Builds the mipmap pyramid from the
    /// referenced WAV if it's present in the cache snapshot.
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
        WavetableOsc {
            params,
            outputs,
            state: WavetableOscState::default(),
            _channel_count: channels,
        }
    }

    fn base_params() -> WavetableOscParams {
        WavetableOscParams {
            wav: Wav::new("test.wav".to_string(), 1),
            pitch: PolySignal::mono(Signal::Volts(0.0)),
            position: None,
            fm: None,
            fm_mode: FmMode::default(),
            phase: None,
            prepared: None,
        }
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
