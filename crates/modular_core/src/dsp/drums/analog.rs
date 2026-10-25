//! Discretised analog building blocks shared by the drum voices.

use super::lanes::{Lanes, Mask, Mono, Real, c, lanewise};

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
pub fn low_pass<S: Real<Elem = f32>>(state: &mut S, input: S, coeff: S) -> S {
    let v = (input - *state) * coeff;
    let lp = *state + v;
    *state = (lp + v).sanitize();
    lp
}

/// Trapezoidal one-pole high-pass with coefficient `g / (1 + g)`.
#[inline]
pub fn high_pass<S: Real<Elem = f32>>(state: &mut S, input: S, coeff: S) -> S {
    input - low_pass(state, input, coeff)
}

/// The squared Newton step on a junction voltage, against an exponential of
/// scale `volts`, below which the solve is within `tolerance`: Newton
/// converges quadratically, so the step after it would be at most
/// step²/(2·volts).
pub const fn converged(tolerance: f64, volts: f64) -> f64 {
    2.0 * volts * tolerance
}

/// Bound on an exponential's argument, keeping junction currents finite.
const EXP_LIMIT: f64 = 80.0;
/// Steps shorter than this move an [`Exp`] by its Taylor series, within
/// 2e-12 of `exp`.
const TAYLOR_STEP: f64 = 1.0 / 8.0;
/// An [`Exp`] is recomputed exactly after this many Taylor steps, bounding
/// the rounding they accumulate.
const REFRESH: f64 = 4096.0;

/// A junction's exponential e^(scale·x), followed from one Newton iterate,
/// and one sample, to the next: a short step costs a few multiplies rather
/// than an `exp`, whose latency would otherwise dominate the solve. The
/// argument is clamped to ±80. The default computes afresh on first use.
#[derive(Clone, Copy)]
pub struct Exp<L: Lanes = Mono> {
    arg: L::D,
    pub value: L::D,
    steps: L::D,
}

lanewise!(Exp { arg, value, steps });

impl<L: Lanes> Default for Exp<L> {
    fn default() -> Self {
        Self {
            arg: c(0.0),
            value: c(1.0),
            steps: c(REFRESH),
        }
    }
}

impl<L: Lanes> Exp<L> {
    /// The exponential at `x`, for the same `scale`.
    #[inline]
    pub fn at(self, x: L::D, scale: f64) -> Self {
        let arg = (x * scale).clamp(c(-EXP_LIMIT), c(EXP_LIMIT));
        let d = arg - self.arg;
        let far = d.abs().ge(c(TAYLOR_STEP)) | self.steps.ge(c(REFRESH));
        if far.all() {
            return Self {
                arg,
                value: arg.map(f64::exp),
                steps: c(0.0),
            };
        }
        let (d2, d4) = (d * d, d * d * (d * d));
        let low = d.mul_add(c(1.0 / 6.0), c(0.5)).mul_add(d2, d + 1.0);
        let high = d
            .mul_add(c(1.0 / 5040.0), c(1.0 / 720.0))
            .mul_add(d2, d.mul_add(c(1.0 / 120.0), c(1.0 / 24.0)));
        let value = self.value * high.mul_add(d4, low);
        let steps = self.steps + 1.0;
        if !far.any() {
            return Self { arg, value, steps };
        }
        Self {
            arg,
            value: exact::<L>(arg, far, value),
            steps: L::D::select(far, c(0.0), steps),
        }
    }

    /// [`Self::at`] in the lanes of `mask`; the others keep `self`.
    #[inline]
    pub fn at_where(self, mask: L::DM, x: L::D, scale: f64) -> Self {
        Self::select(mask, self.at(x, scale), self)
    }

    /// `a` in the lanes of `mask`, `b` in the others.
    #[inline]
    pub fn select(mask: L::DM, a: Self, b: Self) -> Self {
        Self {
            arg: L::D::select(mask, a.arg, b.arg),
            value: L::D::select(mask, a.value, b.value),
            steps: L::D::select(mask, a.steps, b.steps),
        }
    }
}

/// `exp` of `arg` in the lanes of `far`, `taylor` in the others: a group
/// whose lanes differ. Out of line and cold, so the compiler cannot hoist
/// the `exp` into every step.
#[cold]
#[inline(never)]
fn exact<L: Lanes>(arg: L::D, far: L::DM, taylor: L::D) -> L::D {
    arg.map_where(far, taylor, f64::exp)
}

/// A junction's voltage and its exponential, carried from sample to sample
/// as the next solve's starting point.
#[derive(Clone, Copy, Default)]
pub struct Junction<L: Lanes = Mono> {
    pub v: L::D,
    pub e: Exp<L>,
}

lanewise!(Junction { v, e });
