//! `$unstable.filter.tripole` — Surge XT's Tri-pole (`fut_tripole`), an emulation of
//! Ian Fritz's Threeler: three one-pole OTA stages with per-stage saturation, a
//! resonance waveshaper, and global feedback resolved by Newton iteration. `mode`
//! picks the stage topology (LLL/LHL/HLH/HHH); `outputStage` taps stage 1, 2, or the
//! full feedback output.
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::coeffs::note_to_hz;
use super::fastmath::fastexp;
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

const RES_GAIN: f32 = 1.5;
const IN_GAIN: f32 = 4.0;
const OUT_GAIN: f32 = 1.0 / IN_GAIN;

const N_ITER_GLOBAL: usize = 3;
const N_ITER_STAGE: usize = 1;

// Per-OTA saturation asymmetries (Surge: "each OTA is a little different :)").
const OTA1BP: f32 = 0.88;
const OTA1BN: f32 = 1.0;
const OTA2BP: f32 = 0.9;
const OTA2BN: f32 = 0.97;
const OTA3BP: f32 = 0.95;
const OTA3BN: f32 = 1.025;

/// `log10(Iabc · Rload)` = `log10(8 mA · 220 kΩ)` — the resonance dB scale.
const RES_FACTOR_DB: f32 = 3.245_512_7;

/// Tri-pole coefficient slots (Surge `thr_coeffs`).
const THR_B0: usize = 0;
const THR_A0: usize = 1;
const THR_B1: usize = 2;
const THR_A1: usize = 3;
const THR_B2: usize = 4;
const THR_A2: usize = 5;
const THR_K: usize = 6;
const N_THR_COEFF: usize = 7;

/// Tri-pole state registers (Surge `thr_state`).
const THR_Z0: usize = 0;
const THR_X0: usize = 1;
const THR_Z1: usize = 2;
const THR_X1: usize = 3;
const THR_Z2: usize = 4;
const THR_X2: usize = 5;
const THR_FB: usize = 6;
const THR_FB1: usize = 7;

/// Surge `thr_sigmoid`: the inverse-square-root sigmoid `x/√(x²+β)`.
#[inline]
fn thr_sigmoid(x: f32, beta: f32) -> f32 {
    x / (x * x + beta).sqrt()
}

/// `sech²` computed from an already-evaluated tanh-like value.
#[inline]
fn sech2_with_tanh(t: f32) -> f32 {
    1.0 - t * t
}

/// Surge `OnePoleLPF::process` (Newton-refined nonlinear one-pole lowpass).
#[inline]
fn lpf_process(tanh_x: f32, z: f32, b: f32, a: f32, beta: f32) -> f32 {
    let mut estimate = a * (b * tanh_x + z);
    for _ in 0..N_ITER_STAGE {
        let tanh_y = thr_sigmoid(estimate, beta);
        let residue = (b * (tanh_x - tanh_y) + z) - estimate;
        estimate -= residue / (-b * sech2_with_tanh(tanh_y) - 1.0);
    }
    estimate
}

/// Surge `OnePoleHPF::process`.
#[inline]
fn hpf_process(x: f32, x1: f32, z: f32, b: f32, a: f32, beta: f32) -> f32 {
    let xxz = x - x1 + z;
    let mut estimate = a * xxz;
    for _ in 0..N_ITER_STAGE {
        let tanh_y = thr_sigmoid(estimate, beta);
        let residue = (-b * tanh_y + xxz) - estimate;
        estimate -= residue / (-b * sech2_with_tanh(tanh_y) - 1.0);
    }
    estimate
}

/// Surge `OnePoleLPF_FB::process` — stage 1 lowpass with the global feedback mixed
/// into its state.
#[inline]
fn lpf_fb_process(tanh_x: f32, z: f32, fb: f32, fb1: f32, b: f32, a: f32, bx: f32) -> f32 {
    let zff = z - fb + fb1;
    let mut estimate = a * (bx + zff);
    for _ in 0..N_ITER_STAGE {
        let tanh_y = thr_sigmoid(estimate, OTA1BN);
        let residue = (b * (tanh_x - tanh_y) + zff) - estimate;
        estimate -= residue / (-b * sech2_with_tanh(tanh_y) - 1.0);
    }
    estimate
}

/// Surge `OnePoleHPF_FB::process` — stage 1 highpass driven by the saturated
/// feedback estimate.
#[inline]
fn hpf_fb_process(xxz: f32, tanh_fb: f32, b: f32, a: f32) -> f32 {
    let mut estimate = a * (b * tanh_fb + xxz);
    for _ in 0..N_ITER_STAGE {
        let tanh_y = thr_sigmoid(estimate, OTA1BN);
        let residue = (b * (tanh_fb - tanh_y) + xxz) - estimate;
        estimate -= residue / (-b * sech2_with_tanh(tanh_y) - 1.0);
    }
    estimate
}

