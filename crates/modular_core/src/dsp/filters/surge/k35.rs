//! `$unstable.filter.k35Lp` / `$unstable.filter.k35Hp` — Surge XT's K35 filters
//! (`fut_k35_lp`/`fut_k35_hp`), the Korg 35 two-pole sallen-key design from Odin 2.
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::coeffs::note_to_hz;
use super::fastmath::{fasttan, fasttanh_clamped};
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

/// K35 coefficient slots (Surge `k35_coeffs`).
const K35_G: usize = 0;
const K35_LB: usize = 1;
const K35_HB: usize = 2;
const K35_K: usize = 3;
const K35_ALPHA: usize = 4;
const K35_SATURATION: usize = 5;
const K35_SAT_BLEND: usize = 6;
const K35_SAT_BLEND_INV: usize = 7;

/// K35 state registers (Surge `k35_state`).
const K35_LZ: usize = 0;
const K35_HZ: usize = 1;
const K35_2Z: usize = 2;

/// Saturation drive applied inside the feedback path. `none` bypasses the
/// saturator entirely (a fully linear filter).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum K35Saturation {
    /// Linear — no saturation.
    #[default]
    None,
    /// ×1 drive into the saturator.
    Mild,
    /// ×2 drive.
    Moderate,
    /// ×3 drive.
    Heavy,
    /// ×4 drive.
    Extreme,
}

impl K35Saturation {
    /// Surge `fut_k35_saturations[subtype]`.
    #[inline]
    fn amount(self) -> f32 {
        match self {
            K35Saturation::None => 0.0,
            K35Saturation::Mild => 1.0,
            K35Saturation::Moderate => 2.0,
            K35Saturation::Heavy => 3.0,
            K35Saturation::Extreme => 4.0,
        }
    }
}

/// Surge `K35Filter::doLpf`: one ZDF one-pole lowpass step.
#[inline]
fn do_lpf(g: f32, input: f32, z: &mut f32) -> f32 {
    let v = (input - *z) * g;
    let result = v + *z;
    *z = v + result;
    result
}

/// Surge `K35Filter::doHpf`: the complementary highpass tap.
#[inline]
fn do_hpf(g: f32, input: f32, z: &mut f32) -> f32 {
    input - do_lpf(g, input, z)
}

/// Surge `K35Filter::makeCoefficients`: bilinear-prewarped one-pole gain, resonance
/// mapped to the sallen-key feedback `k` (0.01..1.96), and the passband-specific
/// feedback betas.
fn coeffs_k35(
    freq_semi: f32,
    reso: f32,
    rate: f32,
    is_lowpass: bool,
    saturation: f32,
) -> [f32; N_COEFFS] {
    let freq = note_to_hz(freq_semi).clamp(5.0, rate * 0.3);
    let wd = freq * 2.0 * std::f32::consts::PI;
    let wa = (2.0 * rate) * fasttan(wd / rate * 0.5);
    let g = wa / rate * 0.5;
    let gp1 = 1.0 + g;
    let big_g = g / gp1;

    let mk = (reso * 1.96).clamp(0.01, 1.96);

    let mut c = [0.0f32; N_COEFFS];
    c[K35_G] = big_g;
    if is_lowpass {
        c[K35_LB] = (mk - mk * big_g) / gp1;
        c[K35_HB] = -1.0 / gp1;
    } else {
        c[K35_LB] = 1.0 / gp1;
        c[K35_HB] = -big_g / gp1;
    }
    c[K35_K] = mk;
    c[K35_ALPHA] = 1.0 / (1.0 - mk * big_g + mk * big_g * big_g);
    c[K35_SATURATION] = saturation;
    c[K35_SAT_BLEND] = saturation.min(1.0);
    c[K35_SAT_BLEND_INV] = 1.0 - c[K35_SAT_BLEND];
    c
}

/// K35 lowpass kernel.
#[derive(Clone, Copy, Default)]
pub struct K35Lp;

impl Filter for K35Lp {
    type Mode = K35Saturation;
    type Extra = ();

    fn coeffs(mode: K35Saturation, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        coeffs_k35(freq_semi, reso, rate, true, mode.amount())
    }

    fn process(
        _mode: K35Saturation,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        for i in 0..N_COEFFS {
            c[i] += dc[i];
        }

        let y1 = do_lpf(c[K35_G], x, &mut r[K35_LZ]);
        let s35 = c[K35_LB] * r[K35_2Z] + c[K35_HB] * r[K35_HZ];
        let u_clean = c[K35_ALPHA] * (y1 + s35);
        let u_driven = fasttanh_clamped(u_clean * c[K35_SATURATION]);
        let u = u_clean * c[K35_SAT_BLEND_INV] + u_driven * c[K35_SAT_BLEND];

        let y = c[K35_K] * do_lpf(c[K35_G], u, &mut r[K35_2Z]);
        do_hpf(c[K35_G], y, &mut r[K35_HZ]);

        y / c[K35_K]
    }

