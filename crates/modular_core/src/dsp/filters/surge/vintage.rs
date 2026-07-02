//! `$unstable.filter.vintageLadder` — Surge XT's Vintage Ladder (`fut_vintageladder`):
//! three Moog-ladder simulations selected by `type` — `rk` (Runge-Kutta integration of
//! the ladder's differential equations, after Miller Puckette), `huov` (Huovilainen's
//! per-stage-tanh model, via Victor Lazzarini's CSound implementation), and `huov2010`
//! (Huovilainen's 2010 revision) — each with an optional bass-compensated variant.
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::coeffs::note_to_hz;
use super::fastmath::{fastexp, fasttanh_clamped};
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

/// Ladder simulation model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum VintageType {
    /// Runge-Kutta integration of the ladder circuit's differential equations.
    #[default]
    Rk,
    /// Huovilainen model — a tanh saturator inside each of the four pole stages.
    Huov,
    /// Huovilainen's 2010 revision — one input saturator driving four linear poles.
    Huov2010,
}

/// Model + bass-compensation pair selecting one concrete configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VintageMode {
    pub model: VintageType,
    pub compensated: bool,
}

/// Cutoff clamp shared by all three models (Surge `VintageLadder::Common`).
#[inline]
fn clamped_frequency(freq_semi: f32, rate: f32) -> f32 {
    note_to_hz(freq_semi).clamp(5.0, rate * 0.3)
}

// ─── RK model ──────────────────────────────────────────────────────────────────

/// RK feedback bound: resonance maps to 0..4.5 (past ~4 the ladder self-oscillates).
const RK_RESO_SCALE: f32 = 4.5;
const RK_GAIN_COMPENSATION: f32 = 0.666;
/// The RK integrator sub-steps 4× per engine sample.
const RK_OVERSAMPLE: usize = 4;

/// Surge `RK::clip`: the stage saturator, `3·(u − u³/3)` with `u = clamp(v/3, ±1)`.
#[inline]
fn rk_clip(v: f32) -> f32 {
    let u = (v * (1.0 / 3.0)).clamp(-1.0, 1.0);
    3.0 * (u - (1.0 / 3.0) * u * u * u)
}

/// Surge `RK::calculateDerivatives`. `cutoff` arrives pre-scaled by the integrator
/// step size, so the derivative is directly the per-sub-step state increment.
#[inline]
fn rk_derivatives(input: f32, state: &[f32], cutoff: f32, resonance: f32, g_comp: f32) -> [f32; 4] {
    let sat0 = rk_clip(state[0]);
    let sat1 = rk_clip(state[1]);
    let sat2 = rk_clip(state[2]);
    let start = rk_clip(input - resonance * (state[3] - g_comp * input));
    [
        cutoff * (start - sat0),
        cutoff * (sat0 - sat1),
        cutoff * (sat1 - sat2),
        cutoff * (sat2 - rk_clip(state[3])),
    ]
}

/// Surge `RK::makeCoefficients`. The integrator step size `1/(4·rate)` is baked into
/// the cutoff slot (the kernel has no rate argument, and `cutoff` only ever appears
/// multiplied by it); the glide interpolates the product just as Surge glides `ω`.
fn coeffs_rk(freq_semi: f32, reso: f32, rate: f32, compensated: bool) -> [f32; N_COEFFS] {
    let freq = clamped_frequency(freq_semi, rate);
    let mut c = [0.0f32; N_COEFFS];
    c[0] = freq * 2.0 * std::f32::consts::PI / (rate * RK_OVERSAMPLE as f32);
    c[1] = reso.clamp(0.0, 1.0) * RK_RESO_SCALE;
    c[2] = if compensated {
        RK_GAIN_COMPENSATION
    } else {
        0.0
    };
    c
}

