use super::tables::LUT_SINE;

const SINE_LUT_SIZE: f32 = 512.0;

/// Plaits' `Sine`: one cycle per unit of `phase`, read from a 512-point table
/// with linear interpolation. Wraps any finite phase.
#[inline(always)]
pub fn sine(phase: f32) -> f32 {
    let index = (phase - phase.floor()) * SINE_LUT_SIZE;
    let integral = index as usize;
    let fractional = index - integral as f32;
    let a = LUT_SINE[integral];
    let b = LUT_SINE[integral + 1];
    a + (b - a) * fractional
}
