//! Three-band crossover / band splitter module.
//!
//! Splits input into low, mid, and high frequency bands using
//! Linkwitz-Riley 4th-order crossover filters.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::utils::{changed, sanitize, voct_to_hz},
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
    types::Clickless,
};

// Default crossover frequencies in V/Oct
// 200 Hz: hz_to_voct(200.0) = log2(200 / 261.626) ≈ -0.389
const DEFAULT_LOW_MID_FREQ_VOCT: f32 = -0.389;
// 2000 Hz: hz_to_voct(2000.0) = log2(2000 / 261.626) ≈ 2.935
const DEFAULT_MID_HIGH_FREQ_VOCT: f32 = 2.935;

const BUTTERWORTH_Q: f32 = 0.707_107; // 1/sqrt(2)

// ── Params & Outputs ─────────────────────────────────────────────────────────

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct CrossoverParams {
    /// audio input signal
    input: PolySignal,
    /// crossover frequency between low and mid bands (V/Oct, 0V = C4)
    #[deserr(default)]
    low_mid_freq: Option<PolySignal>,
    /// crossover frequency between mid and high bands (V/Oct, 0V = C4)
    #[deserr(default)]
    mid_high_freq: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CrossoverOutputs {
    #[output("output", "input passed through unchanged", default)]
    sample: PolyOutput,
    #[output("low", "low band output")]
    low: PolyOutput,
    #[output("mid", "mid band output")]
    mid: PolyOutput,
    #[output("high", "high band output")]
    high: PolyOutput,
}

// ── Biquad types ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Default)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, input: f32, c: &BiquadCoeffs) -> f32 {
        let w = input - c.a1 * self.z1 - c.a2 * self.z2;
        let w = sanitize(w);
        let y = c.b0 * w + c.b1 * self.z1 + c.b2 * self.z2;
        self.z2 = self.z1;
        self.z1 = w;
        y
    }
}

/// Compute lowpass biquad coefficients (Butterworth, Q = 1/sqrt(2)).
fn compute_lp_coeffs(freq: f32, sample_rate: f32) -> BiquadCoeffs {
    let freq = freq.min(sample_rate * 0.45).max(20.0);
    let omega = 2.0 * std::f32::consts::PI * freq / sample_rate;
    let sin_w = omega.sin();
    let cos_w = omega.cos();
    let alpha = sin_w / (2.0 * BUTTERWORTH_Q);

    let b0 = (1.0 - cos_w) / 2.0;
    let b1 = 1.0 - cos_w;
    let b2 = (1.0 - cos_w) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w;
    let a2 = 1.0 - alpha;

    BiquadCoeffs {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    }
}

/// Compute highpass biquad coefficients (Butterworth, Q = 1/sqrt(2)).
fn compute_hp_coeffs(freq: f32, sample_rate: f32) -> BiquadCoeffs {
    let freq = freq.min(sample_rate * 0.45).max(20.0);
    let omega = 2.0 * std::f32::consts::PI * freq / sample_rate;
    let sin_w = omega.sin();
    let cos_w = omega.cos();
    let alpha = sin_w / (2.0 * BUTTERWORTH_Q);

    let b0 = (1.0 + cos_w) / 2.0;
    let b1 = -(1.0 + cos_w);
    let b2 = (1.0 + cos_w) / 2.0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w;
    let a2 = 1.0 - alpha;

    BiquadCoeffs {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    }
}

// ── Per-channel state ────────────────────────────────────────────────────────

/// Crossover filter state for one crossover point (LR4 = 2 cascaded biquads).
#[derive(Clone, Copy, Default)]
struct Lr4State {
    stage1: BiquadState,
    stage2: BiquadState,
}

impl Lr4State {
    #[inline]
    fn process(&mut self, input: f32, coeffs: &BiquadCoeffs) -> f32 {
        let mid = self.stage1.process(input, coeffs);
        self.stage2.process(mid, coeffs)
    }
}

#[derive(Clone, Copy, Default)]
struct ChannelState {
    // Crossover filter states (LR4 = two cascaded biquads each)
    // LOW band: 2 LP biquads at low_mid_freq
    low_lp: Lr4State,
    // MID band: 2 HP biquads at low_mid_freq, then 2 LP biquads at mid_high_freq
    mid_hp: Lr4State,
    mid_lp: Lr4State,
    // HIGH band: 2 HP biquads at mid_high_freq
    high_hp: Lr4State,

