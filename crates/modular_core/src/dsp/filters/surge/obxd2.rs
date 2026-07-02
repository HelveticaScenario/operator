//! `$unstable.filter.obxd2{Lp,Bp,Hp,Notch}` — Surge XT's OB-Xd 2-pole multimode
//! (`fut_obxd_2pole_*`), a zero-delay state-variable filter with a diode-pair
//! nonlinearity in the resonance feedback, from Filatov's OB-Xd (via Odin 2).
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::coeffs::note_to_hz;
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

/// OB-Xd 2-pole coefficient slots (Surge `Obxd12dBCoeff`).
const G12: usize = 0;
const R12: usize = 1;
const MULTIMODE: usize = 2;
const BANDPASS: usize = 3;
const SELF_OSC_PUSH: usize = 4;
const N_OBXD12_COEFF: usize = 5;

/// Drive character: `pushed` boosts the feedback nonlinearity into easy
/// self-oscillation (Surge XT's "Pushed" subtypes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum ObxdDrive {
    /// Standard feedback bias.
    #[default]
    Standard,
    /// Self-oscillation push — hotter feedback bias.
    Pushed,
}

/// Surge XT's 2-pole `sub` index: passband 0–3 (lp/bp/hp/notch), +4 when pushed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obxd2Mode {
    pub sub: i32,
}

/// Cutoff in radians-per-sample-times-π (Surge XT's unit for the OB-Xd `tan` warp),
/// capped at 22 kHz.
#[inline]
pub(super) fn obxd_cutoff(freq_semi: f32, rate: f32) -> f32 {
    note_to_hz(freq_semi).min(22_000.0) / rate * std::f32::consts::PI
}

/// Surge `diodePairResistanceApprox`: Taylor approximation of a slightly mismatched
/// diode pair, the feedback nonlinearity.
#[inline]
fn diode_pair_resistance_approx(x: f32) -> f32 {
    ((((0.010_359_2 * x) + 0.009_208_33) * x + 0.185) * x + 0.05) * x + 1.0
}

/// OB-Xd 2-pole kernel. The diode-pair feedback runs per sample → 2× oversampled.
#[derive(Clone, Copy, Default)]
pub struct Obxd2Pole;

impl Filter for Obxd2Pole {
    type Mode = Obxd2Mode;
    type Extra = ();

    /// Surge `OBXDFilter::makeCoefficients` (TWO_POLE): `multimode` positions the
    /// output mix along lp→bp/notch→hp; `bandpass` switches the mix formula.
    fn coeffs(mode: Obxd2Mode, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        let mut c = [0.0f32; N_COEFFS];
        c[G12] = obxd_cutoff(freq_semi, rate).tan();
        c[R12] = 1.0 - reso;
        c[BANDPASS] = 0.0;
        match mode.sub & 3 {
            0 => c[MULTIMODE] = 0.0, // lowpass
            1 => {
                c[MULTIMODE] = 0.5; // bandpass
                c[BANDPASS] = 1.0;
            }
            2 => c[MULTIMODE] = 1.0, // highpass
            _ => c[MULTIMODE] = 0.5, // notch
        }
        c[SELF_OSC_PUSH] = if mode.sub > 3 { 1.0 } else { 0.0 };
        c
    }

    fn process(
        _mode: Obxd2Mode,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        for i in 0..N_OBXD12_COEFF {
            c[i] += dc[i];
        }

        // Surge `NewtonRaphson12dB`: resolve the zero-delay feedback with the
        // diode-pair transconductance folded into R.
        let bias = if c[SELF_OSC_PUSH] == 1.0 { 1.035 } else { 1.0 };
        let tcfb = diode_pair_resistance_approx(r[0] * 0.0876) - bias;
        let g = c[G12];
        let rr = c[R12] + tcfb;
        let v = (x - 2.0 * r[0] * rr - g * r[0] - r[1]) / (1.0 + g * (2.0 * rr + g));

        let y1 = v * g + r[0];
        r[0] = v * g + y1;
        let y2 = y1 * g + r[1];
        r[1] = y1 * g + y2;

        let mm = c[MULTIMODE];
        let mc = if c[BANDPASS] == 0.0 {
            (1.0 - mm) * y2 + mm * v
        } else if mm < 0.5 {
            (0.5 - mm) * y2 + mm * y1
        } else {
            (1.0 - mm) * y1 + (mm - 0.5) * v
        };

        mc * 0.74
    }

