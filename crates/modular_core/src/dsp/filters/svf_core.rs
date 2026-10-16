//! Andy Simper's (Cytomic) trapezoidal state-variable filter kernel, from
//! "Solving the continuous SVF equations using trapezoidal integration and
//! equivalent currents". State lives in the two integrators, so coefficients
//! can change every sample without the energy bursts a direct-form biquad
//! releases when its coefficients jump.

use crate::dsp::utils::sanitize;

/// Trapezoidal-integration gains plus an output mix of input (`m0`),
/// bandpass (`m1`) and lowpass (`m2`).
#[derive(Clone, Copy, Default)]
pub struct SvfCoeffs {
    a1: f32,
    a2: f32,
    a3: f32,
    m0: f32,
    m1: f32,
    m2: f32,
}

impl SvfCoeffs {
    /// `g` is the pre-warped integrator gain `tan(π·f/fs)`; `k` is `1/Q`.
    pub fn new(g: f32, k: f32, m0: f32, m1: f32, m2: f32) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self {
            a1,
            a2,
            a3,
            m0,
            m1,
            m2,
        }
    }
}

/// Runs one sample through the filter, updating its two integrator states.
/// `x` must be finite: the output stays finite because the coefficients and
/// states are.
#[inline]
pub fn process(c: &SvfCoeffs, ic1: &mut f32, ic2: &mut f32, x: f32) -> f32 {
    let v3 = x - *ic2;
    let v1 = c.a1 * *ic1 + c.a2 * v3;
    let v2 = *ic2 + c.a2 * *ic1 + c.a3 * v3;
    *ic1 = sanitize(2.0 * v1 - *ic1);
    *ic2 = sanitize(2.0 * v2 - *ic2);
    c.m0 * x + c.m1 * v1 + c.m2 * v2
}
