//! `$unstable.filter.bp` — Surge XT's multimode bandpass biquads (BP 12/24 dB).
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::biquad::{
    BiquadMode, FilterSlope, iir12_b, iir12_cfc, iir24_b, iir24_cfc, svf_bp12, svf_bp24,
};
use super::coeffs::{
    ST_BP_LEGACY_CLEAN, ST_BP_LEGACY_DRIVEN, ST_CLEAN, ST_DRIVEN, ST_STANDARD, bound_freq,
    clipscale, coeff_svf, map_2pole_resonance, map_4pole_resonance, note_to_omega, resoscale,
    to_coupled_form, to_normalized_lattice,
};
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

/// Drive character for the bandpass biquad. The legacy variants exist only at the
/// 12 dB slope (matching Surge XT's menu); at 24 dB they select their modern counterparts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum BpDrive {
    /// State-variable form (cleanest, mildest resonance).
    #[default]
    Standard,
    /// Coupled state-space form with a heavier resonance/clip character.
    Driven,
    /// Normalized-lattice form (high numerical precision).
    Clean,
    /// Surge XT's "Driven (Legacy)" — the darker pre-fix driven bandpass.
    DrivenLegacy,
    /// Surge XT's "Clean (Legacy)" — the darker pre-fix clean bandpass.
    CleanLegacy,
}

impl BpDrive {
    #[inline]
    pub fn subtype(self, four_pole: bool) -> i32 {
        match (self, four_pole) {
            (BpDrive::Standard, _) => ST_STANDARD,
            (BpDrive::Driven, _) | (BpDrive::DrivenLegacy, true) => ST_DRIVEN,
            (BpDrive::Clean, _) | (BpDrive::CleanLegacy, true) => ST_CLEAN,
            (BpDrive::DrivenLegacy, false) => ST_BP_LEGACY_DRIVEN,
            (BpDrive::CleanLegacy, false) => ST_BP_LEGACY_CLEAN,
        }
    }
}

/// Bandpass biquad kernel.
#[derive(Clone, Copy, Default)]
pub struct BpBiquad;

impl Filter for BpBiquad {
    type Mode = BiquadMode;
    type Extra = ();

    fn coeffs(mode: BiquadMode, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        match mode.subtype {
            // Both slopes build the 2-pole SVF (Surge passes `fourPole = false` for
            // BP24 as well) — the 24 dB character comes from the cascaded evaluator.
            ST_STANDARD => coeff_svf(freq_semi, reso, false, rate),
            ST_BP_LEGACY_DRIVEN => coeff_bp_legacy(freq_semi, reso, ST_DRIVEN, rate),
            ST_BP_LEGACY_CLEAN => coeff_bp_legacy(freq_semi, reso, ST_CLEAN, rate),
            st if mode.four_pole => coeff_bp24(freq_semi, reso, st, rate),
            st => coeff_bp12(freq_semi, reso, st, rate),
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
            (ST_STANDARD, false) => svf_bp12(x, c, dc, r),
            (ST_STANDARD, true) => svf_bp24(x, c, dc, r),
            (ST_CLEAN | ST_BP_LEGACY_CLEAN, false) => iir12_b(x, c, dc, r),
            (ST_CLEAN, true) => iir24_b(x, c, dc, r),
            (_, false) => iir12_cfc(x, c, dc, r),
            (_, true) => iir24_cfc(x, c, dc, r),
        }
    }
}

/// Surge `Coeff_BP12`: constant-peak bandpass numerator (`b0 = Q·α, b1 = 0,
/// b2 = −Q·α`) → coupled/lattice form. Driven doubles the numerator gain.
fn coeff_bp12(freq_semi: f32, reso: f32, subtype: i32, rate: f32) -> [f32; N_COEFFS] {
    let mut gain = resoscale(reso, subtype) as f64;
    if subtype == ST_DRIVEN {
        gain *= 2.0;
    }
    let freq = bound_freq(freq_semi);
    let (sinu, cosi) = note_to_omega(freq, rate);
    let (sinu, cosi) = (sinu as f64, cosi as f64);

    let q2inv = map_2pole_resonance(reso as f64, freq as f64, subtype);
    let q = 0.5 / q2inv;
    let alpha = sinu * q2inv;
    let alpha = alpha.min((1.0 - cosi * cosi).sqrt() - 0.0001);

    let a0 = 1.0 + alpha;
    let a0inv = 1.0 / a0;
    let a1 = -2.0 * cosi;
    let a2 = 1.0 - alpha;
    let b0 = q * alpha;
    let b1 = 0.0;
    let b2 = -q * alpha;

    let g = clipscale(freq, subtype);
    if subtype == ST_CLEAN {
        to_normalized_lattice(a0inv, a1, a2, b0 * gain, b1 * gain, b2 * gain, g)
    } else {
        to_coupled_form(a0inv, a1, a2, b0 * gain, b1 * gain, b2 * gain, g)
    }
}

