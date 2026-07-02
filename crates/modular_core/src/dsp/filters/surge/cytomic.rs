//! `$unstable.filter.fastSvf` — Surge XT's Fast SVF (`fut_cytomic_svf`), Andy Simper's
//! (Cytomic) trapezoidal state-variable filter. Fully linear; the mode selects the
//! output mix `m0·in + m1·v1 + m2·v2`.
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::coeffs::note_to_hz;
use super::fastmath::fasttan;
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

/// Cytomic coefficient slots (Surge `cytomic_quadform::Coeff`).
const CY_A1: usize = 0;
const CY_A2: usize = 1;
const CY_A3: usize = 2;
const CY_M0: usize = 3;
const CY_M1: usize = 4;
const CY_M2: usize = 5;
const N_CY_COEFF: usize = 6;

/// Output mix (the six modes the product's subtype menu exposes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum FastSvfMode {
    /// Lowpass.
    #[default]
    Lp,
    /// Highpass.
    Hp,
    /// Bandpass.
    Bp,
    /// Notch.
    Notch,
    /// Resonant peak.
    Peak,
    /// Allpass.
    Allpass,
}

/// Fast SVF kernel — linear, so it runs at the base rate.
#[derive(Clone, Copy, Default)]
pub struct FastSvf;

impl Filter for FastSvf {
    type Mode = FastSvfMode;
    type Extra = ();

    /// Surge `cytomic_quadform::makeCoefficients`: trapezoidal-integration gains from
    /// the prewarped cutoff (stable right up to Nyquist via the 0.499 clamp) and the
    /// per-mode output mix.
    fn coeffs(mode: FastSvfMode, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        let conorm = (note_to_hz(freq_semi) / rate).clamp(0.0, 0.499);
        let reso = reso.clamp(0.0, 0.99);

        let g = fasttan(std::f32::consts::PI * conorm);
        let k = 2.0 - 2.0 * reso;

        let gk = g + k;
        let mut c = [0.0f32; N_COEFFS];
        c[CY_A1] = 1.0 / (1.0 + g * gk);
        c[CY_A2] = g * c[CY_A1];
        c[CY_A3] = g * c[CY_A2];

        let (m0, m1, m2) = match mode {
            FastSvfMode::Lp => (0.0, 0.0, 1.0),
            FastSvfMode::Bp => (0.0, 1.0, 0.0),
            FastSvfMode::Hp => (1.0, -k, -1.0),
            FastSvfMode::Notch => (1.0, -k, 0.0),
            FastSvfMode::Peak => (1.0, -k, -2.0),
            FastSvfMode::Allpass => (1.0, -2.0 * k, 0.0),
        };
        c[CY_M0] = m0;
        c[CY_M1] = m1;
        c[CY_M2] = m2;
        c
    }

    fn process(
        _mode: FastSvfMode,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        for i in 0..N_CY_COEFF {
            c[i] += dc[i];
        }

        let v3 = x - r[1];
        let v1 = c[CY_A1] * r[0] + c[CY_A2] * v3;
        let v2 = r[1] + c[CY_A2] * r[0] + c[CY_A3] * v3;
        r[0] = 2.0 * v1 - r[0];
        r[1] = 2.0 * v2 - r[1];

        c[CY_M0] * x + c[CY_M1] * v1 + c[CY_M2] * v2
    }
}

filter_module! {
    /// Surge XT's Fast SVF — Andy Simper's (Cytomic) trapezoidal state-variable filter.
    /// Clean and linear, stable with cutoff modulation right up to Nyquist.
    ///
    /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5.
    /// - **mode** — `'lp'`, `'hp'`, `'bp'`, `'notch'`, `'peak'`, or `'allpass'`.
    ///
    /// ```js
    /// $unstable.filter.fastSvf($saw('c2'), 'c4', 3, { mode: 'peak' })
    /// ```
    name = "$unstable.filter.fastSvf", ident = FastSvfFilter, kernel = FastSvf,
    output_doc = "filter output",
    params = {
        /// output mix: lp, hp, bp, notch, peak, or allpass (default lp)
        mode: FastSvfMode,
    },
    mode = |p| p.mode,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    const SR: f32 = 48_000.0;

    const ALL_MODES: [FastSvfMode; 6] = [
        FastSvfMode::Lp,
        FastSvfMode::Hp,
        FastSvfMode::Bp,
        FastSvfMode::Notch,
        FastSvfMode::Peak,
        FastSvfMode::Allpass,
    ];

    fn rms(mode: FastSvfMode, freq: f32) -> f32 {
        // 1 kHz cutoff expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        sine_rms::<FastSvf>(mode, cutoff_semi, 0.2, freq, SR)
    }

    #[test]
    fn mode_shapes() {
        assert!(
            rms(FastSvfMode::Lp, 8000.0) < rms(FastSvfMode::Lp, 200.0) * 0.5,
            "lp"
        );
        assert!(
            rms(FastSvfMode::Hp, 125.0) < rms(FastSvfMode::Hp, 8000.0) * 0.5,
            "hp"
        );
        let bp_c = rms(FastSvfMode::Bp, 1000.0);
        assert!(
            rms(FastSvfMode::Bp, 60.0) < bp_c * 0.5 && rms(FastSvfMode::Bp, 12_000.0) < bp_c * 0.5,
            "bp"
        );
        let n_c = rms(FastSvfMode::Notch, 1000.0);
        assert!(
            n_c < rms(FastSvfMode::Notch, 100.0) * 0.5
                && n_c < rms(FastSvfMode::Notch, 10_000.0) * 0.5,
            "notch"
        );
        // Peak boosts the center relative to the skirts.
        let p_c = rms(FastSvfMode::Peak, 1000.0);
        assert!(p_c > rms(FastSvfMode::Peak, 100.0), "peak");
        // Allpass is flat.
        let unit = std::f32::consts::FRAC_1_SQRT_2;
        for freq in [200.0, 8000.0] {
            let a = rms(FastSvfMode::Allpass, freq);
            assert!((a - unit).abs() < 0.05, "allpass flat at {freq}: {a}");
        }
    }

    #[test]
    fn all_modes_survive_resonant_cutoff_sweep() {
        sweep_stays_bounded::<FastSvf>(&ALL_MODES);
    }
}
