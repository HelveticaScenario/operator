//! Tabulated odd transfer curves for the static nonlinearities.

/// Tables cover `|x|` up to 16 in fine steps, where the curves bend, then up
/// to 272 in coarse steps along the zeners' slow logarithmic rise; beyond that
/// a curve is extrapolated linearly.
const FINE_MAX: f64 = 16.0;
const FINE_SEGMENTS: usize = 2048;
const FINE_STEP: f64 = FINE_MAX / FINE_SEGMENTS as f64;
const COARSE_STEP: f64 = 0.125;
const COARSE_SEGMENTS: usize = 2048;
const SEGMENTS: usize = FINE_SEGMENTS + COARSE_SEGMENTS;

/// Start of segment `i` and its width.
fn segment(i: usize) -> (f64, f64) {
    if i < FINE_SEGMENTS {
        (i as f64 * FINE_STEP, FINE_STEP)
    } else {
        (
            FINE_MAX + (i - FINE_SEGMENTS) as f64 * COARSE_STEP,
            COARSE_STEP,
        )
    }
}

/// Odd transfer curve sampled on `[0, 272]`, plus its (even) antiderivative.
pub(super) struct Curve {
    value: Box<[f64]>,
    integral: Box<[f64]>,
}

impl Curve {
    pub(super) fn build(f: impl Fn(f64) -> f64) -> Self {
        let value: Box<[f64]> = (0..=SEGMENTS).map(|i| f(segment(i).0)).collect();
        let mut integral = vec![0.0; SEGMENTS + 1].into_boxed_slice();
        for i in 1..=SEGMENTS {
            let width = segment(i - 1).1;
            integral[i] = integral[i - 1] + 0.5 * (value[i - 1] + value[i]) * width;
        }
        Self { value, integral }
    }

    /// Segment index, fractional position and width for `a = |x|`, clamped to
    /// the last segment so positions past the table extrapolate it.
    #[inline]
    fn locate(a: f64) -> (usize, f64, f64) {
        let (i, t) = if a < FINE_MAX {
            let pos = a / FINE_STEP;
            let i = pos as usize;
            (i, pos - i as f64)
        } else {
            let pos = (a - FINE_MAX) / COARSE_STEP;
            let i = (pos as usize).min(COARSE_SEGMENTS - 1);
            (FINE_SEGMENTS + i, pos - i as f64)
        };
        (i, t, segment(i).1)
    }

    #[inline]
    pub(super) fn eval(&self, x: f64) -> f64 {
        let (i, t, _) = Self::locate(x.abs());
        let v = self.value[i] + (self.value[i + 1] - self.value[i]) * t;
        if x < 0.0 { -v } else { v }
    }

    /// Exact integral of the piecewise-linear interpolant from 0 to `x`.
    #[inline]
    pub(super) fn antiderivative(&self, x: f64) -> f64 {
        let (i, t, width) = Self::locate(x.abs());
        let (v0, v1) = (self.value[i], self.value[i + 1]);
        self.integral[i] + (v0 * t + 0.5 * (v1 - v0) * t * t) * width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_across_both_regions() {
        let curve = Curve::build(|x| x.sqrt());
        for x in [0.3, 15.99, 16.0, 16.06, 100.0, 271.9] {
            assert!((curve.eval(x) - f64::sqrt(x)).abs() < 1.0e-3, "x={x}");
            assert_eq!(curve.eval(-x), -curve.eval(x));
        }
    }

    #[test]
    fn antiderivative_matches_closed_form_across_the_seam() {
        let curve = Curve::build(|x| x * x);
        for x in [5.0, 16.0, 40.0, 200.0] {
            let exact = x * x * x / 3.0;
            assert!(
                (curve.antiderivative(x) - exact).abs() < 1.0e-4 * exact,
                "x={x}"
            );
        }
    }
}
