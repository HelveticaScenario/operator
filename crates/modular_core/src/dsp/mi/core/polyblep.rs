//! Two-sample polyBLEP residuals for a step `t` samples before the end of the
//! current sample, as in `stmlib/dsp/polyblep.h`.

#[inline(always)]
pub fn this_blep_sample(t: f32) -> f32 {
    0.5 * t * t
}

#[inline(always)]
pub fn next_blep_sample(t: f32) -> f32 {
    let t = 1.0 - t;
    -0.5 * t * t
}
