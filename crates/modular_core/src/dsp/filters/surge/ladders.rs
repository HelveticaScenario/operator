//! `$unstable.filter.legacyLadder` / `$unstable.filter.diodeLadder` — Surge XT's ladder
//! lowpasses: the Legacy Ladder (`fut_lpmoog`, classic 4-pole transistor ladder) and
//! the Diode Ladder (`fut_diode`, a zero-delay-feedback EMS-style diode cascade
//! adapted from Odin 2).
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use super::coeffs::note_to_hz;
use super::fastmath::{fasttan, softclip8};
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

/// Ladder slope — which of the four cascaded pole outputs is tapped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum LadderSlope {
    /// 6 dB/oct (1-pole tap).
    Db6,
    /// 12 dB/oct (2-pole tap).
    Db12,
    /// 18 dB/oct (3-pole tap).
    Db18,
    /// 24 dB/oct (full 4-pole ladder).
    #[default]
    Db24,
}

impl LadderSlope {
    #[inline]
    fn tap(self) -> usize {
        match self {
            LadderSlope::Db6 => 0,
            LadderSlope::Db12 => 1,
            LadderSlope::Db18 => 2,
            LadderSlope::Db24 => 3,
        }
    }
}

/// Legacy ladder kernel (`LPMOOGquad`): four cascaded one-pole stages with a
/// `softclip8` saturator on the resonance-feedback input stage — nonlinear, so it
/// runs 2× oversampled.
#[derive(Clone, Copy, Default)]
pub struct LegacyLadder;

impl Filter for LegacyLadder {
    type Mode = LadderSlope;
    type Extra = ();

    /// Surge `Coeff_LP4L`: `gg` is the cutoff as a fraction of the run rate (clamped
    /// to 0.187, the ladder's stability ceiling — no `bound_freq` here), `t_b1` the
    /// one-pole coefficient, `q` the feedback amount capped by `0.5/t_b1⁴`.
    fn coeffs(_mode: LadderSlope, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        let gg = (note_to_hz(freq_semi) as f64 / rate as f64).clamp(0.0, 0.187);
        let t_b1 = 1.0 - (-2.0 * std::f64::consts::PI * gg).exp() as f32;
        let q = (2.15 * reso.clamp(0.0, 1.0)).min(0.5 / (t_b1 * t_b1 * t_b1 * t_b1));

        let mut c = [0.0f32; N_COEFFS];
        c[0] = 3.0 / (3.0 - q);
        c[1] = t_b1;
        c[2] = q;
        c
    }

    fn process(
        mode: LadderSlope,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        c[0] += dc[0];
        c[1] += dc[1];
        c[2] += dc[2];

        // `r[4]` is the previous stage-4 output: feedback taps the sum of the last two
        // ladder outputs, placing a zero at Nyquist in the feedback path.
        r[0] = softclip8(r[0] + c[1] * ((x * c[0] - c[2] * (r[3] + r[4])) - r[0]));
        r[1] += c[1] * (r[0] - r[1]);
        r[2] += c[1] * (r[1] - r[2]);
        r[4] = r[3];
        r[3] += c[1] * (r[2] - r[3]);

        r[mode.tap()]
    }

    fn oversample(_mode: LadderSlope) -> bool {
        true
    }
}

filter_module! {
    /// Surge XT's Legacy Ladder — the classic 4-pole transistor-ladder lowpass.
    /// Self-oscillates at high resonance; runs 2× oversampled.
    ///
    /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; past ~4 the ladder rings and self-oscillates.
    /// - **slope** — output tap: `'db6'`, `'db12'`, `'db18'`, or `'db24'`.
    ///
    /// ```js
    /// $unstable.filter.legacyLadder($saw('c2'), 'c4', 3, { slope: 'db24' })
    /// ```
    name = "$unstable.filter.legacyLadder", ident = LegacyLadderFilter, kernel = LegacyLadder,
    output_doc = "lowpass output",
    params = {
        /// slope: output tap at 6, 12, 18, or 24 dB/oct (default 24)
        slope: LadderSlope,
    },
    mode = |p| p.slope,
}

// ─── Diode ladder ──────────────────────────────────────────────────────────────

/// Diode-ladder coefficient slots (Surge `dlf_coeffs`). The ZDF solve needs more than
/// eight values, so the per-stage betas/gammas are recomputed each sample from these.
const DLF_ALPHA: usize = 0;
const DLF_GAMMA: usize = 1;
const DLF_G: usize = 2;
const DLF_G4: usize = 3;
const DLF_G3: usize = 4;
const DLF_G2: usize = 5;
const DLF_G1: usize = 6;
const DLF_KM: usize = 7;