    fn oversample(mode: K35Saturation) -> bool {
        mode != K35Saturation::None
    }
}

/// K35 highpass kernel.
#[derive(Clone, Copy, Default)]
pub struct K35Hp;

impl Filter for K35Hp {
    type Mode = K35Saturation;
    type Extra = ();

    fn coeffs(mode: K35Saturation, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        coeffs_k35(freq_semi, reso, rate, false, mode.amount())
    }

    fn process(
        _mode: K35Saturation,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        for i in 0..N_COEFFS {
            c[i] += dc[i];
        }

        let y1 = do_hpf(c[K35_G], x, &mut r[K35_HZ]);
        let s35 = c[K35_HB] * r[K35_2Z] + c[K35_LB] * r[K35_LZ];
        let u = c[K35_ALPHA] * (y1 + s35);

        let y_clean = c[K35_K] * u;
        let y_driven = fasttanh_clamped(y_clean * c[K35_SATURATION]);
        let y = y_clean * c[K35_SAT_BLEND_INV] + y_driven * c[K35_SAT_BLEND];

        let hp = do_hpf(c[K35_G], y, &mut r[K35_2Z]);
        do_lpf(c[K35_G], hp, &mut r[K35_LZ]);

        y / c[K35_K]
    }

    fn oversample(mode: K35Saturation) -> bool {
        mode != K35Saturation::None
    }
}

filter_module! {
    /// Surge XT's K35 lowpass — the Korg 35 sallen-key filter (from Odin 2), with a
    /// saturator in the resonance feedback path. Oversampled 2× when saturating.
    ///
    /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; screams near the top.
    /// - **saturation** — `'none'`, `'mild'`, `'moderate'`, `'heavy'`, `'extreme'`.
    ///
    /// ```js
    /// $unstable.filter.k35Lp($saw('c2'), 'c4', 3, { saturation: 'moderate' })
    /// ```
    name = "$unstable.filter.k35Lp", ident = K35LpFilter, kernel = K35Lp,
    output_doc = "lowpass output",
    params = {
        /// feedback saturation: none, mild, moderate, heavy, extreme (default none)
        saturation: K35Saturation,
    },
    mode = |p| p.saturation,
}

filter_module! {
    /// Surge XT's K35 highpass — the Korg 35 sallen-key filter (from Odin 2), with a
    /// saturator in the resonance feedback path. Oversampled 2× when saturating.
    ///
    /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; screams near the top.
    /// - **saturation** — `'none'`, `'mild'`, `'moderate'`, `'heavy'`, `'extreme'`.
    ///
    /// ```js
    /// $unstable.filter.k35Hp($saw('c2'), 'c4', 3, { saturation: 'mild' })
    /// ```
    name = "$unstable.filter.k35Hp", ident = K35HpFilter, kernel = K35Hp,
    output_doc = "highpass output",
    params = {
        /// feedback saturation: none, mild, moderate, heavy, extreme (default none)
        saturation: K35Saturation,
    },
    mode = |p| p.saturation,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    const ALL_SATURATIONS: [K35Saturation; 5] = [
        K35Saturation::None,
        K35Saturation::Mild,
        K35Saturation::Moderate,
        K35Saturation::Heavy,
        K35Saturation::Extreme,
    ];

    #[test]
    fn k35_lp_attenuates_highs_all_saturations() {
        let sr = 96_000.0;
        // 1 kHz cutoff expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        for sat in ALL_SATURATIONS {
            let low = sine_rms::<K35Lp>(sat, cutoff_semi, 0.0, 200.0, sr);
            let high = sine_rms::<K35Lp>(sat, cutoff_semi, 0.0, 8000.0, sr);
            assert!(
                high < low * 0.5,
                "expected lowpass attenuation ({sat:?}): low={low} high={high}"
            );
        }
    }

    #[test]
    fn k35_hp_attenuates_lows_all_saturations() {
        let sr = 96_000.0;
        // 1 kHz cutoff expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        for sat in ALL_SATURATIONS {
            let low = sine_rms::<K35Hp>(sat, cutoff_semi, 0.0, 125.0, sr);
            let high = sine_rms::<K35Hp>(sat, cutoff_semi, 0.0, 8000.0, sr);
            assert!(
                low < high * 0.5,
                "expected highpass attenuation ({sat:?}): low={low} high={high}"
            );
        }
    }

    #[test]
    fn k35_lp_survives_resonant_cutoff_sweep() {
        sweep_stays_bounded::<K35Lp>(&ALL_SATURATIONS);
    }

    #[test]
    fn k35_hp_survives_resonant_cutoff_sweep() {
        sweep_stays_bounded::<K35Hp>(&ALL_SATURATIONS);
    }
}