/// Surge `Coeff_BP24`: identical to `Coeff_BP12` but with the 4-pole resonance map.
fn coeff_bp24(freq_semi: f32, reso: f32, subtype: i32, rate: f32) -> [f32; N_COEFFS] {
    let mut gain = resoscale(reso, subtype) as f64;
    if subtype == ST_DRIVEN {
        gain *= 2.0;
    }
    let freq = bound_freq(freq_semi);
    let (sinu, cosi) = note_to_omega(freq, rate);
    let (sinu, cosi) = (sinu as f64, cosi as f64);

    let q2inv = map_4pole_resonance(reso as f64, freq as f64, subtype);
    let q = 0.5 / q2inv;
    let alpha = sinu * q2inv;
    let alpha = alpha.min((1.0 - cosi * cosi).sqrt() - 0.0001);

    let a0 = 1.0 + alpha;
    let a0inv = 1.0 / a0;
    let a1 = -2.0 * cosi;
    let a2 = 1.0 - alpha;
    let b0 = q * alpha;
    let b1 = 0.0;
    let b2 = -q * alpha;

    let g = clipscale(freq, subtype);
    if subtype == ST_CLEAN {
        to_normalized_lattice(a0inv, a1, a2, b0 * gain, b1 * gain, b2 * gain, g)
    } else {
        to_coupled_form(a0inv, a1, a2, b0 * gain, b1 * gain, b2 * gain, g)
    }
}

/// The legacy 12 dB subtypes: Surge builds `Coeff_BP12` then `Coeff_BP24`
/// back-to-back, and each pass runs the `FromDirect` target smoother, so the packed
/// target settles at the `(4·BP12 + 5·BP24)/9` fixed point. Since the glide already
/// interpolates in the packed-coefficient domain, that blend is the exact steady-state
/// target.
fn coeff_bp_legacy(freq_semi: f32, reso: f32, subtype: i32, rate: f32) -> [f32; N_COEFFS] {
    let n12 = coeff_bp12(freq_semi, reso, subtype, rate);
    let n24 = coeff_bp24(freq_semi, reso, subtype, rate);
    std::array::from_fn(|i| (4.0 * n12[i] + 5.0 * n24[i]) / 9.0)
}

filter_module! {
    /// Surge multimode bandpass (the "12 dB" / "24 dB" vember filter).
    ///
    /// - **cutoff** — center frequency in V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; higher narrows the band and pushes toward ringing.
    /// - **slope** — `'db12'` or `'db24'`.
    /// - **drive** — `'standard'`, `'driven'`, `'clean'`, or the 12 dB-only
    ///   `'drivenLegacy'` / `'cleanLegacy'`.
    ///
    /// ```js
    /// $unstable.filter.bp($saw('c2'), 'c4', 3, { slope: 'db12', drive: 'drivenLegacy' })
    /// ```
    name = "$unstable.filter.bp", ident = BpFilter, kernel = BpBiquad,
    output_doc = "bandpass output",
    params = {
        /// slope: 12 or 24 dB/oct (default 24)
        slope: FilterSlope,
        /// drive character (default standard); legacy variants apply at 12 dB only
        drive: BpDrive,
    },
    mode = |p| {
        let four_pole = p.slope.four_pole();
        BiquadMode {
            four_pole,
            subtype: p.drive.subtype(four_pole),
        }
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    fn all_modes() -> Vec<BiquadMode> {
        let mut v = Vec::new();
        for slope in [FilterSlope::Db12, FilterSlope::Db24] {
            for drive in [
                BpDrive::Standard,
                BpDrive::Driven,
                BpDrive::Clean,
                BpDrive::DrivenLegacy,
                BpDrive::CleanLegacy,
            ] {
                let four_pole = slope.four_pole();
                v.push(BiquadMode {
                    four_pole,
                    subtype: drive.subtype(four_pole),
                });
            }
        }
        v.dedup();
        v
    }

    #[test]
    fn bandpass_passes_center_attenuates_skirts_all_subtypes() {
        let sr = 48_000.0;
        // 1 kHz center expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        for mode in all_modes() {
            let center = sine_rms::<BpBiquad>(mode, cutoff_semi, 0.0, 1000.0, sr);
            let low = sine_rms::<BpBiquad>(mode, cutoff_semi, 0.0, 60.0, sr);
            let high = sine_rms::<BpBiquad>(mode, cutoff_semi, 0.0, 12_000.0, sr);
            assert!(
                low < center * 0.5 && high < center * 0.5,
                "expected bandpass shape (mode {mode:?}): low={low} center={center} high={high}"
            );
        }
    }

    #[test]
    fn all_modes_survive_resonant_cutoff_sweep() {
        sweep_stays_bounded::<BpBiquad>(&all_modes());
    }
}