/// Diode-ladder state registers (Surge `dlf_state`): the four one-pole `z⁻¹` states
/// and the per-stage feedback memories (stage 4's feedback is always zero).
const DLF_Z1: usize = 0;
const DLF_Z2: usize = 1;
const DLF_Z3: usize = 2;
const DLF_Z4: usize = 3;
const DLF_FEEDBACK3: usize = 4;
const DLF_FEEDBACK2: usize = 5;
const DLF_FEEDBACK1: usize = 6;

/// Surge `getFO`: a stage's feedback output, `(feedback · delta + z) · beta`.
#[inline]
fn get_fo(beta: f32, delta: f32, feedback: f32, z: f32) -> f32 {
    (feedback * delta + z) * beta
}

/// Surge `doLpf` (minus its unused `beta`/`delta` args): one ZDF one-pole stage.
#[inline]
fn do_lpf(
    input: f32,
    alpha: f32,
    gamma: f32,
    epsilon: f32,
    ma0: f32,
    feedback: f32,
    feedback_output: f32,
    z: &mut f32,
) -> f32 {
    let i = input * gamma + feedback + epsilon * feedback_output;
    let v = (ma0 * i - *z) * alpha;
    let result = v + *z;
    *z = v + result;
    result
}

/// Diode ladder kernel. The per-sample path is linear (the ZDF feedback is resolved
/// algebraically; `fasttan` appears only in the coefficient prewarp), so it runs at
/// the base rate.
#[derive(Clone, Copy, Default)]
pub struct DiodeLadder;

impl Filter for DiodeLadder {
    type Mode = LadderSlope;
    type Extra = ();

    /// Surge `DiodeLadderFilter::makeCoefficients`: cutoff clamped to
    /// `[5 Hz, 0.3·rate]`, bilinear prewarp via `fasttan`, cascaded one-pole gains
    /// `G1..G4`, and resonance mapped to the feedback amount `km` (0..16).
    fn coeffs(_mode: LadderSlope, freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        let freq = note_to_hz(freq_semi).clamp(5.0, rate * 0.3);
        let wd = freq * 2.0 * std::f32::consts::PI;
        let wa = (2.0 * rate) * fasttan(wd / rate * 0.5);
        let g = wa / rate * 0.5;

        let g4 = 0.5 * g / (1.0 + g);
        let g3 = 0.5 * g / (1.0 + g - 0.5 * g * g4);
        let g2 = 0.5 * g / (1.0 + g - 0.5 * g * g3);
        let g1 = g / (1.0 + g - g * g2);
        let m_gamma = g4 * g3 * g2 * g1;

        let big_g = g / (1.0 + g);
        let km = (reso * 16.0).clamp(0.0, 16.0);

        let mut c = [0.0f32; N_COEFFS];
        c[DLF_ALPHA] = big_g;
        c[DLF_GAMMA] = m_gamma;
        c[DLF_G] = g;
        c[DLF_G4] = g4;
        c[DLF_G3] = g3;
        c[DLF_G2] = g2;
        c[DLF_G1] = g1;
        c[DLF_KM] = km;
        c
    }

