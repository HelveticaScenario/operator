//! First-order antiderivative anti-aliasing (ADAA).

/// Step size below which the difference quotient is numerically unstable, so the
/// nonlinearity's direct value is used instead.
const ADAA_TOL: f32 = 1.0e-4;

/// One channel of ADAA state.
///
/// Given a nonlinearity `F` with antiderivative `AD`, first-order ADAA outputs
/// the average value of `F` over the last step, `(AD(x) − AD(x_prev)) / (x −
/// x_prev)`, which suppresses the aliasing a hard nonlinearity would otherwise
/// fold back into the band. Where the step is too small to divide (and on the
/// very first sample) it falls back to `F(x)`. The caller supplies the
/// precomputed `f = F(x)` and `ad = AD(x)`; this holds the one-sample memory.
#[derive(Clone, Copy, Debug)]
pub struct Adaa {
    x_prev: f32,
    ad_prev: f32,
    /// True only for the first sample after (re)initialization, when there is no
    /// valid previous sample to difference against.
    init: bool,
}

impl Default for Adaa {
    fn default() -> Self {
        Self {
            x_prev: 0.0,
            ad_prev: 0.0,
            init: true,
        }
    }
}

impl Adaa {
    /// Drop the one-sample memory; the next `process` re-seeds and returns `F(x)`
    /// directly. Callers must do this when the nonlinearity itself changes:
    /// differencing the new antiderivative against the old one's memory divides
    /// their offset by a near-zero step, producing an unbounded spike.
    #[inline]
    pub fn reset(&mut self) {
        self.init = true;
    }

    /// Combine the precomputed `f = F(x)` and `ad = AD(x)` with the stored
    /// previous sample, advancing the memory.
    #[inline]
    pub fn process(&mut self, x: f32, f: f32, ad: f32) -> f32 {
        if self.init {
            self.x_prev = x;
            self.ad_prev = ad;
            self.init = false;
            return f;
        }
        let dx = x - self.x_prev;
        // Below tolerance, fall back to F(x) to avoid an unstable divide.
        let r = if dx.abs() < ADAA_TOL {
            f
        } else {
            (ad - self.ad_prev) / dx
        };
        self.x_prev = x;
        self.ad_prev = ad;
        r
    }
}
