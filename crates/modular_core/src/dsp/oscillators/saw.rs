use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    dsp::utils::voct_to_hz,
    poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS},
    types::Clickless,
};

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct SawOscillatorParams {
    /// pitch in V/Oct (0V = C4)
    #[signal(type = pitch)]
    freq: Option<PolySignal>,
    /// waveform shape: 0=saw, 2.5=triangle, 5=ramp
    #[signal(range = (0.0, 5.0))]
    shape: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SawOscillatorOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Per-channel oscillator state
#[derive(Default, Clone, Copy)]
struct ChannelState {
    phase: f32,
    shape: Clickless,
}

/// State for the SawOscillator module.
#[derive(Default)]
struct SawOscillatorState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// A variable-symmetry triangle oscillator that morphs between saw, triangle, and ramp.
///
/// The `shape` parameter shifts the peak position of a triangle wave,
/// smoothly morphing between waveforms by adjusting attack/release time:
/// - **0** — Saw (all rise, instant drop)
/// - **2.5** — Triangle (symmetric)
/// - **5** — Ramp (instant rise, all fall)
///
/// The `freq` input follows the **V/Oct** standard (0V = C4).
/// Output range is **±5V**.
///
/// ## Example
///
/// ```js
/// $saw('a3', { shape: 2.5 }).out() // triangle wave
/// ```
#[module(name = "$saw", args(freq))]
pub struct SawOscillator {
    outputs: SawOscillatorOutputs,
    state: SawOscillatorState,
    params: SawOscillatorParams,
}

impl SawOscillator {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        // Pre-compute inverse sample rate for frequency calculation
        let inv_sample_rate = 1.0 / sample_rate;

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            // Update shape with smoothing - clamp to valid range
            let shape_val = self.params.shape.value_or(ch, 0.0).clamp(0.0, 5.0);
            state.shape.update(shape_val);

            let frequency = voct_to_hz(self.params.freq.value_or(ch, 0.0));
            let phase_increment = frequency * inv_sample_rate;

            // Convert shape (0–5) to symmetry (peak position):
            // 0 = saw (peak at 1.0), 2.5 = triangle (peak at 0.5), 5 = ramp (peak at 0.0)
            let s = (1.0 - *state.shape * 0.2).clamp(0.001, 0.999);

            // DPW: compute integral at current phase BEFORE advancing
            let integral_old = triangle_integral(state.phase, s);

            // Advance phase
            state.phase += phase_increment;
            if state.phase >= 1.0 {
                state.phase -= 1.0;
            }

            // DPW: compute integral at new phase, differentiate
            let integral_new = triangle_integral(state.phase, s);

            let raw_output = if phase_increment > 1.0e-7 {
                (integral_new - integral_old) / phase_increment
            } else {
                // Near-DC fallback: use naive waveform (no aliasing at low freq)
                naive_triangle(state.phase, s)
            };

            self.outputs.sample.set(ch, raw_output * 5.0);
        }
    }
}

/// Anti-derivative of the variable-symmetry triangle waveform.
///
/// This is a continuous, differentiable, periodic piecewise-parabolic function
/// (F(0) = F(1) = 0). Used by the DPW method: the numeric differentiation
/// `(F[n] - F[n-1]) / dt` naturally band-limits the output without requiring
/// any explicit PolyBLEP/PolyBLAMP corrections.
#[inline(always)]
fn triangle_integral(phase: f32, s: f32) -> f32 {
    if phase < s {
        // Integral of rising segment: f(p) = 2p/s - 1  →  F(p) = p²/s - p
        phase * phase / s - phase
    } else {
        // Integral of falling segment: f(p) = 1 - 2(p-s)/(1-s)  →  F(p) = p - (p-s)²/(1-s) - s
        let d = phase - s;
        phase - d * d / (1.0 - s) - s
    }
}

/// Naive variable-symmetry triangle (used as fallback at near-zero frequency).
#[inline(always)]
fn naive_triangle(phase: f32, s: f32) -> f32 {
    if phase < s {
        2.0 * phase / s - 1.0
    } else {
        1.0 - 2.0 * (phase - s) / (1.0 - s)
    }
}

message_handlers!(impl SawOscillator {});
