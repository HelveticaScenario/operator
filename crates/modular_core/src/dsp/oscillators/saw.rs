use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::{
        oscillators::{FmMode, apply_fm, sync_blep, sync_edge_fraction},
        utils::{SchmittTrigger, wrap_phase_f64},
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
    /// when true, a freshly started voice stays silent until its output first
    /// crosses zero. This prevents a click when a phase offset starts the
    /// voice away from a zero crossing. Default true.
    #[serde(default = "default_true")]
    #[deserr(default = default_true())]
    wait_for_zero_cross: bool,
}

fn default_true() -> bool {
    true
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
    /// Phase accumulator in `[0, 1)`. Kept in f64 so the DPW differencing
    /// (which divides a difference of near-equal integral values by the phase
    /// increment) stays well-conditioned at low, LFO-rate frequencies.
    phase: f64,
    shape: Clickless,
    /// False until the voice has seeded its starting phase to a zero crossing
    /// (done on the first `update`). A voice whose state was carried over by
    /// `transfer_state_from` arrives with this already set, so it keeps its
    /// phase for continuity instead of re-seeding.
    seeded: bool,
    /// Whether the zero-cross mute has released. Stays false while a freshly
    /// started voice is held silent, then latches true at the first output
    /// zero crossing. Carried-over (already-sounding) voices arrive with this
    /// true, so they keep playing without re-muting.
    gate_open: bool,
    /// Previous (un-muted) output sample, used to detect the zero crossing that
    /// releases the mute.
    prev_out: f32,
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
        let inv_sample_rate = 1.0 / sample_rate as f64;

        let wait_for_zero_cross = self.params.wait_for_zero_cross;

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];

            // Update shape with smoothing - clamp to valid range
            let shape_val = self.params.shape.value_or(ch, 0.0).clamp(0.0, 5.0);
            state.shape.update(shape_val);

            let pitch = self.params.freq.get_value(ch);
            let fm = self.params.fm.value_or(ch, 0.0);
            let frequency = apply_fm(pitch, fm, self.params.fm_mode);
            let phase_increment = frequency as f64 * inv_sample_rate;

            // Convert shape (0–5) to symmetry (peak position):
            // 0 = saw (peak at 1.0), 2.5 = triangle (peak at 0.5), 5 = ramp (peak at 0.0)
            let s = (1.0 - *state.shape * 0.2).clamp(0.001, 0.999) as f64;

            // Phase offset shifts the read position without altering the
            // accumulator, so it never drifts.
            let offset = self.params.phase_offset.value_or(ch, 0.0);
            let read_offset = offset.rem_euclid(1.0) as f64;

            // Seed a fresh voice so its read position starts just before the
            // gentle zero crossing, then the mute below holds silence until it
            // arrives. The waveform crosses zero mid-rise (phase s/2) and
            // mid-fall (phase (1+s)/2); the crossing on the longer of the two
            // segments is the gentle one (the other is the steep, band-limited
            // edge). We start the read `read_offset` of a cycle before that
            // crossing, so the wait equals the phase offset — a small offset
            // gives a short wait, not nearly a whole cycle, while still honoring
            // the offset in the running phase. The running read adds read_offset
            // on top of the accumulator, so the seed subtracts twice it to land
            // the read at `zero_cross - read_offset`. Only fresh voices seed;
            // carried-over state keeps its phase (see `seeded`).
            if !state.seeded {
                let zero_cross = if s >= 0.5 { s * 0.5 } else { (1.0 + s) * 0.5 };
                state.phase = (zero_cross - 2.0 * read_offset).rem_euclid(1.0);
                state.seeded = true;
                // Arm the zero-cross mute. With no offset the read starts on the
                // crossing, so prev_out is 0 and the mute releases on the first
                // sample; an offset starts it earlier, holding silence until the
                // output reaches the crossing.
                state.gate_open = !wait_for_zero_cross;
                let read_start = (zero_cross - read_offset).rem_euclid(1.0);
                state.prev_out = naive_triangle(read_start, s) as f32 * 5.0;
            }

            // DPW: compute integral at current phase BEFORE advancing
            let read_old = (state.phase + read_offset).rem_euclid(1.0);
            let integral_old = triangle_integral(read_old, s);

            state.phase = wrap_phase_f64(state.phase + phase_increment);
            let read_phase = (state.phase + read_offset).rem_euclid(1.0);

            // DPW body for this sample. The sync reset (below) lands in the
            // upcoming interval, so the phase advanced normally here and the DPW
            // differentiation stays valid; the reset is band-limited separately.
            let body = if phase_increment.abs() > 1.0e-7 {
                // The DPW slope is computed in f64: integral_new and
                // integral_old are near-equal when the phase increment is small
                // (low frequency), so the difference and its division must keep
                // enough precision that f32 rounding does not get amplified into
                // per-sample hash on slow, LFO-rate outputs.
                let integral_new = triangle_integral(read_phase, s);
                ((integral_new - integral_old) / phase_increment) as f32
            } else {
                // Near-DC fallback: use naive waveform (no aliasing at low freq)
                naive_triangle(read_phase, s) as f32
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
                    let (n, carry) = sync_blep((after - before) as f32, frac);
                    now = n;
                    state.blep_carry = carry;
                }
                state.sync_prev = v;
            }

            let out = (body + pending + now) * 5.0;

            // Zero-cross mute: hold silence until the output crosses zero on its
            // gentle segment, then latch open. The waveform also crosses zero on
            // its steep edge — releasing there would itself click — so only the
            // crossing that matches the seed's longer segment counts: rising
            // (negative→positive) for saw-side shapes, falling for ramp-side.
            // Releasing on the gentle slope means the first audible sample is ~0.
            let crossed = if s >= 0.5 {
                state.prev_out <= 0.0 && out >= 0.0
            } else {
                state.prev_out >= 0.0 && out <= 0.0
            };
            let sample = if state.gate_open {
                out
            } else if crossed {
                state.gate_open = true;
                out
            } else {
                0.0
            };
            state.prev_out = out;

            self.outputs.sample.set(ch, sample);
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