// Surge `ResWaveshaper` constants: `beta_exp = 9.03240196 · ln(1.0168177)`.
const RS_BETA_EXP: f32 = 0.150_641_03;
const RS_C: f32 = 0.222_161;
const RS_BIAS: f32 = 8.2;
const RS_MAX_VAL: f32 = 7.5;
const RS_MULT: f32 = 10.0;
const RS_ONE: f32 = 0.99;

/// Surge `ResWaveshaper::res_func_ps`: linear below the knee, exponential approach
/// to the supply rail above it.
#[inline]
fn res_func(x: f32) -> f32 {
    let x = RS_MULT * x;
    if x.abs() < RS_MAX_VAL {
        x * (RS_ONE / RS_MULT)
    } else {
        let y = -fastexp(RS_BETA_EXP * -(x + RS_C).abs()) + RS_BIAS;
        x.signum() * y * (RS_ONE / RS_MULT)
    }
}

/// Surge `ResWaveshaper::res_deriv_ps`.
#[inline]
fn res_deriv(x: f32) -> f32 {
    let x = RS_MULT * x;
    if x.abs() < RS_MAX_VAL {
        RS_ONE
    } else {
        fastexp(RS_BETA_EXP * -(x + RS_C).abs()) + RS_BETA_EXP / RS_MULT
    }
}

/// Stage topology (which of the three one-poles are lowpass vs highpass).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum TriPoleShape {
    /// Lowpass → lowpass → lowpass.
    #[default]
    Lll,
    /// Lowpass → highpass → lowpass.
    Lhl,
    /// Highpass → lowpass → highpass.
    Hlh,
    /// Highpass → highpass → highpass.
    Hhh,
}

/// Topology + Surge subtype (`(mode) | outStage<<2`, 0–11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriPoleMode {
    pub subtype: i32,
}

/// Tri-pole kernel. Saturates throughout → 2× oversampled.
#[derive(Clone, Copy, Default)]
pub struct TriPole;

impl Filter for TriPole {
    type Mode = TriPoleMode;
    type Extra = ();

    /// Surge `TriPoleFilter::makeCoefficients`: per-stage one-pole coefficients
    /// (slightly perturbed per stage) and the exponential resonance coefficient `k`.
    fn coeffs(_mode: TriPoleMode, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        let freq = note_to_hz(freq_semi).clamp(5.0, rate * 0.3);
        let wc = 2.0 * std::f32::consts::PI * freq / rate;
        // `T·g/capVal` reduces to `exp(wc) − 1`.
        let tg = wc.exp() - 1.0;

        let mut c = [0.0f32; N_COEFFS];
        c[THR_B0] = 0.998 * tg;
        c[THR_A0] = 1.0 / (1.0 + c[THR_B0]);
        c[THR_B1] = 1.0012 * tg;
        c[THR_A1] = 1.0 / (1.0 + c[THR_B1]);
        c[THR_B2] = tg;
        c[THR_A2] = 1.0 / (1.0 + c[THR_B2]);
        c[THR_K] = -(10.0f32.powf(RES_FACTOR_DB * reso.clamp(0.0, 1.0)) + 1.0);
        c
    }

