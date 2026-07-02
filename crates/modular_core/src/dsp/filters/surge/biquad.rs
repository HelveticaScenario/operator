//! Vember biquad kernels — the "12 dB"/"24 dB" state-variable / lattice / coupled
//! forms shared by `$unstable.filter.{lp,hp,bp,notch,ap}`.
//!
//! Standard = state-variable (`svf_*`), Clean = normalized lattice (`iir*_b`),
//! Driven = coupled state-space (`iir*_cfc`). All three are linear apart from a mild
//! `R = max(0.1, 1 − clipgain·y²)` resonance-damping register — no hard per-sample
//! saturator — so they run at the base rate (no oversampling).
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::coeffs::{ST_CLEAN, ST_DRIVEN, ST_STANDARD};
use super::filter_core::{N_COEFFS, N_REGISTERS};

/// Filter slope (pole count). Rendered as `'db12'` / `'db24'` in the DSL.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum FilterSlope {
    /// 12 dB/oct (2-pole).
    Db12,
    /// 24 dB/oct (4-pole).
    #[default]
    Db24,
}

impl FilterSlope {
    #[inline]
    pub fn four_pole(self) -> bool {
        matches!(self, FilterSlope::Db24)
    }
}

/// Drive character for the LP/HP biquads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum BiquadDrive {
    /// State-variable form (cleanest, mildest resonance).
    #[default]
    Standard,
    /// Coupled state-space form with a heavier resonance/clip character.
    Driven,
    /// Normalized-lattice form (high numerical precision).
    Clean,
}

impl BiquadDrive {
    #[inline]
    pub fn subtype(self) -> i32 {
        match self {
            BiquadDrive::Standard => ST_STANDARD,
            BiquadDrive::Driven => ST_DRIVEN,
            BiquadDrive::Clean => ST_CLEAN,
        }
    }
}

/// Slope + Surge subtype pair selecting one concrete biquad configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadMode {
    pub four_pole: bool,
    pub subtype: i32,
}

// ─── Process kernels (scalar transliterations; `c += dc` is kept inline, since the
// lattice forms read coefficients both before and after their per-sample advance) ──

#[inline]
pub fn svf_lp12(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    c[0] += dc[0];
    c[1] += dc[1];
    let l = r[1] + c[0] * r[0];
    let h = x - l - c[1] * r[0];
    let b = r[0] + c[0] * h;
    let l2 = l + c[0] * b;
    let h2 = x - l2 - c[1] * b;
    let b2 = b + c[0] * h2;
    r[0] = b2 * r[2];
    r[1] = l2 * r[2];
    c[2] += dc[2];
    r[2] = (1.0 - c[2] * b * b).max(0.1);
    c[3] += dc[3];
    l2 * c[3]
}

/// `svf_lp12` with the highpass tap: same section, returns `H` instead of `L`.
#[inline]
pub fn svf_hp12(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    c[0] += dc[0];
    c[1] += dc[1];
    let l = r[1] + c[0] * r[0];
    let h = x - l - c[1] * r[0];
    let b = r[0] + c[0] * h;
    let l2 = l + c[0] * b;
    let h2 = x - l2 - c[1] * b;
    let b2 = b + c[0] * h2;
    r[0] = b2 * r[2];
    r[1] = l2 * r[2];
    c[2] += dc[2];
    r[2] = (1.0 - c[2] * b * b).max(0.1);
    c[3] += dc[3];
    h2 * c[3]
}

/// `svf_lp12` with the bandpass tap: same section, returns `B`.
#[inline]
pub fn svf_bp12(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    c[0] += dc[0];
    c[1] += dc[1];
    let l = r[1] + c[0] * r[0];
    let h = x - l - c[1] * r[0];
    let b = r[0] + c[0] * h;
    let l2 = l + c[0] * b;
    let h2 = x - l2 - c[1] * b;
    let b2 = b + c[0] * h2;
    r[0] = b2 * r[2];
    r[1] = l2 * r[2];
    c[2] += dc[2];
    r[2] = (1.0 - c[2] * b * b).max(0.1);
    c[3] += dc[3];
    b2 * c[3]
}

#[inline]
pub fn svf_lp24(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    c[0] += dc[0];
    c[1] += dc[1];

    let mut l = r[1] + c[0] * r[0];
    let mut h = x - l - c[1] * r[0];
    let mut b = r[0] + c[0] * h;
    l += c[0] * b;
    h = x - l - c[1] * b;
    b += c[0] * h;
    r[0] = b * r[2];
    r[1] = l * r[2];

    let stage2_in = l;
    l = r[4] + c[0] * r[3];
    h = stage2_in - l - c[1] * r[3];
    b = r[3] + c[0] * h;
    l += c[0] * b;
    h = stage2_in - l - c[1] * b;
    b += c[0] * h;
    r[3] = b * r[2];
    r[4] = l * r[2];

    c[2] += dc[2];
    r[2] = (1.0 - c[2] * b * b).max(0.1);
    c[3] += dc[3];
    l * c[3]
}

