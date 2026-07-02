//! `$unstable.filter.obxd4` / `$unstable.filter.xpander` — Surge XT's OB-Xd 4-pole
//! lowpass (`fut_obxd_4pole`) and the Oberheim Xpander pole-mixing modes
//! (`fut_obxd_xpander`). One cascade of four zero-delay poles with an `atan`
//! damper on the first stage; the mode selects how the five taps are mixed.
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::coeffs::note_to_hz;
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};
use super::obxd2::obxd_cutoff;

/// OB-Xd 4-pole coefficient slots (Surge `Obxd24dBCoeff`, minus the morph mixes).
const G24: usize = 0;
const R24: usize = 1;
const RCOR24: usize = 2;
const RCOR24INV: usize = 3;
const N_OBXD24_COEFF: usize = 4;

/// Pole-mix evaluated on the cascade taps `y0..y4` (Surge `FourPoleMode`, minus the
/// continuous Morph, which the product's subtype menu does not expose).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PoleMix {
    Lp6,
    Lp12,
    Lp18,
    Lp24,
    /// Surge XT's "24 dB (Legacy)" — the `y3 + y4` mix.
    Lp24Broken,
    Hp1,
    Hp2,
    Hp3,
    Bp2,
    Bp4,
    N2,
    Ph3,
    Hp2Lp1,
    Hp3Lp1,
    N2Lp1,
    Ph3Lp1,
}

/// Surge `tptpc`: one zero-delay one-pole step.
#[inline]
fn tptpc(state: &mut f32, inp: f32, cutoff: f32) -> f32 {
    let v = (inp - *state) * cutoff / (1.0 + cutoff);
    let res = v + *state;
    *state = res + v;
    res
}

/// OB-Xd 4-pole kernel. The per-stage `atan` damper runs per sample → 2× oversampled.
#[derive(Clone, Copy, Default)]
pub struct Obxd4Pole;

impl Filter for Obxd4Pole {
    type Mode = PoleMix;
    type Extra = ();

    /// Surge `OBXDFilter::makeCoefficients` (FOUR_POLE/XPANDER): the `rcor24` damper
    /// scale tracks the rate so the `atan` knee lands at the same signal level.
    fn coeffs(_mode: PoleMix, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        let rcrate = (44_000.0 / rate).sqrt();
        let mut c = [0.0f32; N_COEFFS];
        c[G24] = obxd_cutoff(freq_semi, rate).tan();
        c[R24] = 3.5 * reso;
        c[RCOR24] = (970.0 / 44_000.0) * rcrate;
        c[RCOR24INV] = 1.0 / c[RCOR24];
        c
    }

    fn process(
        mode: PoleMix,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        for i in 0..N_OBXD24_COEFF {
            c[i] += dc[i];
        }

        let g = c[G24];
        let lpc = g / (1.0 + g);

        // Surge `NewtonRaphson24dB`: resolve the zero-delay resonance feedback.
        let ml = 1.0 / (1.0 + g);
        let s = (lpc * (lpc * (lpc * r[0] + r[1]) + r[2]) + r[3]) * ml;
        let g4 = lpc * lpc * lpc * lpc;
        let y0 = (x - c[R24] * s) / (1.0 + c[R24] * g4);

        // First pole, with the atan damper on its state.
        let v = (y0 - r[0]) * lpc;
        let res = v + r[0];
        r[0] = res + v;
        r[0] = (r[0] * c[RCOR24]).atan() * c[RCOR24INV];

        let y1 = res;
        let y2 = tptpc(&mut r[1], y1, g);
        let y3 = tptpc(&mut r[2], y2, g);
        let y4 = tptpc(&mut r[3], y3, g);

        // Xpander pole-mix weights on (y0, y1, y2, y3, y4).
        let mc = match mode {
            PoleMix::Lp6 => y1,
            PoleMix::Lp12 => y2,
            PoleMix::Lp18 => y3,
            PoleMix::Lp24 => y4,
            PoleMix::Lp24Broken => y3 + y4,
            PoleMix::Hp1 => y0 - y1,
            PoleMix::Hp2 => y0 - 2.0 * y1 + y2,
            PoleMix::Hp3 => y0 - 3.0 * y1 + 3.0 * y2 - y3,
            PoleMix::Bp2 => 2.0 * (y2 - y1),
            PoleMix::Bp4 => 2.0 * y2 - 4.0 * y3 + 2.0 * y4,
            PoleMix::N2 => y0 - 2.0 * y1 + 2.0 * y2,
            PoleMix::Ph3 => y0 - 3.0 * y1 + 6.0 * y2 - 4.0 * y3,
            PoleMix::Hp2Lp1 => -y1 + 2.0 * y2 - y3,
            PoleMix::Hp3Lp1 => -y1 + 3.0 * y2 - 3.0 * y3 + y4,
            PoleMix::N2Lp1 => -y1 + 2.0 * y2 - 2.0 * y3,
            PoleMix::Ph3Lp1 => -y1 + 3.0 * y2 - 6.0 * y3 + 4.0 * y4,
        };

        // Half-volume compensation scaled by resonance, then the family trim.
        mc * (1.0 + c[R24] * 0.45) * 0.6
    }

