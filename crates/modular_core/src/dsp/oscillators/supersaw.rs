use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::{
        oscillators::{FmMode, apply_fm, sync_blep, sync_edge_fraction},
        utils::SchmittTrigger,
    },
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
};

fn default_voices() -> usize {
    5
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct SupersawParams {
    /// pitch in V/Oct (0V = C4)
    freq: PolySignal,
    /// number of supersaw voices (1–16)
    #[serde(default = "default_voices")]
    #[deserr(default = default_voices())]
    voices: usize,
    /// detune spread in semitones (default 0.18)
    #[signal(type = control, default = 0.18, range = (0, 12))]
    #[deserr(default)]
    detune: Option<PolySignal>,
    /// FM input signal (pre-scaled by user)
    #[deserr(default)]
    fm: Option<PolySignal>,
    /// FM mode: throughZero (default), lin, or exp
    #[serde(default)]
    #[deserr(default)]
    fm_mode: FmMode,
    /// hard sync source — rising edges reset every detuned voice's phase
    #[deserr(default)]
    sync: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SupersawOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Custom channel count: voices clamped to [1, PORT_MAX_CHANNELS].
#[allow(private_interfaces)]
pub fn supersaw_derive_channel_count(params: &SupersawParams) -> usize {
    params.voices.clamp(1, PORT_MAX_CHANNELS)
}

/// Supersaw oscillator with multiple detuned sawtooth voices and PolyBLEP anti-aliasing.
///
/// Generates a classic supersaw sound by stacking multiple sawtooth oscillators
/// with symmetric detuning. Each input channel is processed by all voices,
/// creating a rich, full sound.
///
/// - **freq** — pitch in V/Oct (0V = C4)
/// - **voices** — number of detuned saw voices (1–16, default 5)
/// - **detune** — detune spread in semitones (default 0.18)
///
/// Output range is **±5V** with gain compensation for input channel count.
///
/// ## Example
///
/// ```js
/// $supersaw('c3').out()
/// $supersaw('c3', { voices: 7, detune: 0.3 }).out()
/// ```
#[module(name = "$supersaw", channels_derive = supersaw_derive_channel_count, has_init, patch_update, args(freq))]
pub struct Supersaw {
    outputs: SupersawOutputs,
    params: SupersawParams,
    state: SupersawState,
}

/// State for the Supersaw module.
struct SupersawState {
    /// Phase state for matrix mixing: indexed as [input_ch * PORT_MAX_CHANNELS + voice]
    osc_states: [f32; PORT_MAX_CHANNELS * PORT_MAX_CHANNELS],
    rng_state: u32,
    /// Voice count clamped to [1, PORT_MAX_CHANNELS]. Derived from params.
    voices: usize,
    /// Channel count of the freq input. Derived from params.
    input_channels: usize,
    /// Reciprocal of the sample rate.
    inv_sample_rate: f32,
    /// Output gain, compensated for input channel count.
    gain: f32,
    /// Per-voice interpolation factor for symmetric detuning.
    voice_t: [f32; PORT_MAX_CHANNELS],
    /// Per-input-channel edge detector for the sync input.
    sync_schmitt: [SchmittTrigger; PORT_MAX_CHANNELS],
    /// Per-input-channel previous sync sample, for subsample edge interpolation.
    sync_prev: [f32; PORT_MAX_CHANNELS],
    /// Per-voice PolyBLEP residual carried into the next sample from a sync
    /// reset. Indexed like `osc_states`.
    blep_carry: [f32; PORT_MAX_CHANNELS * PORT_MAX_CHANNELS],
}

impl Default for SupersawState {
    fn default() -> Self {
        Self {
            osc_states: [0.0; PORT_MAX_CHANNELS * PORT_MAX_CHANNELS],
            rng_state: 0,
            voices: 1,
            input_channels: 1,
            inv_sample_rate: 0.0,
            gain: 0.0,
            voice_t: [0.0; PORT_MAX_CHANNELS],
            sync_schmitt: [SchmittTrigger::default(); PORT_MAX_CHANNELS],
            sync_prev: [0.0; PORT_MAX_CHANNELS],
            blep_carry: [0.0; PORT_MAX_CHANNELS * PORT_MAX_CHANNELS],
        }
    }
}

/// PolyBLEP correction for sawtooth wave discontinuity at phase wrap.
#[inline(always)]
fn poly_blep_saw(phase: f32, dt: f32) -> f32 {
    // Near phase = 0 (just after wrap)
    if phase < dt {
        let t = phase / dt;
        return t + t - t * t - 1.0;
    }
    // Near phase = 1 (just before wrap)
    if phase > 1.0 - dt {
        let t = (phase - 1.0) / dt;
        return t * t + t + t + 1.0;
    }
    0.0
}

/// Simple xorshift32 PRNG.
#[inline(always)]
fn xorshift32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// Generate a random phase in [0, 1) from the PRNG state.
#[inline(always)]
fn rand_phase(state: &mut u32) -> f32 {
    let x = xorshift32(state);
    (x as f32) / (u32::MAX as f32)
}

impl Supersaw {
    fn init(&mut self, sample_rate: f32) {
        // Sample-rate-derived: safe in init because the rate never changes across
        // a patch transfer (a rate change rebuilds the processor, not a transfer).
        self.state.inv_sample_rate = 1.0 / sample_rate;

        // Seed per-oscillator phases once. This runtime state is preserved across
        // patch updates by transfer_state_from, so it must not be re-seeded in
        // configure().
        self.state.rng_state = self as *const Self as usize as u32;
        for i in 0..self.state.osc_states.len() {
            self.state.osc_states[i] = rand_phase(&mut self.state.rng_state);
        }
    }

    /// Recompute param- and sample-rate-derived constants. Invoked from
    /// `on_patch_update`, which runs after `transfer_state_from` swaps `state`,
    /// so these reflect the current params — not a transferred predecessor's.
    fn configure(&mut self) {
        let voices = self.params.voices.clamp(1, PORT_MAX_CHANNELS);
        let input_channels = self.params.freq.channels().max(1);

        self.state.voices = voices;
        self.state.input_channels = input_channels;

        // Gain: 5V range, compensated for input channel count
        self.state.gain = 5.0 / (input_channels as f32).sqrt();

        // Voice interpolation factor (precompute per voice)
        // Interleaved ordering: first half of voices gets even detune positions,
        // second half gets odd positions. This ensures each half contains a
        // balanced spread across the full detune range, so splitting voices
        // into two groups (e.g. for stereo panning) gives symmetric detuning
        // on each side — matching Strudel's alternating L/R distribution.
        let half = (voices + 1) / 2;
        for v in 0..voices {
            let linear_pos = if v < half { v * 2 } else { (v - half) * 2 + 1 };
            self.state.voice_t[v] = if voices > 1 {
                linear_pos as f32 / (voices - 1) as f32
            } else {
                0.5 // centered, offset will be 0
            };
        }
    }

    fn update(&mut self, _sample_rate: f32) {
        let voices = self.state.voices;
        let input_channels = self.state.input_channels;
        let inv_sample_rate = self.state.inv_sample_rate;
        let gain = self.state.gain;

        // Detect sync once per input channel (the master edge resets every
        // detuned voice of that channel together) and record the subsample
        // crossing for the PolyBLEP correction below.
        let mut sync_edge = [false; PORT_MAX_CHANNELS];
        let mut sync_frac = [0.0f32; PORT_MAX_CHANNELS];
        if let Some(sync) = &self.params.sync {
            for input_ch in 0..input_channels {
                let v = sync.get_value(input_ch);
                if self.state.sync_schmitt[input_ch].process(v) {
                    sync_edge[input_ch] = true;
                    sync_frac[input_ch] = sync_edge_fraction(self.state.sync_prev[input_ch], v);
                }
                self.state.sync_prev[input_ch] = v;
            }
        }

        for voice in 0..voices {
            let mut accum = 0.0f32;

            for input_ch in 0..input_channels {
                // Detune channel-matches input pitch (per-note detune)
                let detune = self.params.detune.value_or(input_ch, 0.18);
                let offset_semitones = if voices > 1 {
                    // lerp from -detune/2 to +detune/2
                    -detune / 2.0 + self.state.voice_t[voice] * detune
                } else {
                    0.0
                };

                let pitch = self.params.freq.get_value(input_ch);
                let fm = self.params.fm.value_or(input_ch, 0.0);
                let base_freq = apply_fm(pitch, fm, self.params.fm_mode);
                let freq = base_freq * (2.0f32).powf(offset_semitones / 12.0);
                let dt = freq * inv_sample_rate;

                let state_idx = input_ch * PORT_MAX_CHANNELS + voice;

                // Advance phase (rem_euclid supports negative increments from through-zero FM)
                let mut phase = (self.state.osc_states[state_idx] + dt).rem_euclid(1.0);

                // Naive saw + its own PolyBLEP wrap correction, then any residual
                // carried from a sync reset on the previous sample. The reset
                // (below) lands in the upcoming interval, so this operates on the
                // real, pre-reset phase.
                let mut saw = 2.0 * phase - 1.0;
                saw -= poly_blep_saw(phase, dt.abs());
                saw += self.state.blep_carry[state_idx];
                self.state.blep_carry[state_idx] = 0.0;

                // Hard sync resets this voice, band-limited with a PolyBLEP at
                // the subsample crossing.
                if sync_edge[input_ch] {
                    let before = 2.0 * phase - 1.0;
                    phase = 0.0;
                    let after = -1.0;
                    let (now, carry) = sync_blep(after - before, sync_frac[input_ch]);
                    saw += now;
                    self.state.blep_carry[state_idx] = carry;
                }

                self.state.osc_states[state_idx] = phase;
                accum += saw;
            }

            self.outputs.sample.set(voice, accum * gain);
        }
    }
}

impl crate::types::PatchUpdateHandler for Supersaw {
    fn on_patch_update(&mut self) {
        self.configure();
    }
}

message_handlers!(impl Supersaw {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    /// Create a Supersaw with params and properly initialize channel count and output channels.
    /// Mirrors the production lifecycle: `init` (seeds phases, captures sample rate)
    /// then `on_patch_update` (computes param-derived constants), both of which the
    /// `#[module]` macro invokes in production.
    fn make_supersaw(params: SupersawParams) -> Supersaw {
        use crate::types::PatchUpdateHandler;
        let channels = supersaw_derive_channel_count(&params);
        let mut outputs = SupersawOutputs::default();
        outputs.set_all_channels(channels);
        let mut s = Supersaw {
            params,
            outputs,
            _channel_count: channels,
            _block_index: Default::default(),
            state: SupersawState::default(),
        };
        s.init(48000.0);
        s.on_patch_update();
        s
    }

    #[test]
    fn test_single_voice_output() {
        let mut s = make_supersaw(SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 1,
            detune: None,
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        });
        // Run several samples to get past initialization
        for _ in 0..100 {
            s.update(48000.0);
        }
        let val = s.outputs.sample.get(0);
        // Single voice should produce output in ±5V range
        assert!(val.abs() <= 5.01, "Output {val} should be within ±5V");
    }

    #[test]
    fn test_channel_count_equals_voices() {
        let params = SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 7,
            detune: None,
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        };
        assert_eq!(supersaw_derive_channel_count(&params), 7);
    }

    #[test]
    fn test_output_bounded() {
        let mut s = make_supersaw(SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 5,
            detune: None,
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        });
        for _ in 0..1000 {
            s.update(48000.0);
        }
        for ch in 0..5 {
            let val = s.outputs.sample.get(ch);
            assert!(
                val.abs() <= 5.5,
                "Channel {ch} output {val} should be bounded"
            );
        }
    }

    #[test]
    fn test_voices_clamped_to_16() {
        let params = SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 32,
            detune: None,
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        };
        assert_eq!(supersaw_derive_channel_count(&params), 16);

        let params_zero = SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 0,
            detune: None,
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        };
        assert_eq!(supersaw_derive_channel_count(&params_zero), 1);
    }

    #[test]
    fn test_detune_affects_pitch() {
        // With detune=0, all voices should be identical (same phase progression)
        let mut s_no_detune = make_supersaw(SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 3,
            detune: Some(PolySignal::mono(Signal::Volts(0.0))),
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        });
        // Force known phases (overwrite whatever init set)
        for i in 0..PORT_MAX_CHANNELS * PORT_MAX_CHANNELS {
            s_no_detune.state.osc_states[i] = 0.25;
        }

        let mut s_detune = make_supersaw(SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 3,
            detune: Some(PolySignal::mono(Signal::Volts(2.0))),
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        });
        for i in 0..PORT_MAX_CHANNELS * PORT_MAX_CHANNELS {
            s_detune.state.osc_states[i] = 0.25;
        }

        // Run both for several samples
        for _ in 0..100 {
            s_no_detune.update(48000.0);
            s_detune.update(48000.0);
        }

        // With no detune, voice 0 and voice 2 should be the same
        let v0_no = s_no_detune.outputs.sample.get(0);
        let v2_no = s_no_detune.outputs.sample.get(2);
        assert!(
            (v0_no - v2_no).abs() < 1e-6,
            "No-detune voices should be equal: {v0_no} vs {v2_no}"
        );

        // With detune, voice 0 and voice 2 should differ
        let v0_det = s_detune.outputs.sample.get(0);
        let v2_det = s_detune.outputs.sample.get(2);
        assert!(
            (v0_det - v2_det).abs() > 1e-6,
            "Detuned voices should differ: {v0_det} vs {v2_det}"
        );
    }

    #[test]
    fn test_random_phases_differ() {
        let mut s = make_supersaw(SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 4,
            detune: Some(PolySignal::mono(Signal::Volts(0.0))),
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        });
        // Trigger phase initialization via init()
        s.init(48000.0);

        // Check that at least some initial phases differ
        // (statistically extremely unlikely all 4 are identical)
        let phases: Vec<f32> = (0..4).map(|v| s.state.osc_states[v]).collect();
        let all_same = phases.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-10);
        assert!(!all_same, "Random phases should not all be identical");
    }

    #[test]
    fn test_matrix_mixing_mono_input() {
        // Mono input with 3 voices -> 3 output channels
        let mut s = make_supersaw(SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 3,
            detune: Some(PolySignal::mono(Signal::Volts(0.0))),
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        });
        // Force known phases, zero detune -> all voices should produce identical output
        for i in 0..PORT_MAX_CHANNELS * PORT_MAX_CHANNELS {
            s.state.osc_states[i] = 0.5;
        }
        s.update(48000.0);

        let v0 = s.outputs.sample.get(0);
        let v1 = s.outputs.sample.get(1);
        let v2 = s.outputs.sample.get(2);
        // With zero detune and same initial phase, all voices should be identical
        assert!(
            (v0 - v1).abs() < 1e-6,
            "Same phase, no detune: voices should match: {v0} vs {v1}"
        );
        assert!(
            (v1 - v2).abs() < 1e-6,
            "Same phase, no detune: voices should match: {v1} vs {v2}"
        );
    }

    #[test]
    fn hard_sync_resets_all_voice_phases() {
        let mut s = make_supersaw(SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 3,
            detune: Some(PolySignal::mono(Signal::Volts(1.0))),
            fm: None,
            fm_mode: FmMode::default(),
            sync: Some(PolySignal::mono(Signal::Volts(0.0))),
        });

        // Advance so every voice phase is clearly non-zero and the sync edge
        // detector is armed low.
        for _ in 0..50 {
            s.update(48000.0);
        }
        let advanced = (0..3)
            .map(|v| s.state.osc_states[v])
            .any(|p| p.abs() > 1e-3);
        assert!(advanced, "voice phases should have advanced before sync");

        // Drive a rising edge on the sync input.
        s.params.sync = Some(PolySignal::mono(Signal::Volts(5.0)));
        s.update(48000.0);

        for voice in 0..3 {
            let p = s.state.osc_states[voice];
            assert!(
                p.abs() < 1e-6,
                "voice {voice} phase {p} should reset to 0 on hard sync"
            );
        }
    }

    #[test]
    fn synced_output_stays_bounded_across_many_resets() {
        // A fast master sync against a slower slave fires resets constantly,
        // exercising the PolyBLEP correction + carry every few samples.
        let mut s = make_supersaw(SupersawParams {
            freq: PolySignal::mono(Signal::Volts(-1.0)),
            voices: 5,
            detune: Some(PolySignal::mono(Signal::Volts(0.3))),
            fm: None,
            fm_mode: FmMode::default(),
            sync: Some(PolySignal::mono(Signal::Volts(0.0))),
        });

        // Toggle the sync input low/high every few samples to generate edges.
        for i in 0..2000 {
            let high = (i / 7) % 2 == 0;
            s.params.sync = Some(PolySignal::mono(Signal::Volts(if high {
                5.0
            } else {
                0.0
            })));
            s.update(48000.0);
            for voice in 0..5 {
                let v = s.outputs.sample.get(voice);
                assert!(v.is_finite(), "output must stay finite, got {v}");
                assert!(v.abs() <= 6.0, "output {v} should stay bounded near ±5V");
            }
        }
    }

    #[test]
    fn test_gain_compensation() {
        // With 4 input channels, gain should be 5.0 / sqrt(4) = 2.5
        // With 1 input channel, gain should be 5.0 / sqrt(1) = 5.0
        // A single voice with single input: a saw at mid-phase should give ~0 * 5.0
        let mut s1 = make_supersaw(SupersawParams {
            freq: PolySignal::mono(Signal::Volts(0.0)),
            voices: 1,
            detune: Some(PolySignal::mono(Signal::Volts(0.0))),
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        });
        // Set phase to 0.75 -> naive saw = 2*0.75 - 1 = 0.5
        s1.state.osc_states[0] = 0.75;
        s1.update(48000.0);
        let val_mono = s1.outputs.sample.get(0);

        // With 4 input channels, each contributing the same signal
        let mut s4 = make_supersaw(SupersawParams {
            freq: PolySignal::poly(&[
                Signal::Volts(0.0),
                Signal::Volts(0.0),
                Signal::Volts(0.0),
                Signal::Volts(0.0),
            ]),
            voices: 1,
            detune: Some(PolySignal::mono(Signal::Volts(0.0))),
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
        });
        // Set all 4 input channel phases identically
        for input_ch in 0..4 {
            s4.state.osc_states[input_ch * PORT_MAX_CHANNELS] = 0.75;
        }
        s4.update(48000.0);
        let val_quad = s4.outputs.sample.get(0);

        // val_quad should be 4 saws * (5/sqrt(4)) = 4 * saw * 2.5
        // val_mono should be 1 saw * 5.0
        // So val_quad / val_mono ≈ (4 * 2.5) / (1 * 5.0) = 2.0
        let ratio = val_quad / val_mono;
        assert!(
            (ratio - 2.0).abs() < 0.1,
            "Gain compensation ratio should be ~2.0, got {ratio}"
        );
    }
}
