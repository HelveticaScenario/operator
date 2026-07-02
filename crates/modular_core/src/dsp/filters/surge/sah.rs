//! `$unstable.filter.sah` — Surge XT's Sample & Hold (`fut_SNH`): the input is sampled
//! at the cutoff frequency (a downsampling/decimation effect), with resonance
//! feeding the held value back into the sampler through a soft clip.
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use super::coeffs::note_to_hz;
use super::fastmath::softclip;
use super::filter_core::{Filter, N_COEFFS, N_REGISTERS};

/// Sample & Hold kernel. Surge runs it at the oversampled voice rate, so it runs 2×
/// here too (the sample clock in the coefficients tracks the run rate).
#[derive(Clone, Copy, Default)]
pub struct SampleHold;

impl Filter for SampleHold {
    type Mode = ();
    type Extra = ();

    /// Surge `Coeff_SNH`: the per-sample phase increment of the sample clock, plus
    /// the resonance feedback amount.
    fn coeffs(_mode: (), freq_semi: f32, reso: f32, rate: f32) -> [f32; N_COEFFS] {
        let mut c = [0.0f32; N_COEFFS];
        c[0] = note_to_hz(freq_semi) / rate;
        c[1] = reso;
        c
    }

    fn process(
        _mode: (),
        x: f32,
        c: &mut [f32; N_COEFFS],
        dc: &[f32; N_COEFFS],
        r: &mut [f32; N_REGISTERS],
        _extra: &mut (),
    ) -> f32 {
        c[0] += dc[0];
        c[1] += dc[1];

        r[0] += c[0];
        if r[0] > 0.0 {
            r[1] = softclip(x - c[1] * r[1]);
            r[0] -= 1.0;
        }
        r[1]
    }

    fn oversample(_mode: ()) -> bool {
        true
    }
}

filter_module! {
    /// Surge XT's Sample & Hold — samples the input at the cutoff frequency, holding
    /// each value until the next tick (a pitched decimation/downsampling effect).
    /// Resonance feeds the held value back through a soft clip.
    ///
    /// - **cutoff** — sample clock in V/Oct (0 V = C4). Accepts modulation.
    /// - **resonance** — 0–5; feedback into the sampler.
    ///
    /// ```js
    /// $unstable.filter.sah($saw('c2'), 'c1', 0)
    /// ```
    name = "$unstable.filter.sah", ident = SahFilter, kernel = SampleHold,
    output_doc = "sampled output",
    params = {},
    mode = |_p| (),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filters::surge::filter_core::test_util::sweep_stays_bounded;
    use crate::dsp::filters::surge::filter_core::{N_COEFFS, N_REGISTERS};

    /// A slow ramp sampled at a low clock comes out as a staircase: long runs of
    /// exactly-held values.
    #[test]
    fn holds_values_between_clock_ticks() {
        let sr = 96_000.0;
        // ~100 Hz sample clock → 960 samples per hold at 96 kHz.
        let clock_semi = 12.0 * (100.0f32 / 440.0).log2();
        let mut c = SampleHold::coeffs((), clock_semi, 0.0, sr);
        let dc = [0.0f32; N_COEFFS];
        let mut r = [0.0f32; N_REGISTERS];
        let mut prev = f32::NAN;
        let mut holds = 0u32;
        let mut changes = 0u32;
        for i in 0..9600 {
            let x = i as f32 / 9600.0;
            let y = SampleHold::process((), x, &mut c, &dc, &mut r, &mut ());
            if y == prev {
                holds += 1;
            } else {
                changes += 1;
            }
            prev = y;
        }
        assert!(
            changes >= 9 && changes <= 12,
            "expected ~10 clock ticks: {changes}"
        );
        assert!(holds > 9000, "expected long held runs: {holds}");
    }

    #[test]
    fn survives_resonant_cutoff_sweep() {
        sweep_stays_bounded::<SampleHold>(&[()]);
    }
}
