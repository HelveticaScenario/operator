//! Tabulated odd transfer curves for the static nonlinearities.

/// Tables cover `|x|` up to `MAX` in uniform steps, past the ±50 V full drive
/// gives a ±5 V input; beyond that a curve is extrapolated linearly.
const PER_UNIT: f64 = 128.0;
const MAX: f64 = 64.0;
const SEGMENTS: usize = (MAX * PER_UNIT) as usize;

/// Odd transfer curve sampled on `[0, MAX]`, as `(value, antiderivative)` at
/// each node; the antiderivative is even.
pub(super) struct Curve {
    nodes: Box<[(f64, f64)]>,
}

impl Curve {
    pub(super) fn build(f: impl Fn(f64) -> f64) -> Self {
        let value: Vec<f64> = (0..=SEGMENTS).map(|i| f(i as f64 / PER_UNIT)).collect();
        let mut integral = 0.0;
        let nodes = (0..=SEGMENTS)
            .map(|i| {
                if i > 0 {
                    integral += 0.5 * (value[i - 1] + value[i]) / PER_UNIT;
                }
                (value[i], integral)
            })
            .collect();
        Self { nodes }
    }

    /// Segment index and fractional position for `a = |x|`, clamped to the
    /// last segment so positions past the table extrapolate it.
    #[inline]
    fn locate(a: f64) -> (usize, f64) {
        let pos = a * PER_UNIT;
        let i = (pos as usize).min(SEGMENTS - 1);
        (i, pos - i as f64)
    }

    #[inline]
    pub(super) fn eval(&self, x: f64) -> f64 {
        let (i, t) = Self::locate(x.abs());
        let (v0, v1) = (self.nodes[i].0, self.nodes[i + 1].0);
        let v = v0 + (v1 - v0) * t;
        if x < 0.0 { -v } else { v }
    }

    /// Exact integral of the piecewise-linear interpolant from 0 to `x`.
    #[inline]
    pub(super) fn antiderivative(&self, x: f64) -> f64 {
        let (i, t) = Self::locate(x.abs());
        let ((v0, integral), v1) = (self.nodes[i], self.nodes[i + 1].0);
        integral + (v0 * t + 0.5 * (v1 - v0) * t * t) / PER_UNIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_and_extrapolates() {
        let curve = Curve::build(|x| x.sqrt());
        for x in [0.3, 15.99, 16.0, 40.0, 63.9] {
            assert!((curve.eval(x) - f64::sqrt(x)).abs() < 1.0e-3, "x={x}");
            assert_eq!(curve.eval(-x), -curve.eval(x));
        }
        // Past the table the last segment continues.
        let slope = (curve.eval(MAX) - curve.eval(MAX - 1.0 / PER_UNIT)) * PER_UNIT;
        assert!((curve.eval(MAX + 1.0) - curve.eval(MAX) - slope).abs() < 1.0e-9);
    }

    #[test]
    fn antiderivative_matches_closed_form() {
        let curve = Curve::build(|x| x * x);
        for x in [5.0, 16.0, 40.0, 63.0] {
            let exact = x * x * x / 3.0;
            assert!(
                (curve.antiderivative(x) - exact).abs() < 1.0e-4 * exact,
                "x={x}"
            );
        }
    }
}
