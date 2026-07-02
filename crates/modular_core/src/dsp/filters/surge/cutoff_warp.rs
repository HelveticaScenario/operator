//! `$unstable.filter.cutoffWarp{Lp,Hp,Bp,Notch,Ap}` — Surge XT's Cutoff Warp
//! (`fut_cutoffwarp_*`): an RBJ biquad with a saturator in the *feedback* path
//! (Jatin Chowdhury's nonlinear-feedback design), cascadable up to four stages.
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::coeffs::note_to_hz;
use super::fastmath::{fastcos, fastsin, fasttanh_clamped, softclip};
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};
use crate::dsp::shape::shapers::ojd;

/// Biquad passband shared by the warp families.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WarpPassband {
    Lp,
    Hp,
    Notch,
    Bp,
    Ap,
}

/// Cutoff clamp shared by the warp families.
#[inline]
pub(super) fn warp_clamped_frequency(freq_semi: f32, rate: f32) -> f32 {
    note_to_hz(freq_semi).clamp(5.0, rate * 0.3)
}

/// Feedback saturator. Rendered as `'tanh'` / `'softClip'` / `'ojd'` in the DSL.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum WarpDrive {
    /// Smooth tanh feedback saturation.
    #[default]
    Tanh,
    /// Cubic soft clip.
    SoftClip,
    /// OJD overdrive — asymmetric knee.
    Ojd,
}

/// Cutoff-warp coefficient slots (Surge `nlf_coeffs`).
const NLF_A1: usize = 0;
const NLF_A2: usize = 1;
const NLF_B0: usize = 2;
const NLF_B1: usize = 3;
const NLF_B2: usize = 4;
const NLF_MAKEUP: usize = 5;
const N_NLF_COEFF: usize = 6;

/// Passband + Surge subtype (`(stages−1) | saturator<<2`, 0–11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutoffWarpMode {
    pub passband: WarpPassband,
    pub subtype: i32,
}

/// Surge `CutoffWarp::doNLFilter`: one biquad step with the saturator applied to the
/// feedback signal only.
#[inline]
#[allow(clippy::too_many_arguments)]
fn do_nl_filter(
    input: f32,
    a1: f32,
    a2: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    makeup: f32,
    sat: i32,
    z1: &mut f32,
    z2: &mut f32,
) -> f32 {
    let out = *z1 + b0 * input;
    let nf = match sat {
        1 => softclip(out),
        2 => ojd(out, 1.0),
        _ => fasttanh_clamped(out),
    };
    *z1 = *z2 + b1 * input - a1 * nf;
    *z2 = b2 * input - a2 * nf;
    out * makeup
}

/// Cutoff-warp kernel. Every subtype saturates per sample → 2× oversampled.
#[derive(Clone, Copy, Default)]
pub struct CutoffWarp;

impl Filter for CutoffWarp {
    type Mode = CutoffWarpMode;
    type Extra = ();