/// `svf_lp24` with the highpass taps: stage 2 is fed `H`, and `H` is the output.
#[inline]
pub fn svf_hp24(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    c[0] += dc[0];
    c[1] += dc[1];

    let mut l = r[1] + c[0] * r[0];
    let mut h = x - l - c[1] * r[0];
    let mut b = r[0] + c[0] * h;
    l += c[0] * b;
    h = x - l - c[1] * b;
    b += c[0] * h;
    r[0] = b * r[2];
    r[1] = l * r[2];

    let stage2_in = h;
    l = r[4] + c[0] * r[3];
    h = stage2_in - l - c[1] * r[3];
    b = r[3] + c[0] * h;
    l += c[0] * b;
    h = stage2_in - l - c[1] * b;
    b += c[0] * h;
    r[3] = b * r[2];
    r[4] = l * r[2];

    c[2] += dc[2];
    r[2] = (1.0 - c[2] * b * b).max(0.1);
    c[3] += dc[3];
    h * c[3]
}

/// `svf_lp24` with the bandpass taps: stage 2 is fed `B`, and `B` is the output.
#[inline]
pub fn svf_bp24(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    c[0] += dc[0];
    c[1] += dc[1];

    let mut l = r[1] + c[0] * r[0];
    let mut h = x - l - c[1] * r[0];
    let mut b = r[0] + c[0] * h;
    l += c[0] * b;
    h = x - l - c[1] * b;
    b += c[0] * h;
    r[0] = b * r[2];
    r[1] = l * r[2];

    let stage2_in = b;
    l = r[4] + c[0] * r[3];
    h = stage2_in - l - c[1] * r[3];
    b = r[3] + c[0] * h;
    l += c[0] * b;
    h = stage2_in - l - c[1] * b;
    b += c[0] * h;
    r[3] = b * r[2];
    r[4] = l * r[2];

    c[2] += dc[2];
    r[2] = (1.0 - c[2] * b * b).max(0.1);
    c[3] += dc[3];
    b * c[3]
}

#[inline]
pub fn iir12_b(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    let f2 = c[3] * x - c[1] * r[1]; // pre-advance c[1], c[3]
    c[1] += dc[1];
    c[3] += dc[3];
    let g2 = c[1] * x + c[3] * r[1]; // post-advance

    let f1 = c[2] * f2 - c[0] * r[0]; // pre-advance c[0], c[2]
    c[0] += dc[0];
    c[2] += dc[2];
    let g1 = c[0] * f2 + c[2] * r[0]; // post-advance

    c[4] += dc[4];
    c[5] += dc[5];
    c[6] += dc[6];
    let y = c[6] * g2 + c[5] * g1 + c[4] * f1;

    r[0] = f1 * r[2];
    r[1] = g1 * r[2];

    c[7] += dc[7];
    r[2] = (1.0 - c[7] * y * y).max(0.1);
    y
}

#[inline]
pub fn iir24_b(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    c[1] += dc[1];
    c[3] += dc[3];
    c[0] += dc[0];
    c[2] += dc[2];
    c[4] += dc[4];
    c[5] += dc[5];
    c[6] += dc[6];

    let f2 = c[3] * x - c[1] * r[1];
    let g2 = c[1] * x + c[3] * r[1];
    let f1 = c[2] * f2 - c[0] * r[0];
    let g1 = c[0] * f2 + c[2] * r[0];
    r[0] = f1 * r[4];
    r[1] = g1 * r[4];
    let y1 = c[6] * g2 + c[5] * g1 + c[4] * f1;

    let f2b = c[3] * y1 - c[1] * r[3];
    let g2b = c[1] * y1 + c[3] * r[3];
    let f1b = c[2] * f2b - c[0] * r[2];
    let g1b = c[0] * f2b + c[2] * r[2];
    r[2] = f1b * r[4];
    r[3] = g1b * r[4];
    let y2 = c[6] * g2b + c[5] * g1b + c[4] * f1b;

    c[7] += dc[7];
    r[4] = (1.0 - c[7] * y2 * y2).max(0.1);
    y2
}

#[inline]
pub fn iir12_cfc(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    c[0] += dc[0];
    c[1] += dc[1];
    c[2] += dc[2];
    c[4] += dc[4];
    c[5] += dc[5];
    c[6] += dc[6];

    let y = c[4] * r[0] + c[6] * x + c[5] * r[1];
    let s1 = x * c[2] + (c[0] * r[0] - c[1] * r[1]);
    let s2 = c[1] * r[0] + c[0] * r[1];
    r[0] = s1 * r[2];
    r[1] = s2 * r[2];

    c[7] += dc[7];
    r[2] = (1.0 - c[7] * y * y).max(0.1);
    y
}

#[inline]
pub fn iir24_cfc(
    x: f32,
    c: &mut [f32; N_COEFFS],
    dc: &[f32; N_COEFFS],
    r: &mut [f32; N_REGISTERS],
) -> f32 {
    c[0] += dc[0];
    c[1] += dc[1];
    c[2] += dc[2];
    c[4] += dc[4];
    c[5] += dc[5];
    c[6] += dc[6];

    let y = c[4] * r[0] + c[6] * x + c[5] * r[1];
    let s1 = x * c[2] + (c[0] * r[0] - c[1] * r[1]);
    let s2 = c[1] * r[0] + c[0] * r[1];
    r[0] = s1 * r[2];
    r[1] = s2 * r[2];

    let y2 = c[4] * r[3] + c[6] * y + c[5] * r[4];
    let s3 = y * c[2] + (c[0] * r[3] - c[1] * r[4]);
    let s4 = c[1] * r[3] + c[0] * r[4];
    r[3] = s3 * r[2];
    r[4] = s4 * r[2];

    c[7] += dc[7];
    r[2] = (1.0 - c[7] * y2 * y2).max(0.1);
    y2
}
