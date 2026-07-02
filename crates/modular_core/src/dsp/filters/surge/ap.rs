//! `$unstable.filter.ap` — Surge XT's allpass filter.
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use super::biquad::iir12_b;
use super::coeffs::{bound_freq, note_to_omega, to_normalized_lattice};
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

/// Allpass kernel — a single configuration, so `Mode` is `()`.
#[derive(Clone, Copy, Default)]
pub struct ApBiquad;

impl Filter for ApBiquad {
    type Mode = ();
    type Extra = ();

    fn coeffs(_mode: (), freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        coeff_apf(freq_semi, reso, rate)
    }

    fn process(
        _mode: (),
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        iir12_b(x, c, dc, r)
    }
}

/// Surge `Coeff_APF`: allpass numerator (`b0 = 1−α, b1 = −2cos, b2 = 1+α` — the
/// denominator mirrored) → lattice form with a fixed 0.005 clip gain. Magnitude is
/// flat; resonance sharpens the phase transition around the center.
fn coeff_apf(freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
    let freq = bound_freq(freq_semi);
    let (sinu, cosi) = note_to_omega(freq, rate);
    let (sinu, cosi) = (sinu as f64, cosi as f64);

    let reso = reso as f64;
    let warp = (1.0 - (1.0 - reso) * (1.0 - reso)).clamp(0.0, 1.0);
    let q2inv = 2.5 - 2.49 * warp;

    let alpha = sinu * q2inv;

    let a0 = 1.0 + alpha;
    let a0inv = 1.0 / a0;
    let a1 = -2.0 * cosi;
    let a2 = 1.0 - alpha;
    let b0 = 1.0 - alpha;
    let b1 = -2.0 * cosi;
    let b2 = 1.0 + alpha;

    to_normalized_lattice(a0inv, a1, a2, b0, b1, b2, 0.005)
}

filter_module! {
    /// Surge allpass filter — flat magnitude, frequency-dependent phase shift.
    /// Useful for phaser-style effects when mixed against the dry signal.
    ///
    /// - **cutoff** — phase-transition center in V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; sharpens the phase transition around the center.
    ///
    /// ```js
    /// $unstable.filter.ap($saw('c2'), 'c4', 2)
    /// ```
    name = "$unstable.filter.ap", ident = ApFilter, kernel = ApBiquad,
    output_doc = "allpass output",
    params = {},
    mode = |_p| (),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    /// The clip-damp register (`R = 1 − 0.005·y²`) pulls the poles slightly inward at
    /// unit drive, so the magnitude is flat only to within ~1 dB (worst below center).
    #[test]
    fn allpass_magnitude_is_near_flat() {
        let sr = 48_000.0;
        // 1 kHz center expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        let unit_sine_rms = std::f32::consts::FRAC_1_SQRT_2;
        for freq in [200.0, 1000.0, 8000.0] {
            let rms = sine_rms::<ApBiquad>((), cutoff_semi, 0.0, freq, sr);
            assert!(
                rms > unit_sine_rms * 0.89 && rms < unit_sine_rms * 1.12,
                "expected near-flat allpass magnitude at {freq} Hz: rms={rms}"
            );
        }
    }

    #[test]
    fn survives_resonant_cutoff_sweep() {
        sweep_stays_bounded::<ApBiquad>(&[()]);
    }
}
