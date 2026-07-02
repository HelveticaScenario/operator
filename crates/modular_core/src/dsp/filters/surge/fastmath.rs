//! Scalar ports of Surge XT's per-sample saturators (sst-basic-blocks `Clippers.h` /
//! `FastMath.h`). These polynomials define each filter's character — they are cheaper
//! than libm and deliberately imprecise, so kernels must call these, never libm.
//! Only functions with a live kernel caller are here.
//!
//! Ported from https://github.com/surge-synthesizer/sst-basic-blocks (GPL-3.0).

/// Surge `softclip8_ps`: `y = x − (4/27/8³)·x³` with `x` clamped to ±12. The cubic
/// does not flatten smoothly at the clamp (Surge keeps it that way); it is the
/// feedback saturator that gives the legacy ladder its bite.
#[inline]
pub fn softclip8(x: f32) -> f32 {
    const A: f32 = -0.000_289_351_85;
    let x = x.clamp(-12.0, 12.0);
    x + A * x * x * x
}

/// Surge `fasttan`: Padé-style rational approximation of `tan(x)`, valid on
/// (−π/2, π/2). Used by coefficient makers for the bilinear-transform prewarp.
#[inline]
pub fn fasttan(x: f32) -> f32 {
    let x2 = x * x;
    let numerator = x * (-135135.0 + x2 * (17325.0 + x2 * (-378.0 + x2)));
    let denominator = -135135.0 + x2 * (62370.0 + x2 * (-3150.0 + 28.0 * x2));
    numerator / denominator
}

/// Surge `fasttanhSSEclamped`: the `tanh` rational approximation with its input
/// clamped to the approximation's ±5 validity range.
#[inline]
pub fn fasttanh_clamped(x: f32) -> f32 {
    let x = x.clamp(-5.0, 5.0);
    let x2 = x * x;
    let numerator = x * (135135.0 + x2 * (17325.0 + x2 * (378.0 + x2)));
    let denominator = 135135.0 + x2 * (62370.0 + x2 * (3150.0 + 28.0 * x2));
    numerator / denominator
}

/// Surge `fastexp`: rational approximation of `eˣ`, valid on (−6, 4).
#[inline]
pub fn fastexp(x: f32) -> f32 {
    let numerator = 1680.0 + x * (840.0 + x * (180.0 + x * (20.0 + x)));
    let denominator = 1680.0 + x * (-840.0 + x * (180.0 + x * (-20.0 + x)));
    numerator / denominator
}

/// Surge `fastsin`: Padé approximation of `sin(x)`, valid on (−π, π).
#[inline]
pub fn fastsin(x: f32) -> f32 {
    let x2 = x * x;
    let numerator =
        -x * (-11_511_339_840.0 + x2 * (1_640_635_920.0 + x2 * (-52_785_432.0 + x2 * 479_249.0)));
    let denominator = 11_511_339_840.0 + x2 * (277_920_720.0 + x2 * (3_177_720.0 + x2 * 18_361.0));
    numerator / denominator
}

/// Surge `fastcos`: Padé approximation of `cos(x)`, valid on (−π, π).
#[inline]
pub fn fastcos(x: f32) -> f32 {
    let x2 = x * x;
    let numerator = -(-39_251_520.0 + x2 * (18_471_600.0 + x2 * (-1_075_032.0 + 14_615.0 * x2)));
    let denominator = 39_251_520.0 + x2 * (1_154_160.0 + x2 * (16_632.0 + x2 * 127.0));
    numerator / denominator
}

/// Surge `softclip_ps`: `y = x − (4/27)·x³` with `x` clamped to ±1.5.
#[inline]
pub fn softclip(x: f32) -> f32 {
    const A: f32 = -4.0 / 27.0;
    let x = x.clamp(-1.5, 1.5);
    x + A * x * x * x
}