    fn oversample(_mode: Obxd2Mode) -> bool {
        true
    }
}

/// Stamp one 2-pole passband module (they differ only in name, docs, and the
/// passband offset into Surge XT's `sub` index).
macro_rules! obxd2_module {
    ($name:literal, $Struct:ident, $offset:literal, $passband:literal, $example:literal) => {
        filter_module! {
            /// Surge XT's OB-Xd 2-pole filter — a zero-delay state-variable design with
            /// a diode-pair feedback nonlinearity (from Filatov's OB-Xd, via Odin 2).
            /// Runs 2× oversampled.
            ///
            /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
            /// - **resonance** — 0–5.
            /// - **drive** — `'standard'`, or `'pushed'` for easy self-oscillation.
            ///
            /// ```js
            #[doc = $example]
            /// ```
            name = $name, ident = $Struct, kernel = Obxd2Pole,
            output_doc = $passband,
            params = {
                /// drive: standard or pushed (self-oscillation push; default standard)
                drive: ObxdDrive,
            },
            mode = |p| Obxd2Mode {
                sub: $offset + if matches!(p.drive, ObxdDrive::Pushed) { 4 } else { 0 },
            },
        }
    };
}

obxd2_module!(
    "$unstable.filter.obxd2Lp",
    Obxd2LpFilter,
    0,
    "lowpass output",
    "$unstable.filter.obxd2Lp($saw('c2'), 'c4', 3, { drive: 'pushed' })"
);
obxd2_module!(
    "$unstable.filter.obxd2Bp",
    Obxd2BpFilter,
    1,
    "bandpass output",
    "$unstable.filter.obxd2Bp($saw('c2'), 'c4', 3)"
);
obxd2_module!(
    "$unstable.filter.obxd2Hp",
    Obxd2HpFilter,
    2,
    "highpass output",
    "$unstable.filter.obxd2Hp($saw('c2'), 'c4', 3)"
);
obxd2_module!(
    "$unstable.filter.obxd2Notch",
    Obxd2NotchFilter,
    3,
    "notch output",
    "$unstable.filter.obxd2Notch($saw('c2'), 'c4', 3)"
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    const SR: f32 = 96_000.0; // kernel run rate is 2× the 48 kHz engine rate

    fn cutoff_semi() -> f32 {
        // 1 kHz expressed in semitones above A440.
        12.0 * (1000.0f32 / 440.0).log2()
    }

    fn rms(sub: i32, freq: f32) -> f32 {
        sine_rms::<Obxd2Pole>(Obxd2Mode { sub }, cutoff_semi(), 0.2, freq, SR)
    }

    #[test]
    fn passband_shapes_standard_and_pushed() {
        for push in [0, 4] {
            // Lowpass: highs drop.
            assert!(
                rms(push, 8000.0) < rms(push, 200.0) * 0.5,
                "lp (push {push})"
            );
            // Bandpass: skirts drop.
            let bp_c = rms(1 + push, 1000.0);
            assert!(
                rms(1 + push, 60.0) < bp_c * 0.5 && rms(1 + push, 12_000.0) < bp_c * 0.5,
                "bp (push {push})"
            );
            // Highpass: lows drop.
            assert!(
                rms(2 + push, 125.0) < rms(2 + push, 8000.0) * 0.5,
                "hp (push {push})"
            );
            // Notch: center drops below both skirts.
            let n_c = rms(3 + push, 1000.0);
            assert!(
                n_c < rms(3 + push, 100.0) * 0.5 && n_c < rms(3 + push, 10_000.0) * 0.5,
                "notch (push {push})"
            );
        }
    }

    #[test]
    fn all_subs_survive_resonant_cutoff_sweep() {
        let modes: Vec<Obxd2Mode> = (0..8).map(|sub| Obxd2Mode { sub }).collect();
        sweep_stays_bounded::<Obxd2Pole>(&modes);
    }
}
