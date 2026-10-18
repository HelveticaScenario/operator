//! The 2164 state-variable core: an inverting summer (IC14B) into two VCA-fed
//! integrators (IC16A/IC14A, IC16D/IC14D), with a resonance VCA (IC16B) adding
//! damping and a zener pair (D21/D24) limiting the bandpass.

use super::components::{
    ZENER_FLOOR_V, ZENER_MIN_LN_I, ZENER_PAIR_CAPACITANCE_0V, vca_hd2, zener_pair,
    zener_pair_capacitance,
};
use crate::dsp::utils::sanitize;

/// Integrator input resistance (R132, R131). The zener pair across the first
/// integrator injects its current alongside HP's, so it enters the core as that
/// current times this resistance.
const R_INTEGRATOR: f64 = 30.0e3;
/// BP feeds the resonance VCA through R142.
const R_RESONANCE: f32 = 15.0e3;

/// Fixed negative damping from BP into the HP summer (R115 / R122 = 30k / 300k).
pub(super) const NEGATIVE_DAMPING: f32 = 0.1;
/// Damping per unit resonance-VCA gain: BP enters the integrator through 15k
/// (R142) against HP's 30k (R132).
const DAMPING_PER_GAIN: f32 = 2.0;

/// Integrator capacitors (C62, C63).
const C_INTEGRATOR: f32 = 220.0e-12;

/// The core's control-rate tuning at its small-signal operating point.
#[derive(Clone, Copy, Default)]
pub(super) struct CoreTuning {
    /// Prewarp angle `π · f0 / rate` of the small-signal resonance `f0`.
    theta: f32,
    tan_theta: f32,
    /// `√(1 + vca_gain · C_pair(0 V) / C62)`.
    sqrt_slowdown: f32,
    vca_gain: f32,
}

impl CoreTuning {
    /// Tuning for nominal cutoff `fc`, set by VCA gain `vca_gain`. The zener
    /// pair's capacitance runs from BP into the first VCA's input, so its
    /// displacement current is amplified along with the signal and slows that
    /// integrator by `1 + vca_gain · C_pair / C62`. The resonance sits at the
    /// geometric mean of the two integrator rates.
    pub(super) fn new(fc: f32, vca_gain: f32, rate: f32) -> Self {
        let sqrt_slowdown = (1.0 + vca_gain * ZENER_PAIR_CAPACITANCE_0V / C_INTEGRATOR).sqrt();
        let theta = std::f32::consts::PI * (fc / sqrt_slowdown).min(rate * 0.49) / rate;
        Self {
            theta,
            tan_theta: theta.tan(),
            sqrt_slowdown,
            vca_gain,
        }
    }
}

/// Largest prewarp angle the core runs at, just under Nyquist.
const MAX_THETA: f32 = std::f32::consts::PI * 0.49;
/// Prewarp angle below which `tan` is linear enough that the integrators' rate
/// changes scale their gains directly, within 2 cents.
const LINEAR_THETA: f32 = 0.25;

/// Trapezoidal integrator states for `b = −BP` and LP; BP and the first VCA's
/// input current from the last sample, which set this sample's zener
/// capacitance and VCA distortion; and the zener pair's log-current from the
/// last clamped sample, which seeds the next solve.
#[derive(Clone, Copy)]
pub(super) struct SvfCore {
    s1: f32,
    s2: f32,
    bp: f32,
    vca_input: f32,
    ln_i_zener: f64,
}

impl Default for SvfCore {
    fn default() -> Self {
        Self {
            s1: 0.0,
            s2: 0.0,
            bp: 0.0,
            vca_input: 0.0,
            ln_i_zener: ZENER_SEED_LN_I,
        }
    }
}

