//! The 2164 state-variable core: an inverting summer (IC14B) into two VCA-fed
//! integrators (IC16A/IC14A, IC16D/IC14D), with a resonance VCA (IC16B) adding
//! damping and a zener pair (D21/D24) limiting the bandpass.

use super::components::{
    ZENER_FLOOR_V, ZENER_MIN_LN_I, ZENER_PAIR_CAPACITANCE_0V, ZenerPoint, ZenerTables,
};
use super::lanes::{LANES, Lanes, clear_lane, f32x4, set_lane};
use super::vca::{self, GainX4};

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
#[derive(Clone, Copy)]
pub(super) struct CoreTuning {
    /// Prewarp angle `π · f0 / rate` of the small-signal resonance `f0`.
    theta: f32,
    tan_theta: f32,
    /// `√(1 + vca_gain · C_pair(0 V) / C62)`.
    sqrt_slowdown: f32,
    vca: vca::Gain,
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
            vca: vca::Gain::new(vca_gain),
        }
    }
}

/// A group's four [`CoreTuning`]s, one per lane.
#[derive(Clone, Copy, Default)]
pub(super) struct TuningX4 {
    theta: f32x4,
    tan_theta: f32x4,
    sqrt_slowdown: f32x4,
    vca: GainX4,
    /// Bit `l` set where lane `l` is below [`LINEAR_THETA`].
    linear: u32,
}

impl TuningX4 {
    pub(super) fn set(&mut self, lane: usize, tuning: &CoreTuning) {
        set_lane(&mut self.theta, lane, tuning.theta);
        set_lane(&mut self.tan_theta, lane, tuning.tan_theta);
        set_lane(&mut self.sqrt_slowdown, lane, tuning.sqrt_slowdown);
        self.vca.set(lane, &tuning.vca);
        let bit = 1 << lane;
        self.linear = if tuning.theta < LINEAR_THETA {
            self.linear | bit
        } else {
            self.linear & !bit
        };
    }
}

/// Largest prewarp angle the core runs at, just under Nyquist.
const MAX_THETA: f32 = std::f32::consts::PI * 0.49;
/// Prewarp angle below which `tan` is linear enough that the integrators' rate
/// changes scale their gains directly, within 2 cents.
const LINEAR_THETA: f32 = 0.25;

/// Integrator gains `(g1, g2)` where the rate changes scale the prewarped gain
/// directly.
#[inline]
fn linear_gains<L: Lanes>(tuning: &TuningX4, (rate1, rate2): (L, L)) -> (L, L) {
    let g = L::load(&tuning.tan_theta) * L::load(&tuning.sqrt_slowdown);
    (g * rate1, g * rate2)
}

/// Integrator gains near Nyquist: the resonance shift from the small-signal
/// tuning, applied to the prewarped gain by tan addition with a cubic tan for
/// the small step.
#[inline]
fn warped_gains<L: Lanes>(tuning: &TuningX4, (rate1, rate2): (L, L)) -> (L, L) {
    let (theta, tan_theta) = (L::load(&tuning.theta), L::load(&tuning.tan_theta));
    let split = (rate1 / rate2).sqrt();
    let shift = split * rate2 * L::load(&tuning.sqrt_slowdown);
    let step = (theta * (shift - L::splat(1.0))).min(L::splat(MAX_THETA) - theta);
    let tan_step = step * (L::splat(1.0) + step * step * L::splat(1.0 / 3.0));
    let g = (tan_theta + tan_step) / (L::splat(1.0) - tan_theta * tan_step);
    (g * split, g / split)
}

/// Trapezoidal integrator states for `b = −BP` and LP; BP and the first VCA's
/// input current from the last sample, which set this sample's zener
/// capacitance and VCA distortion; and the zener solve's seed. One lane per
/// channel.
#[derive(Clone, Copy)]
pub(super) struct SvfCore {
    s1: f32x4,
    s2: f32x4,
    bp: f32x4,
    vca_input: f32x4,
    zener: [ZenerSeed; LANES],
    /// Bit `l` set where lane `l` was clamped last sample.
    clamped: u32,
}

impl Default for SvfCore {
    fn default() -> Self {
        Self {
            s1: f32x4::default(),
            s2: f32x4::default(),
            bp: f32x4::default(),
            vca_input: f32x4::default(),
            zener: [ZenerSeed::default(); LANES],
            clamped: 0,
        }
    }
}

