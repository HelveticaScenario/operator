//! Three-band crossover / band splitter module.
//!
//! Splits input into low, mid, and high frequency bands using
//! Linkwitz-Riley 4th-order crossover filters.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::filters::resonant_svf::{Response, cutoff_gain},
    dsp::filters::svf_core::{self, SvfCoeffs},
    dsp::utils::{changed, sanitize},
    poly::{PolyOutput, PolySignal, PolySignalExt},
    types::{GlideMark, SwapGlide},
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

// ── Filter sections ──────────────────────────────────────────────────────────

/// Butterworth (Q = 1/√2) lowpass and highpass at one crossover point.
fn compute_coeffs(voct: f32, sample_rate: f32) -> (SvfCoeffs, SvfCoeffs) {
    let g = cutoff_gain(voct, sample_rate);
    let k = 1.0 / BUTTERWORTH_Q;
    (
        Response::Lowpass.coeffs(g, k),
        Response::Highpass.coeffs(g, k),
    )
}

/// One Linkwitz-Riley 4th-order leg: two identical Butterworth sections in series.
#[derive(Clone, Copy, Default)]
struct Lr4State {
    ic: [[f32; 2]; 2],
}

impl Lr4State {
    #[inline]
    fn process(&mut self, input: f32, coeffs: &SvfCoeffs) -> f32 {
        let [[a1, a2], [b1, b2]] = &mut self.ic;
        let mid = svf_core::process(coeffs, a1, a2, input);
        svf_core::process(coeffs, b1, b2, mid)
    }
}

/// Glided crossover frequency and the coefficients derived from it.
/// `NaN` = never computed.
#[derive(Clone, Copy)]
struct CrossoverPoint {
    glide: SwapGlide,
    last: f32,
    lp: SvfCoeffs,
    hp: SvfCoeffs,
}

impl Default for CrossoverPoint {
    fn default() -> Self {
        Self {
            glide: SwapGlide::default(),
            last: f32::NAN,
            lp: SvfCoeffs::default(),
            hp: SvfCoeffs::default(),
        }
    }
}

impl CrossoverPoint {
    #[inline]
    fn refresh(&mut self, voct: f32, mark: GlideMark, sample_rate: f32) {
        self.glide.update(voct, mark, sample_rate);
        let v = *self.glide;
        if changed(v, self.last) {
            (self.lp, self.hp) = compute_coeffs(v, sample_rate);
            self.last = v;
        }
    }
}

// ── Per-channel state ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Default)]
struct ChannelState {
    /// LOW band: LR4 lowpass at the low/mid point.
    low_lp: Lr4State,
    /// MID band: LR4 highpass at the low/mid point, then LR4 lowpass at the
    /// mid/high point.
    mid_hp: Lr4State,
    mid_lp: Lr4State,
    /// HIGH band: LR4 highpass at the mid/high point.
    high_hp: Lr4State,
    low_mid: CrossoverPoint,
    mid_high: CrossoverPoint,
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
/// let bands = $xover($saw('c2'), { lowMidFreq: '200hz', midHighFreq: '2000hz' })
/// let low  = $comp(bands.low,  { threshold: 2.5, ratio: 4 })
/// let mid  = $comp(bands.mid,  { threshold: 3,   ratio: 3 })
/// let high = $comp(bands.high, { threshold: 2,   ratio: 6 })
/// $mix([low, mid, high]).out()
/// ```
#[module(name = "$xover", args(input))]
pub struct Crossover {
    outputs: CrossoverOutputs,
    channel_state: Box<[ChannelState]>,
    params: CrossoverParams,
}

impl Crossover {
    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();

        for ch in 0..channels {
            let state = &mut self.channel_state[ch];

            let input = self.params.input.get_value(ch);

            // ── Read crossover frequencies ───────────────────────────────
            let low_mid_voct = self
                .params
                .low_mid_freq
                .value_or(ch, DEFAULT_LOW_MID_FREQ_VOCT);
            let mid_high_voct = self
                .params
                .mid_high_freq
                .value_or(ch, DEFAULT_MID_HIGH_FREQ_VOCT);

            state.low_mid.refresh(
                low_mid_voct,
                self.params.low_mid_freq.glide_mark(ch),
                sample_rate,
            );
            state.mid_high.refresh(
                mid_high_voct,
                self.params.mid_high_freq.glide_mark(ch),
                sample_rate,
            );

            // ── Split into 3 bands using LR4 crossover ──────────────────
            let x = sanitize(input);
            // Low band: input → LPF₁ → LPF₂ (at low_mid_freq)
            let low_band = state.low_lp.process(x, &state.low_mid.lp);

            // Mid band: input → HPF₁ → HPF₂ (at low_mid_freq) → LPF₁ → LPF₂ (at mid_high_freq)
            let mid_hp_out = state.mid_hp.process(x, &state.low_mid.hp);
            let mid_band = state.mid_lp.process(mid_hp_out, &state.mid_high.lp);

            // High band: input → HPF₁ → HPF₂ (at mid_high_freq)
            let high_band = state.high_hp.process(x, &state.mid_high.hp);

            // ── Write outputs ───────────────────────────────────────────
            self.outputs.sample.set(ch, input);
            self.outputs.low.set(ch, low_band);
            self.outputs.mid.set(ch, mid_band);
            self.outputs.high.set(ch, high_band);
        }
    }
}

message_handlers!(impl Crossover {});