/// Surge `RK::process`: RK4 integration at 4× with zero-stuffed input, reconstructed
/// through Surge XT's backwards Lanczos window.
fn process_rk(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    /// Lanczos factors `2·sin(πx)·sin(πx/2)/(π²x²)` at x = −1.5, −1, 0.5, 0.
    const WINDOW: [f32; RK_OVERSAMPLE] = [-0.063_684_4, 0.0, 0.573_159_17, 1.0];
    const SUB: f32 = 1.0 / RK_OVERSAMPLE as f32;

    let mut out = 0.0f32;
    let mut input = x;
    for w in WINDOW {
        for j in 0..3 {
            c[j] += SUB * dc[j];
        }
        let cutoff = c[0];
        let resonance = c[1];
        let g_comp = c[2];

        let state: [f32; 4] = [r[0], r[1], r[2], r[3]];
        let d1 = rk_derivatives(input, &state, cutoff, resonance, g_comp);
        let t: [f32; 4] = std::array::from_fn(|i| state[i] + 0.5 * d1[i]);
        let d2 = rk_derivatives(input, &t, cutoff, resonance, g_comp);
        let t: [f32; 4] = std::array::from_fn(|i| state[i] + 0.5 * d2[i]);
        let d3 = rk_derivatives(input, &t, cutoff, resonance, g_comp);
        let t: [f32; 4] = std::array::from_fn(|i| state[i] + 0.5 * d3[i]);
        let d4 = rk_derivatives(input, &t, cutoff, resonance, g_comp);
        for i in 0..4 {
            r[i] += (1.0 / 6.0) * (d1[i] + 2.0 * d2[i] + 2.0 * d3[i] + d4[i]);
        }

        out += r[3] * w;
        input = 0.0;
    }
    1.5 * out
}

// ─── Huovilainen model ─────────────────────────────────────────────────────────

/// Huov register layout (Surge `huov_regoffsets`): four stage outputs, three cached
/// stage tanh values, and a six-slot delay chain (the last is the ½-sample-delayed
/// output used for phase compensation and feedback).
const H_STAGE: usize = 0;
const H_STAGE_TANH: usize = 4;
const H_DELAY: usize = 7;

const HUOV_GAIN_COMPENSATION: f32 = 0.5;
/// The transistor thermal voltage scale (Surge XT's experimentally tuned 1/70).
const THERMAL: f32 = 1.0 / 70.0;

/// Surge `Huov::makeCoefficients`. Resonance is pulled down as the cutoff approaches
/// the rate (Surge XT's ear-tuned stability trim), further trimmed when compensated.
fn coeffs_huov(freq_semi: f32, reso: f32, rate: f32, compensated: bool) -> [f32; N_COEFFS] {
    let cutoff = clamped_frequency(freq_semi, rate);
    let co = (cutoff - rate * 0.33333).max(0.0) * 0.1 / rate;
    let gctrim = if compensated { 0.05 } else { 0.0 };
    let reso = reso.clamp(0.0, 0.9925).clamp(0.0, 0.994 - co - gctrim);

    let mut c = [0.0f32; N_COEFFS];
    c[0] = cutoff;
    c[1] = reso;
    c[2] = cutoff / rate;
    c[3] = if compensated {
        HUOV_GAIN_COMPENSATION
    } else {
        0.0
    };
    c
}

