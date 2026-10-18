//! Blades drive stage: the zener-clamped transimpedance amp after the drive VCA
//! and the two-stage diode wavefolder behind it, as static transfer curves.
//!
//! Both curves are memoryless functions of `x`, the drive VCA output expressed as
//! the voltage its transimpedance amp would reach with no clamp. They are solved
//! once from the component equations, tabulated with their antiderivatives, and
//! evaluated with first-order ADAA. Antiderivatives are kept in `f64` so the
//! ADAA difference quotient does not cancel catastrophically on small steps.

use std::sync::LazyLock;

use super::components::{DIODE_IS, DIODE_NVT, zener_clamp};
use super::curve::Curve;

/// Drive transimpedance feedback resistor (R24), in parallel with the zener pair.
const R_DRIVE_FB: f64 = 30.0e3;
/// Series resistor feeding each folder stage's diode clamp (R66, R65).
const R_FOLD_CLAMP: f64 = 100.0e3;
/// First folder stage: non-inverting gain 1 + R58/R61 on the clamped input, minus
/// R58/R61 of the raw input.
const FOLD1_RATIO: f64 = 30.0 / 100.0;
/// Second folder stage: same topology with R51/R50, both 100k.
const FOLD2_RATIO: f64 = 1.0;

pub(super) struct DriveCurves {
    clip: Curve,
    fold: Curve,
}

static CURVES: LazyLock<DriveCurves> = LazyLock::new(|| DriveCurves {
    clip: Curve::build(zener_clip),
    fold: Curve::build(|x| wavefold(zener_clip(x))),
});

/// Build the curves now (main thread) so the audio thread never triggers it.
pub(super) fn prime() {
    LazyLock::force(&CURVES);
}

/// Magnitude of the drive transimpedance output for an unclamped target `x`.
fn zener_clip(x: f64) -> f64 {
    zener_clamp(x, 1.0, R_DRIVE_FB)
}