impl SvfCore {
    /// Advances one sample for summer input `u = −IN`, `tuning` and resonance
    /// VCA gain `res`, returning `(lp, bp)`. In terms of `b = −BP` the circuit
    /// is
    ///   db/dt = ω1(u − lp − damping·b − R·i_z(b)),   dlp/dt = ω2·b,
    /// and trapezoidal integration makes `b` implicit. The zener capacitance
    /// and the VCAs' second-harmonic terms follow last sample's BP and VCA
    /// input current. The zener clamp is solved on the first `active` lanes
    /// that reach it.
    #[inline]
    pub(super) fn step<L: Lanes>(
        &mut self,
        u: L,
        tuning: &TuningX4,
        res: &GainX4,
        zener: &ZenerTables,
        active: usize,
    ) -> (L, L) {
        let gain = &tuning.vca;
        let (bp, s1, s2) = (L::load(&self.bp), L::load(&self.s1), L::load(&self.s2));
        let rate1 = vca::scale(gain, L::load(&self.vca_input));
        // Integrator rates relative to their nominal `gain / (R·C)`.
        let inv_slowdown = L::splat(1.0)
            / (L::splat(1.0)
                + gain.gain::<L>() * rate1 * zener.capacitance(bp) * L::splat(1.0 / C_INTEGRATOR));
        // BP feeds the second integrator through R131 and the resonance VCA
        // through R142.
        let rates = (
            rate1 * inv_slowdown,
            vca::scale(gain, bp * L::splat(1.0 / R_INTEGRATOR as f32)),
        );
        let live = (1 << L::WIDTH.min(active)) - 1;
        let linear = tuning.linear & live;
        let (g1, g2) = if linear == live {
            linear_gains(tuning, rates)
        } else if linear == 0 {
            warped_gains(tuning, rates)
        } else {
            let linear = L::load(&tuning.theta).lt(L::splat(LINEAR_THETA));
            let lin = linear_gains(tuning, rates);
            let warp = warped_gains(tuning, rates);
            (
                L::select(linear, lin.0, warp.0),
                L::select(linear, lin.1, warp.1),
            )
        };
        let damping = L::splat(DAMPING_PER_GAIN)
            * res.gain::<L>()
            * vca::scale(res, bp * L::splat(1.0 / R_RESONANCE))
            - L::splat(NEGATIVE_DAMPING);

        let denom = L::splat(1.0) + g1 * g2 + g1 * damping;
        let rhs = s1 + g1 * (u - s2);
        let linear = rhs / denom;
        // A NaN takes the linear path, where the integrator state's sanitize
        // clears it.
        let clamped =
            L::bits(linear.abs().ge(L::splat(ZENER_FLOOR_V as f32))) & ((1 << active) - 1);
        let (b, i_zener) = if clamped == 0 {
            (linear, L::splat(0.0))
        } else {
            let (mut b, mut i_zener) = (linear.to_lanes(), [0.0; LANES]);
            let (rhs, g1, denom) = (rhs.to_lanes(), g1.to_lanes(), denom.to_lanes());
            for lane in (0..active).filter(|&lane| clamped & (1 << lane) != 0) {
                (b[lane], i_zener[lane]) = solve_clamped(
                    rhs[lane],
                    g1[lane],
                    denom[lane],
                    &mut self.zener[lane],
                    self.clamped & (1 << lane) != 0,
                    zener,
                );
            }
            (L::from_lanes(b), L::from_lanes(i_zener))
        };
        self.clamped = clamped;
        let lp = s2 + g2 * b;
        (L::splat(2.0) * b - s1).sanitize().store(&mut self.s1);
        (L::splat(2.0) * lp - s2).sanitize().store(&mut self.s2);
        (-b).sanitize().store(&mut self.bp);
        // The zener capacitance's displacement current joins the resistive
        // current at the VCA input, scaling their sum by `1 / slowdown`.
        let resistive = (u - lp - damping * b) * L::splat(1.0 / R_INTEGRATOR as f32) - i_zener;
        (resistive * inv_slowdown)
            .sanitize()
            .store(&mut self.vca_input);
        (lp, -b)
    }

    pub(super) fn clear_lane(&mut self, lane: usize) {
        clear_lane(&mut self.s1, lane);
        clear_lane(&mut self.s2, lane);
        clear_lane(&mut self.bp, lane);
        clear_lane(&mut self.vca_input, lane);
        self.zener[lane] = ZenerSeed::default();
        self.clamped &= !(1 << lane);
    }
}

/// Residual, in volts at the integrator, at which the clamp solve stops.
const SOLVE_TOL_V: f64 = 1.0e-5;
/// Residual below which one Newton step is taken on the pair's linearised
/// curve without evaluating it again; the step's error is second order, under
/// `SOLVE_TOL_V` here.
const LINEARIZE_TOL_V: f64 = 1.0e-2;
/// Zener log-current that seeds the first clamped solve (1 µA).
const ZENER_SEED_LN_I: f64 = -13.815_510_557_964_274;