    fn process(
        mode: TriPoleMode,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        let input = IN_GAIN * x;
        let topo = mode.subtype & 3;
        let out_stage = (mode.subtype >> 2) & 3;

        let z0 = r[THR_Z0];
        let x0 = r[THR_X0];
        let mut estimate0 = z0;
        let b0 = c[THR_B0];
        let a0 = c[THR_A0];

        let z1 = r[THR_Z1];
        let x1 = r[THR_X1];
        let mut estimate1 = z1;
        let b1 = c[THR_B1];
        let a1 = c[THR_A1];

        let z2 = r[THR_Z2];
        let x2 = r[THR_X2];
        let mut res_out = x2;
        let mut estimate2 = z2;
        let b2 = c[THR_B2];
        let a2 = c[THR_A2];

        let k = c[THR_K];

        // Stage-1 input terms, fixed per sample by the topology.
        let (tanh_x0, bx, hpf_in) = if topo < 2 {
            let t = thr_sigmoid(input, OTA1BP);
            (t, b0 * t, 0.0)
        } else {
            (0.0, 0.0, input - x0 + z0)
        };

        let mut estimate = r[THR_FB];

        for _ in 0..N_ITER_GLOBAL {
            // Stage 1 (carries the global feedback).
            let f0_deriv = if topo < 2 {
                estimate0 = lpf_fb_process(tanh_x0, z0, estimate, r[THR_FB1], b0, a0, bx);
                2.0
            } else {
                let tanh_fb = thr_sigmoid(estimate, OTA1BP);
                estimate0 = hpf_fb_process(hpf_in, tanh_fb, b0, a0);
                b0 * sech2_with_tanh(tanh_fb)
            };

            // Stage 2.
            let f1_deriv = if topo == 0 || topo == 2 {
                let tanh_x1 = thr_sigmoid(estimate0, OTA2BP);
                estimate1 = lpf_process(tanh_x1, z1, b1, a1, OTA2BN);
                b1 * sech2_with_tanh(tanh_x1)
            } else {
                estimate1 = hpf_process(estimate0, x1, z1, b1, a1, OTA2BN);
                2.0
            };

            // Resonance waveshaper.
            let k_times_f1 = k * estimate1;
            res_out = (1.0 / RES_GAIN) * res_func(RES_GAIN * k_times_f1);
            let rd = res_deriv(k_times_f1);

            // Stage 3.
            let f2_deriv = if topo < 2 {
                let tanh_x2 = thr_sigmoid(res_out, OTA3BP);
                estimate2 = lpf_process(tanh_x2, z2, b2, a2, OTA3BN);
                b2 * sech2_with_tanh(tanh_x2)
            } else {
                estimate2 = hpf_process(res_out, x2, z2, b2, a2, OTA3BN);
                2.0
            };

            // Newton step on the global feedback.
            let num = estimate - estimate2;
            let den = 1.0 - k * rd * f0_deriv * f1_deriv * f2_deriv;
            estimate -= num / den;
        }

        r[THR_Z0] = estimate0;
        r[THR_X0] = input;
        r[THR_Z1] = estimate1;
        r[THR_X1] = estimate0;
        r[THR_Z2] = estimate2;
        r[THR_X2] = res_out;
        r[THR_FB1] = r[THR_FB];
        r[THR_FB] = estimate;

        for i in 0..N_THR_COEFF {
            c[i] += dc[i];
        }

        OUT_GAIN
            * match out_stage {
                0 => estimate0,
                1 => estimate1,
                _ => estimate,
            }
    }

    fn oversample(_mode: TriPoleMode) -> bool {
        true
    }
}

filter_module! {
    /// Surge XT's Tri-pole — Ian Fritz's Threeler filter: three saturating one-pole
    /// stages, a resonance waveshaper, and global feedback. Wild and organic;
    /// self-oscillates readily. Runs 2× oversampled.
    ///
    /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; the feedback coefficient grows exponentially.
    /// - **mode** — stage topology: `'lll'`, `'lhl'`, `'hlh'`, or `'hhh'`.
    /// - **outputStage** — 1, 2, or 3 (3 = the full feedback output).
    ///
    /// ```js
    /// $unstable.filter.tripole($saw('c2'), 'c4', 2, { mode: 'lhl', outputStage: 3 })
    /// ```
    name = "$unstable.filter.tripole", ident = TriPoleFilter, kernel = TriPole,
    output_doc = "filter output",
    params = {
        /// stage topology: lll, lhl, hlh, or hhh (default lll)
        mode: TriPoleShape,
        /// which stage to tap: 1, 2, or 3 (default 3, the full output)
        output_stage: Option<usize>,
    },
    mode = |p| TriPoleMode {
        subtype: (p.mode as i32)
            | (((p.output_stage.unwrap_or(3).clamp(1, 3) as i32) - 1) << 2),
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::sweep_stays_bounded;
    use crate::dsp::filters::surge::filter_core::{N_COEFFS, N_REGISTERS};

    fn all_modes() -> Vec<TriPoleMode> {
        (0..12).map(|subtype| TriPoleMode { subtype }).collect()
    }

    /// The Threeler self-oscillates even at zero resonance — an impulse leaves a
    /// bounded limit cycle. The reference `sst-filters` build settles at peak
    /// ≈0.1299 for LLL/full-output at a 1 kHz cutoff; the scalar port lands within
    /// f32/sigmoid-approximation noise of that.
    #[test]
    fn impulse_settles_to_the_surge_limit_cycle() {
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        let mode = TriPoleMode { subtype: 8 }; // LLL, full output
        let mut c = TriPole::coeffs(mode, cutoff_semi, 0.0, 96_000.0);
        let dc = [0.0f32; N_COEFFS];
        let mut r = [0.0f32; N_REGISTERS];
        let mut peak = 0.0f32;
        for i in 0..16_000 {
            let x = if i == 0 { 0.05 } else { 0.0 };
            let y = TriPole::process(mode, x, &mut c, &dc, &mut r, &mut ());
            assert!(y.is_finite() && y.abs() < 2.0, "unbounded output {y}");
            if i > 12_000 {
                peak = peak.max(y.abs());
            }
        }
        assert!(
            (0.10..=0.16).contains(&peak),
            "limit cycle off the golden value: peak={peak} (reference 0.1299)"
        );
    }

    #[test]
    fn all_modes_survive_resonant_cutoff_sweep() {
        sweep_stays_bounded::<TriPole>(&all_modes());
    }
}
