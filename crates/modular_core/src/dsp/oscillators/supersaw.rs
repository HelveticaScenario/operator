use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    dsp::utils::voct_to_hz,
    poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS},
};

fn default_voices() -> usize {
    5
}

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct SupersawParams {
    /// pitch in V/Oct (0V = C4)
    freq: Option<PolySignal>,
    /// number of supersaw voices (1–16)
    #[serde(default = "default_voices")]
    voices: usize,
    /// detune spread in semitones (default 0.18)
    #[signal(type = control, default = 0.18, range = (0, 12))]
    detune: Option<PolySignal>,
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

/// Phase state array for supersaw oscillator voices.
/// Wraps `[f32; PORT_MAX_CHANNELS * PORT_MAX_CHANNELS]` to provide a `Default` impl
/// (since Rust stable doesn't impl Default for arrays > 32 elements).
struct OscStates([f32; PORT_MAX_CHANNELS * PORT_MAX_CHANNELS]);

impl Default for OscStates {
    fn default() -> Self {
        Self([0.0; PORT_MAX_CHANNELS * PORT_MAX_CHANNELS])
    }
}

impl std::ops::Deref for OscStates {
    type Target = [f32; PORT_MAX_CHANNELS * PORT_MAX_CHANNELS];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for OscStates {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
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
#[module(name = "$supersaw", channels_derive = supersaw_derive_channel_count, has_init, args(freq))]
pub struct Supersaw {
    outputs: SupersawOutputs,
    params: SupersawParams,
    state: SupersawState,
}

/// State for the Supersaw module.
#[derive(Default)]
struct SupersawState {
    /// Phase state for matrix mixing: indexed as [input_ch * PORT_MAX_CHANNELS + voice]
    osc_states: OscStates,
    rng_state: u32,
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
    fn init(&mut self, _sample_rate: f32) {
        self.state.rng_state = self as *const Self as usize as u32;
        for i in 0..self.state.osc_states.len() {
            self.state.osc_states[i] = rand_phase(&mut self.state.rng_state);
        }
    }

    fn update(&mut self, sample_rate: f32) {
        let voices = self.params.voices.clamp(1, PORT_MAX_CHANNELS);
        let input_channels = self.params.freq.channel_count().max(1);

        let inv_sample_rate = 1.0 / sample_rate;

        // Gain: 5V range, compensated for input channel count
        let gain = 5.0 / (input_channels as f32).sqrt();

        // Voice interpolation factor (precompute per voice)
        // Interleaved ordering: first half of voices gets even detune positions,
        // second half gets odd positions. This ensures each half contains a
        // balanced spread across the full detune range, so splitting voices
        // into two groups (e.g. for stereo panning) gives symmetric detuning
        // on each side — matching Strudel's alternating L/R distribution.
        let voice_t: [f32; PORT_MAX_CHANNELS] = {
            let mut t = [0.0f32; PORT_MAX_CHANNELS];
            let half = (voices + 1) / 2;
            for v in 0..voices {
                let linear_pos = if v < half { v * 2 } else { (v - half) * 2 + 1 };
                t[v] = if voices > 1 {
                    linear_pos as f32 / (voices - 1) as f32
                } else {
                    0.5 // centered, offset will be 0
                };
            }
            t
        };

        for voice in 0..voices {
            let mut accum = 0.0f32;

            for input_ch in 0..input_channels {
                // Detune channel-matches input pitch (per-note detune)
                let detune = self.params.detune.value_or(input_ch, 0.18);
                let offset_semitones = if voices > 1 {
                    // lerp from -detune/2 to +detune/2
                    -detune / 2.0 + voice_t[voice] * detune
                } else {
                    0.0
                };

                let pitch = self.params.freq.value_or(input_ch, 0.0);
                let freq = voct_to_hz(pitch) * (2.0f32).powf(offset_semitones / 12.0);
                let dt = freq * inv_sample_rate;

                let state_idx = input_ch * PORT_MAX_CHANNELS + voice;
                let phase = &mut self.state.osc_states[state_idx];

                // Advance phase
                *phase += dt;
                while *phase >= 1.0 {
                    *phase -= 1.0;
                }

                // Naive saw: maps [0,1) to [-1,1)
                let mut saw = 2.0 * *phase - 1.0;

                // Apply PolyBLEP correction
                saw -= poly_blep_saw(*phase, dt);

                accum += saw;
            }

            self.outputs.sample.set(voice, accum * gain);
        }
    }
}

message_handlers!(impl Supersaw {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    /// Create a Supersaw with params and properly initialize channel count and output channels.
    fn make_supersaw(params: SupersawParams) -> Supersaw {
        let channels = supersaw_derive_channel_count(&params);
        let mut outputs = SupersawOutputs::default();
        outputs.set_all_channels(channels);
        Supersaw {
            params,
            outputs,
            _channel_count: channels,
            state: SupersawState::default(),
        }
    }

    #[test]
    fn test_single_voice_output() {
        let mut s = make_supersaw(SupersawParams {
            freq: Some(PolySignal::mono(Signal::Volts(0.0))),
            voices: 1,
            detune: None,
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
            freq: Some(PolySignal::mono(Signal::Volts(0.0))),
            voices: 7,
            detune: None,
        };
        assert_eq!(supersaw_derive_channel_count(&params), 7);
    }

    #[test]
    fn test_output_bounded() {
        let mut s = make_supersaw(SupersawParams {
            freq: Some(PolySignal::mono(Signal::Volts(0.0))),
            voices: 5,
            detune: None,
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
            freq: Some(PolySignal::mono(Signal::Volts(0.0))),
            voices: 32,
            detune: None,
        };
        assert_eq!(supersaw_derive_channel_count(&params), 16);

        let params_zero = SupersawParams {
            freq: Some(PolySignal::mono(Signal::Volts(0.0))),
            voices: 0,
            detune: None,
        };
        assert_eq!(supersaw_derive_channel_count(&params_zero), 1);
    }

    #[test]
    fn test_detune_affects_pitch() {
        // With detune=0, all voices should be identical (same phase progression)
        let mut s_no_detune = make_supersaw(SupersawParams {
            freq: Some(PolySignal::mono(Signal::Volts(0.0))),
            voices: 3,
            detune: Some(PolySignal::mono(Signal::Volts(0.0))),
        });
        // Force known phases (overwrite whatever init set)
        for i in 0..PORT_MAX_CHANNELS * PORT_MAX_CHANNELS {
            s_no_detune.state.osc_states[i] = 0.25;
        }

        let mut s_detune = make_supersaw(SupersawParams {
            freq: Some(PolySignal::mono(Signal::Volts(0.0))),
            voices: 3,
            detune: Some(PolySignal::mono(Signal::Volts(2.0))),
        });
        for i in 0..PORT_MAX_CHANNELS * PORT_MAX_CHANNELS {
            s_detune.osc_states[i] = 0.25;
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
            freq: Some(PolySignal::mono(Signal::Volts(0.0))),
            voices: 4,
            detune: Some(PolySignal::mono(Signal::Volts(0.0))),
        });
        // Trigger phase initialization via init()
        s.init(48000.0);

        // Check that at least some initial phases differ
        // (statistically extremely unlikely all 4 are identical)
        let phases: Vec<f32> = (0..4).map(|v| s.osc_states[v]).collect();
        let all_same = phases.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-10);
        assert!(!all_same, "Random phases should not all be identical");
    }

    #[test]
    fn test_matrix_mixing_mono_input() {
        // Mono input with 3 voices -> 3 output channels
        let mut s = make_supersaw(SupersawParams {
            freq: Some(PolySignal::mono(Signal::Volts(0.0))),
            voices: 3,
            detune: Some(PolySignal::mono(Signal::Volts(0.0))),
        });
        // Force known phases, zero detune -> all voices should produce identical output
        for i in 0..PORT_MAX_CHANNELS * PORT_MAX_CHANNELS {
            s.osc_states[i] = 0.5;
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
    fn test_gain_compensation() {
        // With 4 input channels, gain should be 5.0 / sqrt(4) = 2.5
        // With 1 input channel, gain should be 5.0 / sqrt(1) = 5.0
        // A single voice with single input: a saw at mid-phase should give ~0 * 5.0
        let mut s1 = make_supersaw(SupersawParams {
            freq: Some(PolySignal::mono(Signal::Volts(0.0))),
            voices: 1,
            detune: Some(PolySignal::mono(Signal::Volts(0.0))),
        });
        // Set phase to 0.75 -> naive saw = 2*0.75 - 1 = 0.5
        s1.osc_states[0] = 0.75;
        s1.update(48000.0);
        let val_mono = s1.outputs.sample.get(0);

        // With 4 input channels, each contributing the same signal
        let mut s4 = make_supersaw(SupersawParams {
            freq: Some(PolySignal::poly(&[
                Signal::Volts(0.0),
                Signal::Volts(0.0),
                Signal::Volts(0.0),
                Signal::Volts(0.0),
            ])),
            voices: 1,
            detune: Some(PolySignal::mono(Signal::Volts(0.0))),
        });
        // Set all 4 input channel phases identically
        for input_ch in 0..4 {
            s4.osc_states[input_ch * PORT_MAX_CHANNELS] = 0.75;
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
