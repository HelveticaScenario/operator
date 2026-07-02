//! `$unstable.filter.notch` — Surge XT's notch biquads (Notch 12/24 dB).
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::biquad::{FilterSlope, iir12_b, iir24_b};
use super::coeffs::{bound_freq, note_to_omega, to_normalized_lattice};
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

/// Notch width character.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum NotchStyle {
    /// Wide notch with a strong resonance response.
    #[default]
    Standard,
    /// Narrower, gentler notch.
    Mild,
}

/// Slope + width pair selecting one concrete notch configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotchMode {
    pub four_pole: bool,
    pub mild: bool,
}

/// Notch biquad kernel. Both subtypes run the normalized-lattice form; the 24 dB
/// slope cascades the same section twice.
#[derive(Clone, Copy, Default)]
pub struct NotchBiquad;

impl Filter for NotchBiquad {
    type Mode = NotchMode;
    type Extra = ();

    fn coeffs(mode: NotchMode, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        coeff_notch(freq_semi, reso, mode.mild, rate)
    }

    fn process(
        mode: NotchMode,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        if mode.four_pole {
            iir24_b(x, c, dc, r)
        } else {
            iir12_b(x, c, dc, r)
        }
    }
}

/// Surge `Coeff_Notch`: unit-gain notch numerator (`b0 = 1, b1 = -2cos, b2 = 1`)
/// → lattice form with a fixed 0.005 clip gain. Resonance widens the notch
/// (Standard) or narrows it (Mild).
fn coeff_notch(freq_semi: f32, reso: f32, mild: bool, rate: f32) -> [f32; N_COEFFS] {
    let freq = bound_freq(freq_semi);
    let (sinu, cosi) = note_to_omega(freq, rate);
    let (sinu, cosi) = (sinu as f64, cosi as f64);

    let reso = reso as f64;
    let warp = (1.0 - (1.0 - reso) * (1.0 - reso)).clamp(0.0, 1.0);
    let q2inv = if mild {
        1.00 - 0.99 * warp
    } else {
        2.5 - 2.49 * warp
    };

    let alpha = sinu * q2inv;

    let a0 = 1.0 + alpha;
    let a0inv = 1.0 / a0;
    let a1 = -2.0 * cosi;
    let a2 = 1.0 - alpha;
    let b0 = 1.0;
    let b1 = -2.0 * cosi;
    let b2 = 1.0;

    to_normalized_lattice(a0inv, a1, a2, b0, b1, b2, 0.005)
}

filter_module! {
    /// Surge notch filter (the "12 dB" / "24 dB" vember notch).
    ///
    /// - **cutoff** — notch center in V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; shapes the notch width per `style`.
    /// - **slope** — `'db12'` or `'db24'`.
    /// - **style** — `'standard'` (wide) or `'mild'` (gentle).
    ///
    /// ```js
    /// $unstable.filter.notch($saw('c2'), 'c4', 1, { slope: 'db12', style: 'mild' })
    /// ```
    name = "$unstable.filter.notch", ident = NotchFilter, kernel = NotchBiquad,
    output_doc = "notch output",
    params = {
        /// slope: 12 or 24 dB/oct (default 24)
        slope: FilterSlope,
        /// notch width: standard or mild (default standard)
        style: NotchStyle,
    },
    mode = |p| NotchMode {
        four_pole: p.slope.four_pole(),
        mild: matches!(p.style, NotchStyle::Mild),
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    fn all_modes() -> Vec<NotchMode> {
        let mut v = Vec::new();
        for four_pole in [false, true] {
            for mild in [false, true] {
                v.push(NotchMode { four_pole, mild });
            }
        }
        v
    }

    #[test]
    fn notch_nulls_center_passes_skirts_all_subtypes() {
        let sr = 48_000.0;
        // 1 kHz center expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        for mode in all_modes() {
            let center = sine_rms::<NotchBiquad>(mode, cutoff_semi, 0.5, 1000.0, sr);
            let low = sine_rms::<NotchBiquad>(mode, cutoff_semi, 0.5, 100.0, sr);
            let high = sine_rms::<NotchBiquad>(mode, cutoff_semi, 0.5, 10_000.0, sr);
            assert!(
                center < low * 0.3 && center < high * 0.3,
                "expected notch shape (mode {mode:?}): low={low} center={center} high={high}"
            );
        }
    }

    #[test]
    fn all_modes_survive_resonant_cutoff_sweep() {
        sweep_stays_bounded::<NotchBiquad>(&all_modes());
    }
}
