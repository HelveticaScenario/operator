use crate::{
    dsp::utils::wrap,
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
    types::Clickless,
};
use deserr::Deserr;
use schemars::JsonSchema;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct PSawOscillatorParams {
    /// phasor input (0–1, wraps at boundaries)
    #[signal(range = (0.0, 1.0))]
    phase: PolySignal,
    /// waveform shape: 0=saw, 2.5=triangle, 5=ramp
    #[signal(range = (0.0, 5.0))]
    #[deserr(default)]
    shape: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PSawOscillatorOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Per-channel state for shape smoothing and phase tracking
#[derive(Default, Clone, Copy)]
struct ChannelState {
    shape: Clickless,
    prev_phase: f32,
}

/// State for the PSawOscillator module.
#[derive(Default)]
struct PSawOscillatorState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// Phase-driven variable-symmetry triangle oscillator.
///
/// Instead of a frequency input, this oscillator is driven by an external
/// phasor signal (0–1). Connect a `ramp` or other phase source to `phase`
/// and use phase-distortion modules between them for complex timbres.
///
/// The `shape` parameter shifts the peak position of a triangle wave,
/// smoothly morphing between waveforms by adjusting attack/release time:
/// - **0** — Saw (all rise, instant drop)
/// - **2.5** — Triangle (symmetric)
/// - **5** — Ramp (instant rise, all fall)
///
/// Output range is **±5V**.
#[module(name = "$pSaw", args(phase))]
pub struct PSawOscillator {
    outputs: PSawOscillatorOutputs,
    state: PSawOscillatorState,
    params: PSawOscillatorParams,
}

impl PSawOscillator {
    fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            // Update shape with smoothing - clamp to valid range
            let shape_val = self.params.shape.value_or(ch, 0.0).clamp(0.0, 5.0);
            state.shape.update(shape_val);

            let phase = wrap(0.0..1.0, self.params.phase.get_value(ch));

            // Calculate phase increment from phase difference
            // Handle phase wrapping correctly
            let mut phase_increment = phase - state.prev_phase;
            if phase_increment < -0.5 {
                phase_increment += 1.0;
            } else if phase_increment > 0.5 {
                phase_increment -= 1.0;
            }
            phase_increment = phase_increment.abs().clamp(0.0, 0.5);

            // Convert shape (0–5) to symmetry (peak position):
            // 0 = saw (peak at 1.0), 2.5 = triangle (peak at 0.5), 5 = ramp (peak at 0.0)
            let s = (1.0 - *state.shape * 0.2).clamp(0.001, 0.999);

            // DPW (Differentiated Polynomial Waveform) method:
            // Compute the smooth anti-derivative of the waveform at both the
            // previous and current phase, then differentiate numerically.
            // The numeric differentiation naturally band-limits the output.
            let raw_output = if phase_increment > 1.0e-7 {
                let integral_old = triangle_integral(state.prev_phase, s);
                let integral_new = triangle_integral(phase, s);
                (integral_new - integral_old) / phase_increment
            } else {
                // Near-DC fallback: use naive waveform (no aliasing at low freq)
                naive_triangle(phase, s)
            };

            state.prev_phase = phase;
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

message_handlers!(impl PSawOscillator {});
