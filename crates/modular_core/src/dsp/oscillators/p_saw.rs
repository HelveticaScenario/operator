use crate::{
    dsp::utils::wrap,
    poly::{PolyOutput, PolySignal, PolySignalExt},
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
    /// Previous phase, kept in f64 so the DPW differencing (which divides a
    /// difference of near-equal integral values by the per-sample phase delta)
    /// stays well-conditioned when that delta is tiny at slow phasor rates.
    prev_phase: f64,
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
    channel_state: Box<[ChannelState]>,
    params: PSawOscillatorParams,
}

impl PSawOscillator {
    fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];

            // Update shape with smoothing - clamp to valid range
            let shape_val = self.params.shape.value_or(ch, 0.0).clamp(0.0, 5.0);
            state.shape.update(shape_val);

            let phase = wrap(0.0..1.0, self.params.phase.get_value(ch)) as f64;

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
            let s = (1.0 - *state.shape * 0.2).clamp(0.001, 0.999) as f64;

            // DPW (Differentiated Polynomial Waveform) method:
            // Compute the smooth anti-derivative of the waveform at both the
            // previous and current phase, then differentiate numerically.
            // The numeric differentiation naturally band-limits the output.
            //
            // The differencing is done in f64: at slow (LFO-rate) phasor inputs
            // the per-sample phase delta is tiny, so the difference of near-equal
            // integral values and its division must keep enough precision that
            // f32 rounding does not amplify into per-sample hash.
            let raw_output = if phase_increment > 1.0e-7 {
                let integral_old = triangle_integral(state.prev_phase, s);
                let integral_new = triangle_integral(phase, s);
                (integral_new - integral_old) / phase_increment
            } else {
                // Near-DC fallback: use naive waveform (no aliasing at low freq)
                naive_triangle(phase, s)
            };

            state.prev_phase = phase;
            self.outputs.sample.set(ch, raw_output as f32 * 5.0);
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
fn triangle_integral(phase: f64, s: f64) -> f64 {
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
fn naive_triangle(phase: f64, s: f64) -> f64 {
    if phase < s {
        2.0 * phase / s - 1.0
    } else {
        1.0 - 2.0 * (phase - s) / (1.0 - s)
    }
}

message_handlers!(impl PSawOscillator {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::{OutputStruct, Signal};

    #[test]
    fn low_rate_phase_ramp_is_a_clean_ramp() {
        // Driving $pSaw with a slow external phasor (LFO rate) must yield a
        // smooth ramp. The DPW body divides a difference of near-equal integral
        // values by the per-sample phase delta; when that delta is tiny the
        // differencing must keep enough precision that f32 rounding does not
        // blow up into per-sample hash. Mirrors the saw.rs regression.
        let params = PSawOscillatorParams {
            phase: PolySignal::mono(Signal::Volts(0.0)),
            shape: None, // 0 → saw
        };
        let channels = params.phase.channels().max(1);
        let mut outputs = PSawOscillatorOutputs::default();
        outputs.set_all_channels(channels);
        let mut osc = PSawOscillator {
            params,
            outputs,
            _channel_count: channels,
            _block_index: Default::default(),
            channel_state: vec![ChannelState::default(); channels].into_boxed_slice(),
        };

        // 0.2 Hz at 48 kHz ⇒ the phasor advances 0.2/48000 per sample. Start
        // mid-ramp (0.5) so 2000 samples stay on the rising segment (no wrap).
        let inc = 0.2_f32 / 48_000.0;
        let mut out = Vec::with_capacity(2000);
        for i in 0..2000 {
            let ph = 0.5 + inc * i as f32;
            osc.params.phase = PolySignal::mono(Signal::Volts(ph));
            osc.update(48_000.0);
            out.push(osc.outputs.sample.get(0));
        }

        // Skip the first samples (the initial step from prev_phase = 0), then
        // measure the slope of the ramp.
        let diffs: Vec<f32> = out[10..].windows(2).map(|w| w[1] - w[0]).collect();
        let mean = diffs.iter().sum::<f32>() / diffs.len() as f32;
        let std =
            (diffs.iter().map(|d| (d - mean).powi(2)).sum::<f32>() / diffs.len() as f32).sqrt();

        assert!(
            mean > 0.0,
            "ramp should rise; mean per-sample step = {mean}"
        );
        // Pre-fix f32 DPW produced step std ~0.03 here; a clean ramp sits at the
        // f32 output-quantization floor (~1e-8).
        assert!(
            std < 1.0e-3,
            "per-sample step std {std} indicates high-frequency hash (clean ramp ≈ 0)"
        );
    }
}