/// Surge `Huov::process`: four tanh-saturated pole stages with a ½-sample output
/// delay, run 2× internally per engine sample.
fn process_huov(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    let mut out = 0.0f32;
    for _ in 0..2 {
        let fc = c[2];
        let res = c[1];
        let fr = fc * 0.5;
        let fc2 = fc * fc;
        let fc3 = fc * fc2;

        let fcr = 1.8730 * fc3 + 0.4955 * fc2 - 0.6490 * fc + 0.9988;
        let acr = -3.9364 * fc2 + 1.8409 * fc + 0.9968;
        let tune = (1.0 - fastexp(-2.0 * std::f32::consts::PI * fr * fcr)) / THERMAL;
        let resquad = 4.0 * res * acr;

        for k in 0..4 {
            c[k] += 0.5 * dc[k];
        }

        let input = x - resquad * (r[H_DELAY + 5] - c[3] * x);
        r[H_STAGE] = r[H_DELAY] + tune * (fasttanh_clamped(input * THERMAL) - r[H_STAGE_TANH]);
        r[H_DELAY] = r[H_STAGE];

        for k in 1..4 {
            let stage_in = r[H_STAGE + k - 1];
            r[H_STAGE_TANH + k - 1] = fasttanh_clamped(stage_in * THERMAL);
            let upper = if k != 3 {
                r[H_STAGE_TANH + k]
            } else {
                fasttanh_clamped(r[H_DELAY + k] * THERMAL)
            };
            r[H_STAGE + k] = r[H_DELAY + k] + tune * (r[H_STAGE_TANH + k - 1] - upper);
            r[H_DELAY + k] = r[H_STAGE + k];
        }

        // ½-sample delay for phase compensation.
        r[H_DELAY + 5] = 0.5 * (r[H_STAGE + 3] + r[H_DELAY + 4]);
        r[H_DELAY + 4] = r[H_STAGE + 3];

        out = r[H_DELAY + 5];
    }
    out
}

// ─── Huovilainen 2010 model ────────────────────────────────────────────────────

/// Huov2010 slots (Surge `Huov2010::coeff`/`reg`): coefficient slot 0 is unused.
const H10_GRES: usize = 1;
const H10_GONEPOLE: usize = 2;
const H10_GCOMP: usize = 3;
const H10_ONEPOLE_IN: usize = 0;
const H10_ONEPOLE_OUT: usize = 4;
const H10_DELAY_LINE: usize = 9;

const H10_DRIVE: f32 = 4.0;

/// Surge `Huov2010::makeCoefficients`: polynomial fits (in `ω`) for the feedback and
/// one-pole gains.
fn coeffs_huov2010(freq_semi: f32, reso: f32, rate: f32, compensated: bool) -> [f32; N_COEFFS] {
    let cutoff = clamped_frequency(freq_semi, rate);
    let omega = (2.0 * std::f64::consts::PI * cutoff as f64) / rate as f64;
    let ureso = reso.clamp(0.0, 1.0) as f64;

    let mut c = [0.0f32; N_COEFFS];
    c[H10_GRES] =
        (4.5 * ureso * (1.0029 + omega * (0.0526 + omega * (-0.0926 + 0.0218 * omega)))) as f32;
    c[H10_GONEPOLE] =
        (omega * (0.9892 + omega * (-0.4342 + omega * (0.1381 - 0.0202 * omega)))) as f32;
    c[H10_GCOMP] = if compensated { -0.5 } else { 0.0 };
    c
}

/// Surge `Huov2010::nonlin`: the input saturator at the transistor thermal scale.
#[inline]
fn h10_nonlin(v: f32) -> f32 {
    fasttanh_clamped(v * THERMAL) / THERMAL
}

/// Surge `Huov2010::onePole`: a ZDF one-pole with a fixed 0.3 input feedforward.
#[inline]
fn h10_one_pole(idx: usize, gonepole: f32, input: f32, r: &mut [f32; N_REGISTERS]) -> f32 {
    const ZDF: f32 = 0.3 / 1.3;
    const IDF: f32 = 1.0 / 1.3;
    let zd = r[H10_ONEPOLE_IN + idx];
    r[H10_ONEPOLE_IN + idx] = input;
    let n1 = zd * ZDF + input * IDF;
    let od = r[H10_ONEPOLE_OUT + idx];
    let n3 = gonepole * (n1 - od) + od;
    r[H10_ONEPOLE_OUT + idx] = n3;
    n3
}

