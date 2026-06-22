use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::{
        oscillators::{FmMode, apply_fm, sync_blep, sync_edge_fraction},
        utils::SchmittTrigger,
    },
    poly::{PolyOutput, PolySignal, PolySignalExt},
    types::Clickless,
};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct SawOscillatorParams {
    /// pitch in V/Oct (0V = C4)
    #[signal(type = pitch)]
    freq: PolySignal,
    /// waveform shape: 0=saw, 2.5=triangle, 5=ramp
    #[signal(range = (0.0, 5.0))]
    #[deserr(default)]
    shape: Option<PolySignal>,
    /// FM input signal (pre-scaled by user)
    #[deserr(default)]
    fm: Option<PolySignal>,
    /// FM mode: throughZero (default), lin, or exp
    #[serde(default)]
    #[deserr(default)]
    fm_mode: FmMode,
    /// hard sync source — rising edges reset the oscillator phase
    #[deserr(default)]
    sync: Option<PolySignal>,
    /// phase offset in [0, 1) added to the internal phase before sampling
    #[signal(default = 0.0, range = (0.0, 1.0))]
    #[deserr(default)]
    phase_offset: Option<PolySignal>,
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
    /// Edge detector for the sync input.
    sync_schmitt: SchmittTrigger,
    /// Previous sync-input sample, for subsample edge interpolation.
    sync_prev: f32,
    /// PolyBLEP residual carried into the next sample from a sync reset.
    blep_carry: f32,
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
    channel_state: Box<[ChannelState]>,
    params: SawOscillatorParams,
}

impl SawOscillator {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        // Pre-compute inverse sample rate for frequency calculation
        let inv_sample_rate = 1.0 / sample_rate;

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];

            // Update shape with smoothing - clamp to valid range
            let shape_val = self.params.shape.value_or(ch, 0.0).clamp(0.0, 5.0);
            state.shape.update(shape_val);

            let pitch = self.params.freq.get_value(ch);
            let fm = self.params.fm.value_or(ch, 0.0);
            let frequency = apply_fm(pitch, fm, self.params.fm_mode);
            let phase_increment = frequency * inv_sample_rate;

            // Convert shape (0–5) to symmetry (peak position):
            // 0 = saw (peak at 1.0), 2.5 = triangle (peak at 0.5), 5 = ramp (peak at 0.0)
            let s = (1.0 - *state.shape * 0.2).clamp(0.001, 0.999);

            // Phase offset shifts the read position without altering the
            // accumulator, so it never drifts.
            let offset = self.params.phase_offset.value_or(ch, 0.0);
            let read_offset = offset.rem_euclid(1.0);

            // DPW: compute integral at current phase BEFORE advancing
            let read_old = (state.phase + read_offset).rem_euclid(1.0);
            let integral_old = triangle_integral(read_old, s);

            // Advance phase (rem_euclid supports negative increments from through-zero FM)
            state.phase += phase_increment;
            state.phase = state.phase.rem_euclid(1.0);
            let read_phase = (state.phase + read_offset).rem_euclid(1.0);

            // DPW body for this sample. The sync reset (below) lands in the
            // upcoming interval, so the phase advanced normally here and the DPW
            // differentiation stays valid; the reset is band-limited separately.
            let body = if phase_increment.abs() > 1.0e-7 {
                let integral_new = triangle_integral(read_phase, s);
                (integral_new - integral_old) / phase_increment
            } else {
                // Near-DC fallback: use naive waveform (no aliasing at low freq)
                naive_triangle(read_phase, s)
            };

            let pending = state.blep_carry;
            state.blep_carry = 0.0;

            // Hard sync: a rising edge resets the phase, with a PolyBLEP placed
            // at the subsample crossing to band-limit the reset discontinuity.
            let mut now = 0.0;
            if let Some(sync) = &self.params.sync {
                let v = sync.get_value(ch);
                if state.sync_schmitt.process(v) {
                    let frac = sync_edge_fraction(state.sync_prev, v);
                    let before = naive_triangle(read_phase, s);
                    state.phase = 0.0;
                    let after = naive_triangle(read_offset, s);
                    let (n, carry) = sync_blep(after - before, frac);
                    now = n;
                    state.blep_carry = carry;
                }
                state.sync_prev = v;
            }

            self.outputs.sample.set(ch, (body + pending + now) * 5.0);
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