    /// Surge `CutoffWarp::makeCoefficients`: RBJ coefficients with `q = 18·reso³+0.1`
    /// and ear-tuned makeup gain (per-subtype table for LP/HP, extra resonance makeup
    /// for the OJD subtypes).
    fn coeffs(mode: CutoffWarpMode, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        /// Surge XT's RMS-measured, hand-tweaked LP/HP normalization per subtype.
        const LP_NORM_TABLE: [f32; 12] = [
            1.53273, 1.33407, 1.08197, 0.958219, 1.27374, 0.932342, 0.761765, 0.665462, 0.776856,
            0.597575, 0.496207, 0.471714,
        ];

        let reso = reso.clamp(0.0, 1.0);
        let q = (reso * reso * reso) * 18.0 + 0.1;

        let normalised_freq = 2.0 * warp_clamped_frequency(freq_semi, rate) / rate;
        let wc = std::f32::consts::PI * normalised_freq;

        let wsin = fastsin(wc);
        let wcos = fastcos(wc);
        let alpha = wsin / (2.0 * q);
        let a0r = 1.0 / (1.0 + alpha);

        let mut c = [0.0f32; N_COEFFS];
        c[NLF_A1] = -2.0 * wcos * a0r;
        c[NLF_A2] = (1.0 - alpha) * a0r;
        c[NLF_MAKEUP] = 1.0;

        let exp_min = if matches!(mode.passband, WarpPassband::Lp) {
            0.1
        } else {
            0.35
        };
        let res_makeup = if mode.subtype < 8 {
            1.0
        } else {
            1.0 / reso.max(exp_min).sqrt()
        };

        match mode.passband {
            WarpPassband::Lp => {
                c[NLF_B1] = (1.0 - wcos) * a0r;
                c[NLF_B0] = c[NLF_B1] * 0.5;
                c[NLF_B2] = c[NLF_B0];
                c[NLF_MAKEUP] = res_makeup * LP_NORM_TABLE[mode.subtype as usize]
                    / normalised_freq.max(0.001).powf(0.1);
            }
            WarpPassband::Hp => {
                c[NLF_B1] = -(1.0 + wcos) * a0r;
                c[NLF_B0] = c[NLF_B1] * -0.5;
                c[NLF_B2] = c[NLF_B0];
                c[NLF_MAKEUP] = res_makeup * LP_NORM_TABLE[mode.subtype as usize]
                    / (1.0 - normalised_freq).max(0.001).powf(0.1);
            }
            WarpPassband::Notch => {
                c[NLF_B0] = a0r;
                c[NLF_B1] = -2.0 * wcos * a0r;
                c[NLF_B2] = c[NLF_B0];
            }
            WarpPassband::Bp => {
                c[NLF_B0] = wsin * 0.5 * a0r;
                c[NLF_B1] = 0.0;
                c[NLF_B2] = -c[NLF_B0];
            }
            WarpPassband::Ap => {
                c[NLF_B0] = c[NLF_A2];
                c[NLF_B1] = c[NLF_A1];
                c[NLF_B2] = 1.0;
            }
        }
        c
    }

    fn process(
        mode: CutoffWarpMode,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        let stages = (mode.subtype & 3) as usize;
        let sat = (mode.subtype >> 2) & 3;

        let mut out = x;
        for stage in 0..=stages {
            let mut z1 = r[stage * 2];
            let mut z2 = r[stage * 2 + 1];
            out = do_nl_filter(
                out,
                c[NLF_A1],
                c[NLF_A2],
                c[NLF_B0],
                c[NLF_B1],
                c[NLF_B2],
                c[NLF_MAKEUP],
                sat,
                &mut z1,
                &mut z2,
            );
            r[stage * 2] = z1;
            r[stage * 2 + 1] = z2;
        }

        for i in 0..N_NLF_COEFF {
            c[i] += dc[i];
        }
        out
    }

    fn oversample(_mode: CutoffWarpMode) -> bool {
        true
    }
}

/// Stamp one cutoff-warp passband module.
macro_rules! cutoff_warp_module {
    ($name:literal, $Struct:ident, $passband:expr, $output_doc:literal, $example:literal) => {
        filter_module! {
            /// Surge XT's Cutoff Warp filter — an RBJ biquad with a saturator in the
            /// feedback path (a nonlinear-feedback design by Jatin Chowdhury), up to
            /// four cascaded stages. Runs 2× oversampled.
            ///
            /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
            /// - **resonance** — 0–5 (biquad Q up to ~18).
            /// - **drive** — `'tanh'`, `'softClip'`, or `'ojd'`.
            /// - **stages** — 1–4 cascaded sections.
            ///
            /// ```js
            #[doc = $example]
            /// ```
            name = $name, ident = $Struct, kernel = CutoffWarp,
            output_doc = $output_doc,
            params = {
                /// feedback saturator: tanh, softClip, or ojd (default tanh)
                drive: WarpDrive,
                /// number of cascaded filter stages, 1–4 (default 1)
                stages: Option<usize>,
            },
            mode = |p| CutoffWarpMode {
                passband: $passband,
                subtype: (p.stages.unwrap_or(1).clamp(1, 4) as i32 - 1) | ((p.drive as i32) << 2),
            },
        }
    };
}

