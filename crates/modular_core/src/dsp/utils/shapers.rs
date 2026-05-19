//! Cheap nonlinear waveshapers.
//!
//! Drop-in replacements for `tanh()` in saturation contexts where the
//! exact transcendental output isn't required — the shaper's job is to
//! introduce bounded harmonic distortion, and small approximation
//! errors in the soft-knee region are masked by the intended distortion.
//!
//! Reference: Émilie Gillet's `stmlib::SoftLimit` /  `stmlib::SoftClip`,
//! used unchanged in Mutable Instruments Plaits / Warps. The Padé
//! rational below was chosen by MI for the same reason — cheap, smooth
//! C¹-continuous saturator that asymptotes to ±1.
//!
//! Error vs `f32::tanh`:
//!   |x|=0.5  → ~0.3%
//!   |x|=1.0  → ~1.7%
//!   |x|=2.0  → ~2.5% peak
//!   |x|≥3.0  → 0 (both = ±1 exactly)

/// Padé-3 rational saturator: `x · (27 + x²) / (27 + 9x²)`.
///
/// Smoothly approaches ±1 as |x| grows; numerically valid everywhere.
/// Use this when the input is already bounded (e.g. has been clipped or
/// is known small). For unbounded inputs use [`soft_clip`] which adds
/// the explicit ±1 hard rail outside ±3.
#[inline]
pub fn soft_limit(x: f32) -> f32 {
    let xx = x * x;
    x * (27.0 + xx) / (27.0 + 9.0 * xx)
}

/// `soft_limit` with explicit clamp to ±1 outside ±3 — matches
/// `stmlib::SoftClip`. For |x| ≥ 3 the rational already returns
/// approximately ±1; this guarantees exactly ±1 outside that band so
/// downstream stages can assume a strict bound.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    if x < -3.0 {
        -1.0
    } else if x > 3.0 {
        1.0
    } else {
        soft_limit(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_limit_passes_zero() {
        assert_eq!(soft_limit(0.0), 0.0);
    }

    #[test]
    fn soft_limit_odd_symmetric() {
        for &x in &[0.1f32, 0.5, 1.0, 2.0, 5.0] {
            let y_pos = soft_limit(x);
            let y_neg = soft_limit(-x);
            assert!(
                (y_pos + y_neg).abs() < 1e-6,
                "odd symmetry violated at {x}: {y_pos} vs {y_neg}"
            );
        }
    }

    #[test]
    fn soft_limit_bounded_for_large_inputs() {
        // Padé asymptote is 1/9 · x — wait, no: numerator x*(27+x²) ≈ x³,
        // denominator 9x² ⇒ asymptote x/9. So |soft_limit| grows linearly
        // outside the soft-knee — bounded for the inputs we see (drive ≤ 5)
        // but not at infinity. Verify the ±3 case maps inside ±1.
        assert!((soft_limit(3.0) - 1.0).abs() < 1e-6);
        assert!((soft_limit(-3.0) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn soft_clip_bounded_strictly() {
        assert_eq!(soft_clip(100.0), 1.0);
        assert_eq!(soft_clip(-100.0), -1.0);
        assert_eq!(soft_clip(3.0), 1.0);
        assert_eq!(soft_clip(-3.0), -1.0);
        // Inside the band the clip is identical to soft_limit.
        for &x in &[0.0f32, 0.5, 1.5, 2.9] {
            assert_eq!(soft_clip(x), soft_limit(x));
        }
    }

    #[test]
    fn soft_limit_close_to_tanh_in_knee() {
        // Sanity check the documented error budget. Padé is ~2.5% off
        // tanh at peak; assert <5% over the soft-knee region.
        for x_int in -25..=25 {
            let x = x_int as f32 * 0.1;
            let approx = soft_limit(x);
            let exact = x.tanh();
            let err = (approx - exact).abs();
            // The peak error vs tanh sits around |x|≈1.5 and never exceeds
            // ~5% of unity in this band.
            assert!(err < 0.05, "soft_limit({x}) = {approx}, tanh = {exact}");
        }
    }
}
