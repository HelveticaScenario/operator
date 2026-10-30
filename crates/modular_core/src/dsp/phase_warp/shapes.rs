//! Pure phase-shaping functions behind the [`PhaseWarp`](super::PhaseWarp) variants.
//!
//! The geometric warps have the signature `(x: f32, param: f32) -> f32`: `x` is a
//! normalized phase in `[0, 1]`, the output is clamped to `[0, 1]`, and `param`'s range
//! depends on the function. The Ensemble Oscillator twists (pulsar, crush, feedback) take
//! a 0–5 knob value and the oscillator frequency for antialiasing.
//!
//! Twists adapted from the 4ms Ensemble Oscillator. Copyright 4ms Company. Used under
//! GPL v3.
//!
//! These functions are allocation-free — safe to call from the audio thread.

use crate::dsp::utils::{sanitize, sin_turns};

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

/// Normalize a 0–5 knob value to `[0, 1]`.
#[inline]
pub fn knob(amount: f32) -> f32 {
    (amount / 5.0).clamp(0.0, 1.0)
}

/// Shared rolloff of the feedback and pulsar antialiasing: `max(1 − 2f, 0)²`,
/// with `freq_norm` the oscillator frequency over the sample rate. `freq_norm = 0`
/// (unknown pitch) applies no rolloff.
#[inline]
fn twist_rolloff(freq_norm: f32) -> f32 {
    let base = (1.0 - 2.0 * freq_norm).max(0.0);
    base * base
}

// The twists take their knob prepared once per channel by the matching
// `*_amount` function, leaving only the per-oscillator antialiasing here.

/// Pulsar speed-up for a 0–5 knob: `2^(6·k²)`, 1× to 64×.
#[inline]
pub fn pulsar_amount(knob_value: f32) -> f32 {
    let k = knob(knob_value);
    f32::exp2(k * k * 6.0)
}

/// Pulsar — plays each cycle `multiplier` times faster and rests at phase 0
/// for the remainder.
#[inline]
pub fn pulsar(x: f32, multiplier: f32, freq_norm: f32) -> f32 {
    // Ensemble Oscillator antialiasing: (m − 1)·max(1 − 2f, 0)¹⁶ + 1.
    let rolloff = twist_rolloff(freq_norm);
    let rolloff = rolloff * rolloff;
    let rolloff = rolloff * rolloff;
    let multiplier = (multiplier - 1.0) * rolloff * rolloff + 1.0;
    let y = x * multiplier;
    // The reference saturates at 1.0 and then wraps, which lands on 0.
    if y < 1.0 { y } else { 0.0 }
}

/// Crush depth for a 0–5 knob: `0.5·k²`.
#[inline]
pub fn crush_amount(knob_value: f32) -> f32 {
    let k = knob(knob_value);
    k * k * 0.5
}

/// Crush — XORs the 32-bit phase with scaled copies of itself, a glitchy
/// bit-level distortion. Not antialiased.
#[inline]
pub fn crush(x: f32, depth: f32) -> f32 {
    let am = (depth * 65535.0) as u32;
    let phase = (x.clamp(0.0, 1.0) * 4_294_967_295.0) as u32;
    let mut y = phase;
    y ^= (phase >> 16).wrapping_mul(am);
    // 0.25 in u0.16 fixed point.
    y ^= 16384u32.wrapping_mul(am);
    y as f32 / 4_294_967_295.0
}

/// Feedback depth for a 0–5 knob: `0.7·k²`.
#[inline]
pub fn feedback_amount(knob_value: f32) -> f32 {
    let k = knob(knob_value);
    k * k * 0.7
}

/// Feedback — offsets the phase by a one-pole-lowpassed sine of its own output,
/// scaled by `depth`, plus `depth` as a constant offset. `lp` is the lowpass
/// state, carried from sample to sample.
#[inline]
pub fn feedback(x: f32, depth: f32, freq_norm: f32, lp: &mut f32) -> f32 {
    let depth = depth * twist_rolloff(freq_norm);
    let y = x + *lp * depth + depth;
    let y = y - y.floor();
    // One-pole lowpass with coefficient 1/4.
    *lp = sanitize(*lp + (sin_turns(y) - *lp) * 0.25);
    y
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