    fn process(
        mode: LadderSlope,
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        for i in 0..N_COEFFS {
            c[i] += dc[i];
        }

        let sg3 = c[DLF_G4];
        let sg2 = sg3 * c[DLF_G3];
        let sg1 = sg2 * c[DLF_G2];

        let g = c[DLF_G];
        let gp1 = g + 1.0;
        let hg = g * 0.5;

        let beta1 = 1.0 / (gp1 - g * c[DLF_G2]);
        let beta2 = 1.0 / (gp1 - hg * c[DLF_G3]);
        let beta3 = 1.0 / (gp1 - hg * c[DLF_G4]);
        let beta4 = 1.0 / gp1;

        let gamma1 = c[DLF_G1] * c[DLF_G2] + 1.0;
        let gamma2 = c[DLF_G2] * c[DLF_G3] + 1.0;
        let gamma3 = c[DLF_G3] * c[DLF_G4] + 1.0;

        let feedback3 = get_fo(beta4, 0.0, 0.0, r[DLF_Z4]);
        let feedback2 = get_fo(beta3, hg, r[DLF_FEEDBACK3], r[DLF_Z3]);
        let feedback1 = get_fo(beta2, hg, r[DLF_FEEDBACK2], r[DLF_Z2]);

        let sigma = sg1 * get_fo(beta1, g, feedback1, r[DLF_Z1])
            + sg2 * get_fo(beta2, hg, feedback2, r[DLF_Z2])
            + sg3 * get_fo(beta3, hg, feedback3, r[DLF_Z3])
            + get_fo(beta4, 0.0, 0.0, r[DLF_Z4]);

        r[DLF_FEEDBACK3] = feedback3;
        r[DLF_FEEDBACK2] = feedback2;
        r[DLF_FEEDBACK1] = feedback1;

        let km = c[DLF_KM];
        // Gain compensation, then the zero-delay feedback solve for the ladder input.
        let comp = (0.3 * km + 1.0) * x;
        let u = (comp - km * sigma) / (km * c[DLF_GAMMA] + 1.0);

        let alpha = c[DLF_ALPHA];
        let fo1 = get_fo(beta1, g, feedback1, r[DLF_Z1]);
        let result1 = do_lpf(
            u,
            alpha,
            gamma1,
            c[DLF_G2],
            1.0,
            feedback1,
            fo1,
            &mut r[DLF_Z1],
        );
        let fo2 = get_fo(beta2, hg, feedback2, r[DLF_Z2]);
        let result2 = do_lpf(
            result1,
            alpha,
            gamma2,
            c[DLF_G3],
            0.5,
            feedback2,
            fo2,
            &mut r[DLF_Z2],
        );
        let fo3 = get_fo(beta3, hg, feedback3, r[DLF_Z3]);
        let result3 = do_lpf(
            result2,
            alpha,
            gamma3,
            c[DLF_G4],
            0.5,
            feedback3,
            fo3,
            &mut r[DLF_Z3],
        );
        let fo4 = get_fo(beta4, 0.0, 0.0, r[DLF_Z4]);
        let result4 = do_lpf(result3, alpha, 1.0, 0.0, 0.5, 0.0, fo4, &mut r[DLF_Z4]);

        // Per-tap makeup gains from Surge.
        match mode {
            LadderSlope::Db6 => result1 * 0.125,
            LadderSlope::Db12 => result2 * 0.3,
            LadderSlope::Db18 => result3 * 0.6,
            LadderSlope::Db24 => result4 * 1.2,
        }
    }
}

filter_module! {
    /// Surge XT's Diode Ladder — an EMS-style diode-cascade lowpass (zero-delay
    /// feedback, adapted from Odin 2). Screams at high resonance.
    ///
    /// - **cutoff** — V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; high values self-oscillate.
    /// - **slope** — output tap: `'db6'`, `'db12'`, `'db18'`, or `'db24'`.
    ///
    /// ```js
    /// $unstable.filter.diodeLadder($saw('c2'), 'c4', 3, { slope: 'db24' })
    /// ```
    name = "$unstable.filter.diodeLadder", ident = DiodeLadderFilter, kernel = DiodeLadder,
    output_doc = "lowpass output",
    params = {
        /// slope: output tap at 6, 12, 18, or 24 dB/oct (default 24)
        slope: LadderSlope,
    },
    mode = |p| p.slope,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::{sine_rms, sweep_stays_bounded};

    const ALL_SLOPES: [LadderSlope; 4] = [
        LadderSlope::Db6,
        LadderSlope::Db12,
        LadderSlope::Db18,
        LadderSlope::Db24,
    ];

    /// Lowpass shape at every tap, and each successive tap attenuates more at 8 kHz
    /// (the makeup gains never outweigh the added pole).
    fn assert_lowpass_taps<F: Filter<Mode = LadderSlope>>(sr: f32) {
        // 1 kHz cutoff expressed in semitones above A440.
        let cutoff_semi = 12.0 * (1000.0f32 / 440.0).log2();
        let mut prev_high = f32::MAX;
        for slope in ALL_SLOPES {
            let low = sine_rms::<F>(slope, cutoff_semi, 0.0, 200.0, sr);
            let high = sine_rms::<F>(slope, cutoff_semi, 0.0, 8000.0, sr);
            assert!(
                high < low * 0.5,
                "expected lowpass attenuation ({slope:?}): low={low} high={high}"
            );
            assert!(
                high < prev_high,
                "expected each tap to attenuate more than the last ({slope:?}): {high} vs {prev_high}"
            );
            prev_high = high;
        }
    }

    #[test]
    fn legacy_ladder_attenuates_highs_and_steepens_per_tap() {
        // The legacy ladder's kernel run rate is 2× the 48 kHz engine rate.
        assert_lowpass_taps::<LegacyLadder>(96_000.0);
    }

    #[test]
    fn diode_ladder_attenuates_highs_and_steepens_per_tap() {
        assert_lowpass_taps::<DiodeLadder>(48_000.0);
    }

    #[test]
    fn legacy_ladder_survives_resonant_cutoff_sweep() {
        sweep_stays_bounded::<LegacyLadder>(&ALL_SLOPES);
    }

    #[test]
    fn diode_ladder_survives_resonant_cutoff_sweep() {
        sweep_stays_bounded::<DiodeLadder>(&ALL_SLOPES);
    }
}