cutoff_warp_module!(
    "$unstable.filter.cutoffWarpLp",
    CutoffWarpLpFilter,
    WarpPassband::Lp,
    "lowpass output",
    "$unstable.filter.cutoffWarpLp($saw('c2'), 'c4', 2, { drive: 'ojd', stages: 2 })"
);
cutoff_warp_module!(
    "$unstable.filter.cutoffWarpHp",
    CutoffWarpHpFilter,
    WarpPassband::Hp,
    "highpass output",
    "$unstable.filter.cutoffWarpHp($saw('c2'), 'c4', 2, { stages: 2 })"
);
cutoff_warp_module!(
    "$unstable.filter.cutoffWarpBp",
    CutoffWarpBpFilter,
    WarpPassband::Bp,
    "bandpass output",
    "$unstable.filter.cutoffWarpBp($saw('c2'), 'c4', 2)"
);
cutoff_warp_module!(
    "$unstable.filter.cutoffWarpNotch",
    CutoffWarpNotchFilter,
    WarpPassband::Notch,
    "notch output",
    "$unstable.filter.cutoffWarpNotch($saw('c2'), 'c4', 2)"
);
cutoff_warp_module!(
    "$unstable.filter.cutoffWarpAp",
    CutoffWarpApFilter,
    WarpPassband::Ap,
    "allpass output",
    "$unstable.filter.cutoffWarpAp($saw('c2'), 'c4', 2, { drive: 'softClip' })"
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    const SR: f32 = 96_000.0; // kernel run rate is 2× the 48 kHz engine rate

    fn all_modes(passband: WarpPassband) -> Vec<CutoffWarpMode> {
        (0..12)
            .map(|subtype| CutoffWarpMode { passband, subtype })
            .collect()
    }

    fn rms(mode: CutoffWarpMode, freq: f32) -> f32 {
        // 1 kHz cutoff expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        sine_rms::<CutoffWarp>(mode, cutoff_semi, 0.2, freq, SR)
    }

    #[test]
    fn passband_shapes_all_subtypes() {
        for subtype in 0..12 {
            let lp = |f| {
                rms(
                    CutoffWarpMode {
                        passband: WarpPassband::Lp,
                        subtype,
                    },
                    f,
                )
            };
            assert!(lp(8000.0) < lp(200.0) * 0.5, "lp shape (subtype {subtype})");
            let hp = |f| {
                rms(
                    CutoffWarpMode {
                        passband: WarpPassband::Hp,
                        subtype,
                    },
                    f,
                )
            };
            assert!(hp(125.0) < hp(8000.0) * 0.5, "hp shape (subtype {subtype})");
            let bp = |f| {
                rms(
                    CutoffWarpMode {
                        passband: WarpPassband::Bp,
                        subtype,
                    },
                    f,
                )
            };
            let bp_c = bp(1000.0);
            assert!(
                bp(60.0) < bp_c * 0.5 && bp(12_000.0) < bp_c * 0.5,
                "bp shape (subtype {subtype})"
            );
            let n = |f| {
                rms(
                    CutoffWarpMode {
                        passband: WarpPassband::Notch,
                        subtype,
                    },
                    f,
                )
            };
            assert!(
                n(1000.0) < n(100.0) * 0.5 && n(1000.0) < n(10_000.0) * 0.5,
                "notch shape (subtype {subtype})"
            );
        }
    }

    #[test]
    fn all_passbands_survive_resonant_cutoff_sweep() {
        for passband in [
            WarpPassband::Lp,
            WarpPassband::Hp,
            WarpPassband::Bp,
            WarpPassband::Notch,
            WarpPassband::Ap,
        ] {
            sweep_stays_bounded::<CutoffWarp>(&all_modes(passband));
        }
    }
}