    fn oversample(_mode: PoleMix) -> bool {
        true
    }
}

/// 4-pole lowpass slope (Surge `fut_obxd_4p_subtypes`, minus Morph).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum Obxd4Slope {
    /// 6 dB/oct (1-pole tap).
    Db6,
    /// 12 dB/oct (2-pole tap).
    Db12,
    /// 18 dB/oct (3-pole tap).
    Db18,
    /// 24 dB/oct (full cascade).
    #[default]
    Db24,
    /// Surge XT's "24 dB (Legacy)" — the brighter `y3 + y4` mix.
    Db24Legacy,
}

impl Obxd4Slope {
    #[inline]
    fn pole_mix(self) -> PoleMix {
        match self {
            Obxd4Slope::Db6 => PoleMix::Lp6,
            Obxd4Slope::Db12 => PoleMix::Lp12,
            Obxd4Slope::Db18 => PoleMix::Lp18,
            Obxd4Slope::Db24 => PoleMix::Lp24,
            Obxd4Slope::Db24Legacy => PoleMix::Lp24Broken,
        }
    }
}

/// Xpander pole-mix selection (Surge `fut_obxd_xpander_subtypes`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum XpanderMode {
    /// 1-pole lowpass.
    Lp1,
    /// 2-pole lowpass.
    Lp2,
    /// 3-pole lowpass.
    Lp3,
    /// 4-pole lowpass.
    #[default]
    Lp4,
    /// 1-pole highpass.
    Hp1,
    /// 2-pole highpass.
    Hp2,
    /// 3-pole highpass.
    Hp3,
    /// 2-pole bandpass.
    Bp2,
    /// 4-pole bandpass.
    Bp4,
    /// 2-pole notch.
    N2,
    /// 3-stage phaser.
    Ph3,
    /// 2-pole highpass + 1-pole lowpass.
    Hp2Lp1,
    /// 3-pole highpass + 1-pole lowpass.
    Hp3Lp1,
    /// 2-pole notch + 1-pole lowpass.
    N2Lp1,
    /// 3-stage phaser + 1-pole lowpass.
    Ph3Lp1,
}

impl XpanderMode {
    #[inline]
    fn pole_mix(self) -> PoleMix {
        match self {
            XpanderMode::Lp1 => PoleMix::Lp6,
            XpanderMode::Lp2 => PoleMix::Lp12,
            XpanderMode::Lp3 => PoleMix::Lp18,
            XpanderMode::Lp4 => PoleMix::Lp24,
            XpanderMode::Hp1 => PoleMix::Hp1,
            XpanderMode::Hp2 => PoleMix::Hp2,
            XpanderMode::Hp3 => PoleMix::Hp3,
            XpanderMode::Bp2 => PoleMix::Bp2,
            XpanderMode::Bp4 => PoleMix::Bp4,
            XpanderMode::N2 => PoleMix::N2,
            XpanderMode::Ph3 => PoleMix::Ph3,
            XpanderMode::Hp2Lp1 => PoleMix::Hp2Lp1,
            XpanderMode::Hp3Lp1 => PoleMix::Hp3Lp1,
            XpanderMode::N2Lp1 => PoleMix::N2Lp1,
            XpanderMode::Ph3Lp1 => PoleMix::Ph3Lp1,
        }
    }
}

