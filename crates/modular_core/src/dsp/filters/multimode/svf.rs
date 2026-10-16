//! The 2164 state-variable core: an inverting summer (IC14B) into two VCA-fed
//! integrators (IC16A/IC14A, IC16D/IC14D), with a resonance VCA (IC16B) adding
//! damping and a zener pair (D21/D24) limiting the bandpass.

use super::components::{ZENER_CAPACITANCE, ZENER_MIN_LN_I, zener_pair};
use crate::dsp::utils::sanitize;

/// Integrator input resistance (R132, R131). The zener pair across the first
/// integrator injects its current alongside HP's, so it enters the core as that
/// current times this resistance.
const R_INTEGRATOR: f64 = 30.0e3;
/// Below this |BP| the zener pair carries well under a microamp and the core is
/// solved as linear.
const BP_CLAMP_ONSET_V: f32 = 5.0;

/// Fixed negative damping from BP into the HP summer (R115 / R122 = 30k / 300k).
pub(super) const NEGATIVE_DAMPING: f32 = 0.1;
/// Damping per unit resonance-VCA gain: BP enters the integrator through 15k
/// (R142) against HP's 30k (R132).
pub(super) const DAMPING_PER_GAIN: f32 = 2.0;

/// Integrator capacitors (C62, C63).
const C_INTEGRATOR: f32 = 220.0e-12;

/// Prewarped gains `(g1, g2)` of the BP and LP integrators at nominal cutoff `fc`
/// (set by VCA gain `vca_gain`). The zener pair's capacitance runs from BP into
/// the first VCA's input, so its displacement current is amplified along with the
/// signal and slows that integrator by `1 + vca_gain · C_pair / C62`. The
/// resonance sits at the geometric mean of the two integrator rates, which pulls
/// the top of the range down to ~18.6 kHz.
pub(super) fn integrator_gains(fc: f32, vca_gain: f32, rate: f32) -> (f32, f32) {
    let slowdown = (1.0 + vca_gain * 0.5 * ZENER_CAPACITANCE / C_INTEGRATOR).sqrt();
    let resonance = (fc / slowdown).min(rate * 0.49);
    let g = (std::f32::consts::PI * resonance / rate).tan();
    (g / slowdown, g * slowdown)
}

/// Trapezoidal integrator states for `b = −BP` and LP, plus the zener pair's
/// log-current from the last clamped sample, which seeds the next solve.
#[derive(Clone, Copy)]
pub(super) struct SvfCore {
    s1: f32,
    s2: f32,
    ln_i_zener: f64,
}

impl Default for SvfCore {
    fn default() -> Self {
        Self {
            s1: 0.0,
            s2: 0.0,
            ln_i_zener: ZENER_SEED_LN_I,
        }
    }
}

impl SvfCore {
    /// Advances one sample for summer input `u = −IN`, integrator gains `(g1, g2)`
    /// from [`integrator_gains`] and total `damping`, returning `(lp, bp)`. In
    /// terms of `b = −BP` the circuit is
    ///   db/dt = ω1(u − lp − damping·b − R·i_z(b)),   dlp/dt = ω2·b,
    /// and trapezoidal integration makes `b` implicit.
    #[inline]
    pub(super) fn step(&mut self, u: f32, (g1, g2): (f32, f32), damping: f32) -> (f32, f32) {
        let denom = 1.0 + g1 * g2 + g1 * damping;
        let b = solve_clamped(
            self.s1 + g1 * (u - self.s2),
            g1,
            denom,
            &mut self.ln_i_zener,
        );
        let lp = self.s2 + g2 * b;
        self.s1 = sanitize(2.0 * b - self.s1);
        self.s2 = sanitize(2.0 * lp - self.s2);
        (lp, -b)
    }
}

/// Residual, in volts at the integrator, at which the clamp solve stops.
const SOLVE_TOL_V: f64 = 1.0e-5;
/// Zener log-current that seeds the first clamped solve (1 µA).
const ZENER_SEED_LN_I: f64 = -13.815_510_557_964_274;

/// Solves `b·denom + g·R·i_z(b) = rhs` for `b`, where `i_z` is the zener pair's
/// current at voltage `b`. Newton runs on the pair's log-current `ln_i`, where
/// its I–V curve is explicit, inside a bisection bracket. `ln_i` carries the
/// solution between samples: consecutive clamped samples sit close together, so
/// Newton starts next to the root.
fn solve_clamped(rhs: f32, g: f32, denom: f32, ln_i: &mut f64) -> f32 {
    let linear = rhs / denom;
    if linear.abs() < BP_CLAMP_ONSET_V {
        return linear;
    }
    let (target, denom, gr) = (rhs.abs() as f64, denom as f64, g as f64 * R_INTEGRATOR);
    // The pair's 1 pA floor sits under the onset; at the upper bound the
    // resistor term alone meets the target.
    let (mut lo, mut hi) = (ZENER_MIN_LN_I, (target / gr).ln());
    let mut t = ln_i.clamp(lo, hi);
    let mut zener = zener_pair(t);
    for _ in 0..40 {
        let r = denom * zener.voltage + gr * zener.current - target;
        if r.abs() < SOLVE_TOL_V {
            break;
        }
        if r > 0.0 {
            hi = t;
        } else {
            lo = t;
        }
        let next = t - r / (denom * zener.slope + gr * zener.current);
        t = if next > lo && next < hi {
            next
        } else {
            0.5 * (lo + hi)
        };
        zener = zener_pair(t);
    }
    *ln_i = t;
    (zener.voltage as f32).copysign(rhs)
}

#[cfg(test)]
mod tests {
    use super::super::components::zener_pair_voltage;
    use super::*;

    #[test]
    fn clamped_solution_satisfies_the_implicit_equation() {
        let (g, denom) = (0.05_f32, 1.0 + 0.05 * 0.05 - 0.05 * 0.04);
        for rhs in [5.5_f32, 6.0, 8.0, 20.0, -7.0] {
            let b = solve_clamped(rhs, g, denom, &mut ZENER_SEED_LN_I.clone()) as f64;
            // Recover the zener current from the solved voltage by bisection.
            let (mut lo, mut hi) = (-60.0_f64, 2.0_f64);
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if zener_pair_voltage(mid) < b.abs() {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let current = (0.5 * (lo + hi)).exp().copysign(b);
            let lhs = b * denom as f64 + g as f64 * R_INTEGRATOR * current;
            assert!((lhs - rhs as f64).abs() < 1.0e-3, "rhs={rhs}: lhs={lhs}");
        }
    }

    #[test]
    fn clamp_holds_bandpass_near_the_zener_knee() {
        // Drive the linear solution 1.5× and 4× past the knee.
        for rhs in [8.0, 20.0] {
            let b = solve_clamped(rhs, 0.05, 1.0, &mut ZENER_SEED_LN_I.clone());
            assert!((5.5..6.3).contains(&b), "rhs={rhs}: b = {b}");
        }
    }
}