    // Cached crossover coefficients
    low_mid_lp_coeffs: BiquadCoeffs,
    low_mid_hp_coeffs: BiquadCoeffs,
    mid_high_lp_coeffs: BiquadCoeffs,
    mid_high_hp_coeffs: BiquadCoeffs,

    // Change detection for crossover frequencies
    last_low_mid_freq: f32,
    last_mid_high_freq: f32,

    // Clickless smoothing for crossover frequencies
    smooth_low_mid: Clickless,
    smooth_mid_high: Clickless,
}

/// State for the Crossover module.
#[derive(Default)]
struct CrossoverState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

// ── Module ───────────────────────────────────────────────────────────────────

/// EXPERIMENTAL
///
/// Three-band crossover / band splitter.
///
/// Splits an input signal into three frequency bands (low, mid, high).
/// The default `sample` output passes the input through unchanged,
/// so the module is a no-op unless you explicitly tap the
/// `.low`, `.mid`, or `.high` outputs.
///
/// Two crossover frequencies define the band boundaries:
/// - **lowMidFreq** — boundary between the low and mid bands (V/Oct, default ~200 Hz).
/// - **midHighFreq** — boundary between the mid and high bands (V/Oct, default ~2000 Hz).
///
/// ```js
/// // Split into 3 bands and process each independently
/// let bands = $xover(input, { lowMidFreq: '200hz', midHighFreq: '2000hz' })
/// let low  = $comp(bands.low,  { threshold: 2.5, ratio: 4 })
/// let mid  = $comp(bands.mid,  { threshold: 3,   ratio: 3 })
/// let high = $comp(bands.high, { threshold: 2,   ratio: 6 })
/// $mix([low, mid, high]).out()
/// ```
#[module(name = "$xover", args(input))]
pub struct Crossover {
    outputs: CrossoverOutputs,
    state: CrossoverState,
    params: CrossoverParams,
}

impl Crossover {
    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();

        for ch in 0..channels {
            let state = &mut self.state.channels[ch];

            let input = self.params.input.get_value(ch);

            // ── Read and smooth crossover frequencies ────────────────────
            let low_mid_voct = self
                .params
                .low_mid_freq
                .value_or(ch, DEFAULT_LOW_MID_FREQ_VOCT);
            let mid_high_voct = self
                .params
                .mid_high_freq
                .value_or(ch, DEFAULT_MID_HIGH_FREQ_VOCT);

            state.smooth_low_mid.update(low_mid_voct);
            state.smooth_mid_high.update(mid_high_voct);

            let low_mid_voct_smooth = *state.smooth_low_mid;
            let mid_high_voct_smooth = *state.smooth_mid_high;

            // ── Recompute coefficients if frequencies changed ────────────
            if changed(low_mid_voct_smooth, state.last_low_mid_freq) {
                let freq = voct_to_hz(low_mid_voct_smooth);
                state.low_mid_lp_coeffs = compute_lp_coeffs(freq, sample_rate);
                state.low_mid_hp_coeffs = compute_hp_coeffs(freq, sample_rate);
                state.last_low_mid_freq = low_mid_voct_smooth;
            }

            if changed(mid_high_voct_smooth, state.last_mid_high_freq) {
                let freq = voct_to_hz(mid_high_voct_smooth);
                state.mid_high_lp_coeffs = compute_lp_coeffs(freq, sample_rate);
                state.mid_high_hp_coeffs = compute_hp_coeffs(freq, sample_rate);
                state.last_mid_high_freq = mid_high_voct_smooth;
            }

            // ── Split into 3 bands using LR4 crossover ──────────────────
            // Low band: input → LPF₁ → LPF₂ (at low_mid_freq)
            let low_band = state.low_lp.process(input, &state.low_mid_lp_coeffs);

            // Mid band: input → HPF₁ → HPF₂ (at low_mid_freq) → LPF₁ → LPF₂ (at mid_high_freq)
            let mid_hp_out = state.mid_hp.process(input, &state.low_mid_hp_coeffs);
            let mid_band = state.mid_lp.process(mid_hp_out, &state.mid_high_lp_coeffs);

            // High band: input → HPF₁ → HPF₂ (at mid_high_freq)
            let high_band = state.high_hp.process(input, &state.mid_high_hp_coeffs);

            // ── Write outputs ───────────────────────────────────────────
            self.outputs.sample.set(ch, input);
            self.outputs.low.set(ch, low_band);
            self.outputs.mid.set(ch, mid_band);
            self.outputs.high.set(ch, high_band);
        }
    }
}

message_handlers!(impl Crossover {});