/// The zener solve's last log-current and its change over the sample before,
/// which predict the next solution while the clamp holds.
#[derive(Clone, Copy)]
pub(super) struct ZenerSeed {
    ln_i: f64,
    step: f64,
}

impl Default for ZenerSeed {
    fn default() -> Self {
        Self {
            ln_i: ZENER_SEED_LN_I,
            step: 0.0,
        }
    }
}

/// Solves `b·denom + g·R·i_z(b) = rhs` for `b`, where `i_z` is the zener pair's
/// current at voltage `b`, returning `b` and that current. Newton runs on the
/// pair's log-current, where its I–V curve is explicit, inside a bisection
/// bracket. `seed` carries the solution between samples; while the clamp
/// holds (`continuing`), the last change extrapolates it, so Newton starts
/// next to the root.
fn solve_clamped(
    rhs: f32,
    g: f32,
    denom: f32,
    seed: &mut ZenerSeed,
    continuing: bool,
    zener: &ZenerTables,
) -> (f32, f32) {
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
    let guess = if continuing {
        seed.ln_i + seed.step
    } else {
        seed.ln_i
    };
    let mut t = guess.clamp(lo, hi);
    let mut point = zener.pair(t);
    for _ in 0..40 {
        let r = denom * point.voltage + gr * point.current - target;
        if r.abs() < SOLVE_TOL_V {
            break;
        }
        if r > 0.0 {
            hi = t;
        } else {
            lo = t;
        }
        let step = -r / (denom * point.slope + gr * point.current);
        let next = t + step;
        if !(next > lo && next < hi) {
            t = 0.5 * (lo + hi);
            point = zener.pair(t);
        } else if r.abs() < LINEARIZE_TOL_V {
            t = next;
            point = ZenerPoint {
                current: point.current * (1.0 + step * (1.0 + 0.5 * step)),
                voltage: point.voltage + point.slope * step,
                slope: point.slope,
            };
            break;
        } else {
            t = next;
            point = zener.pair(t);
        }
    }
    seed.step = if continuing { t - seed.ln_i } else { 0.0 };
    seed.ln_i = t;
    (
        (point.voltage as f32).copysign(rhs),
        (point.current as f32).copysign(rhs),
    )
}

#[cfg(test)]
mod tests {
    use super::super::components::{prime, zener_pair_voltage, zener_tables};
    use super::*;

    #[test]
    fn clamped_solution_satisfies_the_implicit_equation() {
        let (g, denom) = (0.05_f32, 1.0 + 0.05 * 0.05 - 0.05 * 0.04);
        for rhs in [3.0_f32, 4.5, 6.0, 8.0, 20.0, -7.0] {
            prime();
            let (b, current) = solve_clamped(
                rhs,
                g,
                denom,
                &mut ZenerSeed::default(),
                false,
                zener_tables(),
            );
            // The returned current is the pair's at the solved voltage.
            let ln_i = (current.abs() as f64).ln();
            assert!((zener_pair_voltage(ln_i) - b.abs() as f64).abs() < 1.0e-3);
            let lhs = b as f64 * denom as f64 + g as f64 * R_INTEGRATOR * current as f64;
            assert!((lhs - rhs as f64).abs() < 1.0e-3, "rhs={rhs}: lhs={lhs}");
        }
    }

    #[test]
    fn continuing_solves_meet_the_tolerance() {
        prime();
        let (g, denom) = (0.3_f32, 1.05_f32);
        let mut seed = ZenerSeed::default();
        for n in 0..20000 {
            let swing = (n as f32 * 0.003).sin().abs() * (1.0 + 0.5 * (n as f32 * 0.0007).sin());
            let rhs = 4.0 + 6.0 * swing;
            let (b, current) = solve_clamped(rhs, g, denom, &mut seed, n > 0, zener_tables());
            let lhs = b as f64 * denom as f64 + g as f64 * R_INTEGRATOR * current as f64;
            assert!((lhs - rhs as f64).abs() < 3.0 * SOLVE_TOL_V, "sample {n}");
            // The current is the pair's at the returned voltage.
            let ln_i = (current.abs() as f64).ln();
            let v = zener_pair_voltage(ln_i);
            assert!((v - b.abs() as f64).abs() < 3.0 * SOLVE_TOL_V, "sample {n}");
        }
    }

    #[test]
    fn clamp_holds_bandpass_near_the_zener_knee() {
        // Drive the linear solution 1.5× and 4× past the knee.
        for rhs in [8.0, 20.0] {
            prime();
            let (b, _) = solve_clamped(
                rhs,
                0.05,
                1.0,
                &mut ZenerSeed::default(),
                false,
                zener_tables(),
            );
            assert!((5.6..6.0).contains(&b), "rhs={rhs}: b = {b}");
        }
    }
}
