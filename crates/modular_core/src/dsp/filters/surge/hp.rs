//! `$unstable.filter.hp` — Surge XT's multimode highpass biquads (HP 12/24 dB).
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use super::biquad::{
    BiquadDrive, BiquadMode, FilterSlope, iir12_b, iir12_cfc, iir24_b, iir24_cfc, svf_hp12,
    svf_hp24,
};
use super::coeffs::{
    ST_CLEAN, ST_STANDARD, bound_freq, clipscale, coeff_svf, map_2pole_resonance,
    map_4pole_resonance, note_to_omega, resoscale, to_coupled_form, to_normalized_lattice,
};
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

/// Highpass biquad kernel.
#[derive(Clone, Copy, Default)]
pub struct HpBiquad;

impl Filter for HpBiquad {
    type Mode = BiquadMode;
    type Extra = ();

    fn coeffs(mode: BiquadMode, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        if mode.subtype == ST_STANDARD {
            coeff_svf(freq_semi, reso, mode.four_pole, rate)
        } else if mode.four_pole {
            coeff_hp24(freq_semi, reso, mode.subtype, rate)
        } else {
            coeff_hp12(freq_semi, reso, mode.subtype, rate)
        }
    }

    fn process(
        mode: BiquadMode,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        match (mode.subtype, mode.four_pole) {
            (ST_STANDARD, false) => svf_hp12(x, c, dc, r),
            (ST_STANDARD, true) => svf_hp24(x, c, dc, r),
            (ST_CLEAN, false) => iir12_b(x, c, dc, r),
            (ST_CLEAN, true) => iir24_b(x, c, dc, r),
            (_, false) => iir12_cfc(x, c, dc, r),
            (_, true) => iir24_cfc(x, c, dc, r),
        }
    }
}

/// Surge `Coeff_HP12`: the highpass biquad numerator (`b0 = (1+cos)/2, b1 = -(1+cos),
/// b2 = (1+cos)/2`) → coupled/lattice form. Both non-SVF subtypes clamp alpha for
/// stability (LP exempts Clean).
fn coeff_hp12(freq_semi: f32, reso: f32, subtype: i32, rate: f32) -> [f32; N_COEFFS] {
    let gain = resoscale(reso, subtype) as f64;
    let freq = bound_freq(freq_semi);
    let (sinu, cosi) = note_to_omega(freq, rate);
    let (sinu, cosi) = (sinu as f64, cosi as f64);

    let alpha = sinu * map_2pole_resonance(reso as f64, freq as f64, subtype);
    let alpha = alpha.min((1.0 - cosi * cosi).sqrt() - 0.0001);

    let a0 = 1.0 + alpha;
    let a0inv = 1.0 / a0;
    let a1 = -2.0 * cosi;
    let a2 = 1.0 - alpha;
    let b0 = (1.0 + cosi) * 0.5;
    let b1 = -(1.0 + cosi);
    let b2 = (1.0 + cosi) * 0.5;

    let g = clipscale(freq, subtype);
    if subtype == ST_CLEAN {
        to_normalized_lattice(a0inv, a1, a2, b0 * gain, b1 * gain, b2 * gain, g)
    } else {
        to_coupled_form(a0inv, a1, a2, b0 * gain, b1 * gain, b2 * gain, g)
    }
}

/// Surge `Coeff_HP24`: identical to `Coeff_HP12` but with the 4-pole resonance map.
fn coeff_hp24(freq_semi: f32, reso: f32, subtype: i32, rate: f32) -> [f32; N_COEFFS] {
    let gain = resoscale(reso, subtype) as f64;
    let freq = bound_freq(freq_semi);
    let (sinu, cosi) = note_to_omega(freq, rate);
    let (sinu, cosi) = (sinu as f64, cosi as f64);

    let alpha = sinu * map_4pole_resonance(reso as f64, freq as f64, subtype);
    let alpha = alpha.min((1.0 - cosi * cosi).sqrt() - 0.0001);

    let a0 = 1.0 + alpha;
    let a0inv = 1.0 / a0;
    let a1 = -2.0 * cosi;
    let a2 = 1.0 - alpha;
    let b0 = (1.0 + cosi) * 0.5;
    let b1 = -(1.0 + cosi);
    let b2 = (1.0 + cosi) * 0.5;

    let g = clipscale(freq, subtype);
    if subtype == ST_CLEAN {
        to_normalized_lattice(a0inv, a1, a2, b0 * gain, b1 * gain, b2 * gain, g)
    } else {
        to_coupled_form(a0inv, a1, a2, b0 * gain, b1 * gain, b2 * gain, g)
    }
}

filter_module! {
    /// Surge multimode highpass (the "12 dB" / "24 dB" vember filter).
    ///
    /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; near the top the filter rings and self-oscillates.
    /// - **slope** — `'db12'` or `'db24'`.
    /// - **drive** — `'standard'`, `'driven'`, or `'clean'`.
    ///
    /// ```js
    /// $unstable.filter.hp($saw('c2'), 'c4', 2, { slope: 'db12', drive: 'clean' })
    /// ```
    name = "$unstable.filter.hp", ident = HpFilter, kernel = HpBiquad,
    output_doc = "highpass output",
    params = {
        /// slope: 12 or 24 dB/oct (default 24)
        slope: FilterSlope,
        /// drive character: standard, driven, or clean (default standard)
        drive: BiquadDrive,
    },
    mode = |p| BiquadMode {
        four_pole: p.slope.four_pole(),
        subtype: p.drive.subtype(),
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::coeffs::ST_DRIVEN;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    fn all_modes() -> Vec<BiquadMode> {
        let mut v = Vec::new();
        for four_pole in [false, true] {
            for subtype in [ST_STANDARD, ST_DRIVEN, ST_CLEAN] {
                v.push(BiquadMode { four_pole, subtype });
            }
        }
        v
    }

    #[test]
    fn highpass_attenuates_lows_all_subtypes() {
        let sr = 48_000.0;
        // 1 kHz cutoff expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        for mode in all_modes() {
            let low = sine_rms::<HpBiquad>(mode, cutoff_semi, 0.0, 125.0, sr);
            let high = sine_rms::<HpBiquad>(mode, cutoff_semi, 0.0, 8000.0, sr);
            assert!(
                low < high * 0.5,
                "expected highpass attenuation (mode {mode:?}): low={low} high={high}"
            );
        }
    }

    #[test]
    fn all_modes_survive_resonant_cutoff_sweep() {
        sweep_stays_bounded::<HpBiquad>(&all_modes());
    }
}