/// Surge `Huov2010::process`: one input saturator driving four linear ZDF poles.
fn process_huov2010(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    let zm1 = r[H10_DELAY_LINE];
    let gaincomp = zm1 + c[H10_GCOMP] * (H10_DRIVE * x);
    let fb = c[H10_GRES] * gaincomp;
    let n1 = h10_nonlin(H10_DRIVE * x - fb);
    let gonepole = c[H10_GONEPOLE];
    let s1 = h10_one_pole(0, gonepole, n1, r);
    let s2 = h10_one_pole(1, gonepole, s1, r);
    let s3 = h10_one_pole(2, gonepole, s2, r);
    let s4 = h10_one_pole(3, gonepole, s3, r);
    r[H10_DELAY_LINE] = s4;

    for k in 0..4 {
        c[k] += dc[k];
    }
    (0.5 / H10_DRIVE) * s4
}

/// Vintage-ladder kernel dispatching on the model. Every model has a per-sample
/// saturator, so all run 2× oversampled.
#[derive(Clone, Copy, Default)]
pub struct VintageLadder;

impl Filter for VintageLadder {
    type Mode = VintageMode;
    type Extra = ();

    fn coeffs(mode: VintageMode, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        match mode.model {
            VintageType::Rk => coeffs_rk(freq_semi, reso, rate, mode.compensated),
            VintageType::Huov => coeffs_huov(freq_semi, reso, rate, mode.compensated),
            VintageType::Huov2010 => coeffs_huov2010(freq_semi, reso, rate, mode.compensated),
        }
    }

    fn process(
        mode: VintageMode,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        match mode.model {
            VintageType::Rk => process_rk(x, c, dc, r),
            VintageType::Huov => process_huov(x, c, dc, r),
            VintageType::Huov2010 => process_huov2010(x, c, dc, r),
        }
    }

    fn oversample(_mode: VintageMode) -> bool {
        true
    }
}

filter_module! {
    /// Surge XT's Vintage Ladder — three Moog-ladder simulations. `type` picks the
    /// model: `'rk'` (Runge-Kutta circuit integration), `'huov'` (Huovilainen,
    /// per-stage saturation), or `'huov2010'` (Huovilainen's 2010 revision).
    /// Runs 2× oversampled.
    ///
    /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; the top of the range self-oscillates.
    /// - **compensated** — bass compensation: keeps low end at high resonance.
    ///
    /// ```js
    /// $unstable.filter.vintageLadder($saw('c2'), 'c4', 2, { type: 'huov', compensated: true })
    /// ```
    name = "$unstable.filter.vintageLadder", ident = VintageLadderFilter, kernel = VintageLadder,
    output_doc = "lowpass output",
    params = {
        /// ladder model: rk, huov, or huov2010 (default rk)
        #[serde(rename = "type")]
        #[deserr(rename = "type")]
        r#type: VintageType,
        /// bass compensation (default false)
        compensated: bool,
    },
    mode = |p| VintageMode {
        model: p.r#type,
        compensated: p.compensated,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    fn all_modes() -> Vec<VintageMode> {
        let mut v = Vec::new();
        for model in [VintageType::Rk, VintageType::Huov, VintageType::Huov2010] {
            for compensated in [false, true] {
                v.push(VintageMode { model, compensated });
            }
        }
        v
    }

    #[test]
    fn vintage_ladder_attenuates_highs_all_models() {
        // The kernel run rate is 2× the 48 kHz engine rate.
        let sr = 96_000.0;
        // 1 kHz cutoff expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        for mode in all_modes() {
            let low = sine_rms::<VintageLadder>(mode, cutoff_semi, 0.0, 200.0, sr);
            let high = sine_rms::<VintageLadder>(mode, cutoff_semi, 0.0, 8000.0, sr);
            assert!(
                high < low * 0.5,
                "expected lowpass attenuation (mode {mode:?}): low={low} high={high}"
            );
        }
    }

    #[test]
    fn all_models_survive_resonant_cutoff_sweep() {
        sweep_stays_bounded::<VintageLadder>(&all_modes());
    }
}