message_handlers!(impl SawOscillator {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::{OutputStruct, Signal};

    fn make_saw(phase_offset: f32, wait_for_zero_cross: bool) -> SawOscillator {
        let params = SawOscillatorParams {
            freq: PolySignal::mono(Signal::Volts(0.0)), // C4 ≈ 261 Hz
            shape: None,                                // 0 → saw
            fm: None,
            fm_mode: FmMode::default(),
            sync: None,
            phase_offset: Some(PolySignal::mono(Signal::Volts(phase_offset))),
            wait_for_zero_cross,
        };
        let channels = params.freq.channels().max(1);
        let mut outputs = SawOscillatorOutputs::default();
        outputs.set_all_channels(channels);
        SawOscillator {
            params,
            outputs,
            _channel_count: channels,
            _block_index: Default::default(),
            channel_state: vec![ChannelState::default(); channels].into_boxed_slice(),
        }
    }

    /// Collect `n` output samples from channel 0.
    fn run(osc: &mut SawOscillator, n: usize) -> Vec<f32> {
        (0..n)
            .map(|_| {
                osc.update(48_000.0);
                osc.outputs.sample.get(0)
            })
            .collect()
    }

    #[test]
    fn offset_start_is_muted_until_a_zero_crossing() {
        // A phase offset starts the voice away from a zero crossing. With the
        // mute on, the output is held silent for many samples and then releases
        // on a near-zero sample (no click).
        let mut osc = make_saw(0.25, true);
        let out = run(&mut osc, 400);

        assert_eq!(out[0], 0.0, "first sample should be muted");
        let release = out.iter().position(|&v| v != 0.0).expect("must release");
        assert!(
            release > 1,
            "should hold silence past the first sample, released at {release}"
        );
        assert!(
            out[release].abs() < 0.5,
            "release sample {} should be near zero (click-free)",
            out[release]
        );

        // The wait tracks the offset itself (~0.25 of a cycle), not its
        // complement: starting just before the gentle crossing avoids waiting
        // nearly a whole extra oscillation. C4 ≈ 261.63 Hz → period ≈ 183.5
        // samples, so 0.25 of a cycle ≈ 46 samples.
        let period = 48_000.0 / 261.63;
        let expected = 0.25 * period;
        assert!(
            (release as f32 - expected).abs() < 0.05 * period,
            "release {release} should be ~{expected:.0} samples (offset·period), not its complement"
        );
    }

    #[test]
    fn offset_start_clicks_when_mute_disabled() {
        // With the mute off, the same offset start emits its (non-zero) value
        // immediately — the offset is honored at the cost of a click.
        let mut osc = make_saw(0.25, false);
        let out = run(&mut osc, 4);
        assert!(
            out[0].abs() > 0.5,
            "first sample {} should be non-zero",
            out[0]
        );
    }

    #[test]
    fn zero_offset_starts_immediately_on_the_crossing() {
        // No offset: the seed already lands on a zero crossing, so the mute
        // releases right away even though it is enabled.
        let mut osc = make_saw(0.0, true);
        let out = run(&mut osc, 8);
        let release = out.iter().position(|&v| v != 0.0).expect("must release");
        assert!(
            release <= 1,
            "zero-offset voice should sound immediately, released at {release}"
        );
    }

    #[test]
    fn low_frequency_output_is_a_clean_ramp() {
        // A sub-audio (LFO-rate) saw must be a smooth ramp. The DPW body divides
        // a difference of near-equal integral values by the phase increment;
        // when that increment is tiny (low frequency) the division must keep
        // enough precision that f32 rounding does not blow up into per-sample
        // hash. Regression for the audible high-frequency artifact heard when a
        // 0.2 Hz saw is used as an amplitude LFO.
        let v = (0.2_f32 / 261.625_58).log2(); // 0.2 Hz expressed in V/Oct
        let mut osc = make_saw(0.0, true);
        osc.params.freq = PolySignal::mono(Signal::Volts(v));

        // 0.2 Hz ⇒ ~240k samples/cycle, so 2000 samples stay on the rising
        // segment. Skip the seed/mute-release samples and measure the slope.
        let out = run(&mut osc, 2000);
        let diffs: Vec<f32> = out[50..].windows(2).map(|w| w[1] - w[0]).collect();
        let mean = diffs.iter().sum::<f32>() / diffs.len() as f32;
        let std =
            (diffs.iter().map(|d| (d - mean).powi(2)).sum::<f32>() / diffs.len() as f32).sqrt();

        assert!(
            mean > 0.0,
            "ramp should rise; mean per-sample step = {mean}"
        );
        // The pre-fix f32 DPW produced a step std ~0.04 (≈600x the true slope);
        // a clean ramp has step std at the f32 output-quantization floor (~1e-6).
        assert!(
            std < 1.0e-3,
            "per-sample step std {std} indicates high-frequency hash (clean ramp ≈ 0)"
        );
    }
}