/// Voltage across an anti-parallel 1N4148 pair fed from `v` through 100 kΩ.
fn diode_clamp(v: f64) -> f64 {
    let a = v.abs();
    let (mut lo, mut hi) = (0.0_f64, a);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        let i = 2.0 * DIODE_IS * (mid / DIODE_NVT).sinh();
        if mid + R_FOLD_CLAMP * i < a {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (0.5 * (lo + hi)).copysign(v)
}

/// The two cascaded folder stages (IC6B, IC6A). Each passes small signals at
/// unity and folds once its diode clamp saturates.
fn wavefold(v: f64) -> f64 {
    let stage = |v: f64, ratio: f64| (1.0 + ratio) * diode_clamp(v) - ratio * v;
    stage(stage(v, FOLD1_RATIO), FOLD2_RATIO)
}

/// Step below which the ADAA quotient is replaced by the interpolant at the
/// midpoint, which equals the exact average within one linear segment.
const ADAA_TOL: f64 = 1.0e-6;

/// First-order ADAA memory for one curve on one channel. `ad_prev` is `None`
/// after a skipped sample and is rebuilt from `x_prev` on the next one.
#[derive(Clone, Copy, Default)]
pub(super) struct CurveAdaa {
    x_prev: f64,
    ad_prev: Option<f64>,
}

impl CurveAdaa {
    #[inline]
    fn process(&mut self, curve: &Curve, x: f64) -> f64 {
        let ad = curve.antiderivative(x);
        let ad_prev = self
            .ad_prev
            .unwrap_or_else(|| curve.antiderivative(self.x_prev));
        let dx = x - self.x_prev;
        let y = if dx.abs() < ADAA_TOL {
            curve.eval(0.5 * (x + self.x_prev))
        } else {
            (ad - ad_prev) / dx
        };
        self.x_prev = x;
        self.ad_prev = Some(ad);
        y
    }

    /// Tracks `x` while the curve's output is unused.
    #[inline]
    fn skip(&mut self, x: f64) {
        self.x_prev = x;
        self.ad_prev = None;
    }
}

/// Treble boost of [`PreEmphasis`] per unit of `1 − cos ω`. At 1/8 the FIR
/// inverts ADAA's `cos(ω/2)` to second order in ω.
const PRE_EMPHASIS: f32 = 0.125;

/// First-order ADAA averages adjacent samples, a `cos(ω/2)` roll-off that at
/// the engine rate takes audible treble off everything through the drive
/// stage. This symmetric three-tap FIR, `1 + 2k(1 − cos ω)`, restores it ahead
/// of the curves at the cost of one sample of delay.
#[derive(Clone, Copy, Default)]
pub(super) struct PreEmphasis {
    x1: f32,
    x2: f32,
}

impl PreEmphasis {
    #[inline]
    pub(super) fn process(&mut self, x: f32) -> f32 {
        let y = self.x1 + PRE_EMPHASIS * (2.0 * self.x1 - x - self.x2);
        self.x2 = self.x1;
        self.x1 = x;
        y
    }
}

/// Per-channel drive-stage memory.
#[derive(Clone, Copy, Default)]
pub(super) struct DriveState {
    clip: CurveAdaa,
    fold: CurveAdaa,
}

impl DriveState {
    /// The filter input for drive VCA output `x`: the clipped and folded paths
    /// weighted by the R80 crossfade gains. Each path keeps the sign of `x`
    /// because the stage's two inversions cancel. A path whose gain is zero is
    /// skipped.
    #[inline]
    pub(super) fn process(&mut self, x: f32, clean_gain: f32, fold_gain: f32) -> f32 {
        let curves = &*CURVES;
        let x = x as f64;
        let path = |adaa: &mut CurveAdaa, curve: &Curve, gain: f32| {
            if gain == 0.0 {
                adaa.skip(x);
                0.0
            } else {
                gain * adaa.process(curve, x) as f32
            }
        };
        path(&mut self.clip, &curves.clip, clean_gain)
            + path(&mut self.fold, &curves.fold, fold_gain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_is_transparent_below_the_zener_knee() {
        for x in [0.0, 0.5, 1.0, 2.0] {
            assert!((zener_clip(x) - x).abs() < 1e-3, "x={x}");
        }
    }

    #[test]
    fn clip_compresses_softly_then_saturates() {
        // The 5.1 V zeners' soft knee bends the curve from about 3 V.
        assert!(zener_clip(4.0) < 3.85);
        let at_12 = zener_clip(12.0);
        assert!((5.1..5.3).contains(&at_12), "clip(12) = {at_12}");
        assert!(zener_clip(16.0) > at_12, "clip must stay monotonic");
    }

    #[test]
    fn folder_is_near_unity_for_small_signals() {
        // Each diode's ~9 MΩ small-signal resistance divides against its 100 kΩ feed.
        assert!((wavefold(0.01) / 0.01 - 0.964).abs() < 0.005);
    }

    #[test]
    fn drive_curves_match_spice() {
        // Each row is (x, clipped, folded) from an ngspice DC sweep of the
        // transcribed drive stage with the vendor zener model, with signs
        // referred to the filter input.
        let reference = [
            (0.5, 0.5, 0.1857),
            (1.0, 1.0, 0.1470),
            (2.0, 1.9999, -0.0810),
            (3.0, 2.9918, -0.1772),
            (4.0, 3.8085, -0.0486),
            (5.0, 4.2939, 0.0553),
            (6.0, 4.5751, 0.1201),
            (12.0, 5.2084, 0.2736),
        ];
        for (x, clip, fold) in reference {
            let c = zener_clip(x);
            let f = wavefold(c);
            assert!((c - clip).abs() < 0.002, "clip({x}) = {c}, SPICE {clip}");
            assert!((f - fold).abs() < 0.003, "fold({x}) = {f}, SPICE {fold}");
        }
    }

    #[test]
    fn a_skipped_path_resumes_as_if_it_had_run() {
        prime();
        let input = |n: usize| 7.0 * (n as f32 * 0.05).sin();
        let mut always = DriveState::default();
        let mut gated = DriveState::default();
        for n in 0..400 {
            let x = input(n);
            let reference = always.process(x, 0.5, 2.0);
            let fold_gain = if (100..200).contains(&n) { 0.0 } else { 2.0 };
            let y = gated.process(x, 0.5, fold_gain);
            if fold_gain != 0.0 {
                assert_eq!(y, reference, "sample {n}");
            }
        }
    }

    #[test]
    fn antiderivative_matches_numeric_integral() {
        prime();
        let curve = &CURVES.fold;
        let (a, b) = (-3.1, 7.4);
        let n = 20000;
        let h = (b - a) / n as f64;
        let numeric: f64 = (0..n)
            .map(|k| curve.eval(a + (k as f64 + 0.5) * h) * h)
            .sum();
        let exact = curve.antiderivative(b) - curve.antiderivative(a);
        assert!((numeric - exact).abs() < 1e-6, "{numeric} vs {exact}");
    }
}
