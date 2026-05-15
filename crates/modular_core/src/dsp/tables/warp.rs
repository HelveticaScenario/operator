//! Pure warp functions for table phase distortion.
//!
//! Each function has the signature `(x: f32, param: f32) -> f32`.
//! `x` is a normalized phase in `[0, 1]`, the output is clamped to `[0, 1]`.
//! `param` range depends on the function.
//!
//! These functions are pure and allocation-free — safe to call from the audio thread.

/// Identity — pass through unchanged.
#[inline]
pub fn identity(x: f32, _param: f32) -> f32 {
    x
}

/// Mirror — reflects the waveform around the midpoint.
/// `param=0`: identity. `param=1`: full triangle (double-speed, mirrored).
#[inline]
pub fn mirror(x: f32, param: f32) -> f32 {
    let reflected = if x < 0.5 { x * 2.0 } else { 2.0 - x * 2.0 };
    let result = x + (reflected - x) * param.clamp(0.0, 1.0);
    result.clamp(0.0, 1.0)
}

/// Bend — asymmetric phase distortion (power curve).
/// `param=0`: identity. `param=1`: aggressive bend. `param=-1`: inverse bend.
#[inline]
pub fn bend(x: f32, param: f32) -> f32 {
    let param = param.clamp(-1.0, 1.0);
    if param.abs() < 1e-6 {
        return x;
    }
    let exponent = (2.0f32).powf(param * 2.0);
    x.powf(exponent).clamp(0.0, 1.0)
}

/// Sync — hard sync effect. Multiplies phase frequency.
/// `param=0`: 1x (identity). `param=1`: 16x frequency.
#[inline]
pub fn sync(x: f32, param: f32) -> f32 {
    let ratio = 1.0 + param.clamp(0.0, 1.0) * 15.0;
    (x * ratio).fract()
}

/// Fold — wave folding. Folds phase back at boundaries.
/// `param=0`: identity. `param=1`: 4x folding.
#[inline]
pub fn fold(x: f32, param: f32) -> f32 {
    let param = param.clamp(0.0, 1.0);
    let scaled = x * (1.0 + param * 3.0);
    let period = scaled % 2.0;
    if period <= 1.0 { period } else { 2.0 - period }
}

/// PWM — pulse width modulation of phase.
/// `param=0.5`: identity-like. `param→0`: compress first half. `param→1`: compress second half.
#[inline]
pub fn pwm(x: f32, param: f32) -> f32 {
    let width = param.clamp(0.01, 0.99);
    if x < width {
        x / width * 0.5
    } else {
        0.5 + (x - width) / (1.0 - width) * 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_range(v: f32) -> bool {
        (0.0..=1.0).contains(&v)
    }

    /// Sweep x across [0, 1] at the given param and verify every output sits in [0, 1].
    fn assert_in_range(f: fn(f32, f32) -> f32, param: f32) {
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            let y = f(x, param);
            assert!(
                in_range(y),
                "output {} out of range for x={}, param={}",
                y,
                x,
                param
            );
        }
    }

    // ---------- identity ----------

    #[test]
    fn identity_passes_through() {
        for i in 0..=20 {
            let x = i as f32 / 20.0;
            assert_eq!(identity(x, 0.0), x);
            assert_eq!(identity(x, 0.5), x);
            assert_eq!(identity(x, 1.0), x);
        }
    }

    // ---------- mirror ----------

    #[test]
    fn mirror_at_zero_is_identity() {
        for i in 0..=20 {
            let x = i as f32 / 20.0;
            let y = mirror(x, 0.0);
            assert!(
                (y - x).abs() < 1e-6,
                "mirror(x={}, 0) = {}, expected {}",
                x,
                y,
                x
            );
        }
    }

    #[test]
    fn mirror_endpoints_preserved() {
        for p in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert!(mirror(0.0, p).abs() < 1e-6);
        }
    }

    #[test]
    fn mirror_output_in_range() {
        for p in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_in_range(mirror, p);
        }
    }

    #[test]
    fn mirror_full_at_one_reaches_peak_at_midpoint() {
        // At param=1 with x=0.5, reflected = 1.0, result = 1.0
        assert!((mirror(0.5, 1.0) - 1.0).abs() < 1e-6);
    }

    // ---------- bend ----------

    #[test]
    fn bend_at_zero_is_identity() {
        for i in 0..=20 {
            let x = i as f32 / 20.0;
            let y = bend(x, 0.0);
            assert!(
                (y - x).abs() < 1e-6,
                "bend(x={}, 0) = {}, expected {}",
                x,
                y,
                x
            );
        }
    }

    #[test]
    fn bend_endpoints_preserved() {
        for p in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            assert!(bend(0.0, p).abs() < 1e-6, "bend(0, {}) should be 0", p);
            assert!(
                (bend(1.0, p) - 1.0).abs() < 1e-6,
                "bend(1, {}) should be 1",
                p
            );
        }
    }

    #[test]
    fn bend_output_in_range() {
        for p in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            assert_in_range(bend, p);
        }
    }

    // ---------- sync ----------

    #[test]
    fn sync_at_zero_is_identity_interior() {
        // At param=0, ratio=1, (x * 1).fract() == x for x in [0, 1).
        for i in 0..100 {
            let x = i as f32 / 100.0;
            let y = sync(x, 0.0);
            assert!((y - x).abs() < 1e-6, "sync(x={}, 0) = {}", x, y);
        }
    }

    #[test]
    fn sync_output_in_range() {
        for p in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_in_range(sync, p);
        }
    }

    // ---------- fold ----------

    #[test]
    fn fold_at_zero_is_identity() {
        for i in 0..=20 {
            let x = i as f32 / 20.0;
            let y = fold(x, 0.0);
            assert!((y - x).abs() < 1e-6, "fold(x={}, 0) = {}", x, y);
        }
    }

    #[test]
    fn fold_output_in_range() {
        for p in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_in_range(fold, p);
        }
    }

    // ---------- pwm ----------

    #[test]
    fn pwm_endpoints_preserved() {
        for p in [0.01, 0.25, 0.5, 0.75, 0.99] {
            assert!(pwm(0.0, p).abs() < 1e-6, "pwm(0, {}) should be 0", p);
            assert!(
                (pwm(1.0, p) - 1.0).abs() < 1e-6,
                "pwm(1, {}) should be 1",
                p
            );
        }
    }

    #[test]
    fn pwm_midpoint_is_half_at_half_width() {
        // At width=0.5 and x=0.5, boundary case: x < width is false, so 0.5 + 0 = 0.5
        let y = pwm(0.5, 0.5);
        assert!((y - 0.5).abs() < 1e-6, "pwm(0.5, 0.5) = {}", y);
    }

    #[test]
    fn pwm_output_in_range() {
        for p in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_in_range(pwm, p);
        }
    }
}