filter_module! {
    /// Surge XT's OB-Xd 4-pole lowpass — four cascaded zero-delay poles with an `atan`
    /// damper (from Filatov's OB-Xd, via Odin 2). Runs 2× oversampled.
    ///
    /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; self-oscillates near the top.
    /// - **slope** — `'db6'`, `'db12'`, `'db18'`, `'db24'`, or `'db24Legacy'`.
    ///
    /// ```js
    /// $unstable.filter.obxd4($saw('c2'), 'c4', 3, { slope: 'db24Legacy' })
    /// ```
    name = "$unstable.filter.obxd4", ident = Obxd4Filter, kernel = Obxd4Pole,
    output_doc = "lowpass output",
    params = {
        /// slope: 6, 12, 18, 24 dB/oct, or the brighter 24 dB legacy mix (default 24)
        slope: Obxd4Slope,
    },
    mode = |p| p.slope.pole_mix(),
}

filter_module! {
    /// The Oberheim Xpander pole-mixing filter (Surge XT's OB-Xd cascade with the
    /// Xpander mode matrix): one 4-pole lowpass core re-mixed into lowpass,
    /// highpass, bandpass, notch, and phaser responses. Runs 2× oversampled.
    ///
    /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5.
    /// - **mode** — `'lp1'`–`'lp4'`, `'hp1'`–`'hp3'`, `'bp2'`, `'bp4'`, `'n2'`,
    ///   `'ph3'`, `'hp2Lp1'`, `'hp3Lp1'`, `'n2Lp1'`, `'ph3Lp1'`.
    ///
    /// ```js
    /// $unstable.filter.xpander($saw('c2'), 'c4', 3, { mode: 'bp4' })
    /// ```
    name = "$unstable.filter.xpander", ident = XpanderFilter, kernel = Obxd4Pole,
    output_doc = "filter output",
    params = {
        /// pole-mix mode (default lp4)
        mode: XpanderMode,
    },
    mode = |p| p.mode.pole_mix(),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    const SR: f32 = 96_000.0; // kernel run rate is 2× the 48 kHz engine rate

    const ALL_MODES: [PoleMix; 16] = [
        PoleMix::Lp6,
        PoleMix::Lp12,
        PoleMix::Lp18,
        PoleMix::Lp24,
        PoleMix::Lp24Broken,
        PoleMix::Hp1,
        PoleMix::Hp2,
        PoleMix::Hp3,
        PoleMix::Bp2,
        PoleMix::Bp4,
        PoleMix::N2,
        PoleMix::Ph3,
        PoleMix::Hp2Lp1,
        PoleMix::Hp3Lp1,
        PoleMix::N2Lp1,
        PoleMix::Ph3Lp1,
    ];

    fn rms(mode: PoleMix, freq: f32) -> f32 {
        // 1 kHz cutoff expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        sine_rms::<Obxd4Pole>(mode, cutoff_semi, 0.2, freq, SR)
    }

    #[test]
    fn lowpass_taps_steepen() {
        let mut prev_high = f32::MAX;
        for mode in [PoleMix::Lp6, PoleMix::Lp12, PoleMix::Lp18, PoleMix::Lp24] {
            let low = rms(mode, 200.0);
            let high = rms(mode, 8000.0);
            assert!(
                high < low * 0.5,
                "lowpass shape ({mode:?}): low={low} high={high}"
            );
            assert!(high < prev_high, "tap steepening ({mode:?})");
            prev_high = high;
        }
    }

    #[test]
    fn xpander_mix_shapes() {
        // Highpasses attenuate lows.
        for mode in [PoleMix::Hp1, PoleMix::Hp2, PoleMix::Hp3] {
            assert!(
                rms(mode, 125.0) < rms(mode, 8000.0) * 0.5,
                "hp shape ({mode:?})"
            );
        }
        // Bandpasses attenuate both skirts.
        for mode in [PoleMix::Bp2, PoleMix::Bp4] {
            let center = rms(mode, 1000.0);
            assert!(
                rms(mode, 60.0) < center * 0.5 && rms(mode, 12_000.0) < center * 0.5,
                "bp shape ({mode:?})"
            );
        }
        // Notch attenuates the center against both skirts.
        let n_c = rms(PoleMix::N2, 1000.0);
        assert!(
            n_c < rms(PoleMix::N2, 100.0) * 0.5 && n_c < rms(PoleMix::N2, 10_000.0) * 0.5,
            "notch shape"
        );
    }

    #[test]
    fn all_modes_survive_resonant_cutoff_sweep() {
        sweep_stays_bounded::<Obxd4Pole>(&ALL_MODES);
    }
}