impl SvfCore {
    /// Advances one sample for summer input `u = −IN`, `tuning` from
    /// [`CoreTuning::new`] and resonance VCA gain `res_gain`, returning
    /// `(lp, bp)`. In terms of `b = −BP` the circuit is
    ///   db/dt = ω1(u − lp − damping·b − R·i_z(b)),   dlp/dt = ω2·b,
    /// and trapezoidal integration makes `b` implicit. The zener capacitance
    /// and the VCAs' second-harmonic terms follow last sample's BP and VCA
    /// input current.
    #[inline]
    pub(super) fn step(&mut self, u: f32, tuning: CoreTuning, res_gain: f32) -> (f32, f32) {
        let gain = tuning.vca_gain;
        // Integrator rates relative to their nominal `gain / (R·C)`.
        let rate1 = vca_hd2(gain, self.vca_input);
        let inv_slowdown =
            1.0 / (1.0 + gain * rate1 * zener_pair_capacitance(self.bp) / C_INTEGRATOR);
        let rate1 = rate1 * inv_slowdown;
        let rate2 = vca_hd2(gain, self.bp / R_INTEGRATOR as f32);
        let (g1, g2) = if tuning.theta < LINEAR_THETA {
            let g = tuning.tan_theta * tuning.sqrt_slowdown;
            (g * rate1, g * rate2)
        } else {
            // Resonance shift from the small-signal tuning, applied to the
            // prewarped gain by tan addition with a cubic tan for the small step.
            let split = (rate1 / rate2).sqrt();
            let shift = split * rate2 * tuning.sqrt_slowdown;
            let step = (tuning.theta * (shift - 1.0)).min(MAX_THETA - tuning.theta);
            let tan_step = step * (1.0 + step * step / 3.0);
            let g = (tuning.tan_theta + tan_step) / (1.0 - tuning.tan_theta * tan_step);
            (g * split, g / split)
        };
        let damping = DAMPING_PER_GAIN * res_gain * vca_hd2(res_gain, self.bp / R_RESONANCE)
            - NEGATIVE_DAMPING;

        let denom = 1.0 + g1 * g2 + g1 * damping;
        let (b, i_zener) = solve_clamped(
            self.s1 + g1 * (u - self.s2),
            g1,
            denom,
            &mut self.ln_i_zener,
        );
        let lp = self.s2 + g2 * b;
        self.s1 = sanitize(2.0 * b - self.s1);
        self.s2 = sanitize(2.0 * lp - self.s2);
        self.bp = sanitize(-b);
        // The zener capacitance's displacement current joins the resistive
        // current at the VCA input, scaling their sum by `1 / slowdown`.
        let resistive = (u - lp - damping * b) / R_INTEGRATOR as f32 - i_zener;
        self.vca_input = sanitize(resistive * inv_slowdown);
        (lp, -b)
    }
}

/// Residual, in volts at the integrator, at which the clamp solve stops.
const SOLVE_TOL_V: f64 = 1.0e-5;
/// Zener log-current that seeds the first clamped solve (1 µA).
const ZENER_SEED_LN_I: f64 = -13.815_510_557_964_274;

/// Solves `b·denom + g·R·i_z(b) = rhs` for `b`, where `i_z` is the zener pair's
/// current at voltage `b`, returning `b` and that current. Newton runs on the
/// pair's log-current `ln_i`, where its I–V curve is explicit, inside a
/// bisection bracket. `ln_i` carries the solution between samples:
/// consecutive clamped samples sit close together, so Newton starts next to
/// the root.
fn solve_clamped(rhs: f32, g: f32, denom: f32, ln_i: &mut f64) -> (f32, f32) {
    let linear = rhs / denom;
    // A NaN takes the linear path, where the integrator state's sanitize
    // clears it.
    if linear.is_nan() || linear.abs() < ZENER_FLOOR_V as f32 {
        return (linear, 0.0);
    }
    let (target, denom, gr) = (rhs.abs() as f64, denom as f64, g as f64 * R_INTEGRATOR);
    // At the 10 nA floor the pair sits at the onset; at the upper bound the
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
    (
        (zener.voltage as f32).copysign(rhs),
        (zener.current as f32).copysign(rhs),
    )
}

#[cfg(test)]
mod tests {
    use super::super::components::zener_pair_voltage;
    use super::*;

    #[test]
    fn clamped_solution_satisfies_the_implicit_equation() {
        let (g, denom) = (0.05_f32, 1.0 + 0.05 * 0.05 - 0.05 * 0.04);
        for rhs in [3.0_f32, 4.5, 6.0, 8.0, 20.0, -7.0] {
            let (b, current) = solve_clamped(rhs, g, denom, &mut ZENER_SEED_LN_I.clone());
            // The returned current is the pair's at the solved voltage.
            let ln_i = (current.abs() as f64).ln();
            assert!((zener_pair_voltage(ln_i) - b.abs() as f64).abs() < 1.0e-3);
            let lhs = b as f64 * denom as f64 + g as f64 * R_INTEGRATOR * current as f64;
            assert!((lhs - rhs as f64).abs() < 1.0e-3, "rhs={rhs}: lhs={lhs}");
        }
    }

    #[test]
    fn clamp_holds_bandpass_near_the_zener_knee() {
        // Drive the linear solution 1.5× and 4× past the knee.
        for rhs in [8.0, 20.0] {
            let (b, _) = solve_clamped(rhs, 0.05, 1.0, &mut ZENER_SEED_LN_I.clone());
            assert!((5.6..6.0).contains(&b), "rhs={rhs}: b = {b}");
        }
    }
}
