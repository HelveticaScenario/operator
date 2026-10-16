//! Discretised analog building blocks shared by the drum voices.

use crate::dsp::utils::sanitize;

/// Thermal voltage at room temperature.
pub const VT: f32 = 0.025_85;

/// One-pole filter coefficient `g / (1 + g)` for a cutoff in Hz.
pub fn one_pole_coeff(hz: f32, sample_rate: f32) -> f32 {
    let g = (std::f32::consts::PI * hz.min(0.49 * sample_rate) / sample_rate).tan();
    g / (1.0 + g)
}

/// Cutoff in Hz of an RC section.
pub fn rc_hz(r: f32, c: f32) -> f32 {
    1.0 / (std::f32::consts::TAU * r * c)
}

/// Trapezoidal one-pole low-pass with coefficient `g / (1 + g)`.
#[inline]
pub fn low_pass(state: &mut f32, input: f32, coeff: f32) -> f32 {
    let v = coeff * (input - *state);
    let lp = *state + v;
    *state = sanitize(lp + v);
    lp
}

/// Trapezoidal one-pole high-pass with coefficient `g / (1 + g)`.
#[inline]
pub fn high_pass(state: &mut f32, input: f32, coeff: f32) -> f32 {
    input - low_pass(state, input, coeff)
}
