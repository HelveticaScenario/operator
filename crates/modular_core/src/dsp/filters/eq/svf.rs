//! EQ sections on the shared trapezoidal SVF kernel, with the
//! gain-parameterized output mixes from Simper's "Solving the continuous SVF
//! equations using trapezoidal integration and equivalent currents" (2016).

use super::params::{DEFAULT_Q, EqSection, EqSectionType, MAX_Q, MIN_Q, effective_q};
use crate::dsp::filters::svf_core::SvfCoeffs;
use crate::dsp::utils::{changed, voct_to_hz};
use crate::poly::{MonoSignal, MonoSignalExt};
use crate::types::SwapGlide;

/// Per-section patch-update glides and change-detection state. `NaN` = never computed.
#[derive(Clone, Copy)]
struct SectionSmooth {
    last_freq: f32,
    last_gain: f32,
    last_q: f32,
    smooth_freq: SwapGlide,
    smooth_gain: SwapGlide,
    smooth_q: SwapGlide,
}

impl Default for SectionSmooth {
    fn default() -> Self {
        Self {
            last_freq: f32::NAN,
            last_gain: f32::NAN,
            last_q: f32::NAN,
            smooth_freq: SwapGlide::default(),
            smooth_gain: SwapGlide::default(),
            smooth_q: SwapGlide::default(),
        }
    }
}

/// One section's glided params and the coefficients derived from them.
#[derive(Clone, Copy, Default)]
pub(super) struct SectionRuntime {
    smooth: SectionSmooth,
    pub(super) coeffs: SvfCoeffs,
    /// Every param is a fixed value whose glide has finished, so refreshing
    /// can change nothing until the next patch update.
    settled: bool,
}

/// An optional param's fixed value (its default when absent), or None when
/// it is signal-driven.
fn fixed_or(signal: &Option<MonoSignal>, default: f32) -> Option<f32> {
    match signal {
        Some(s) => s.constant_value(),
        None => Some(default),
    }
}

impl SectionRuntime {
    /// Glides the section's params and recomputes coefficients when they move.
    pub(super) fn refresh(&mut self, sec: &EqSection, sample_rate: f32, adaptive_q: bool) {
        if self.settled {
            return;
        }
        let s = &mut self.smooth;
        let (target_f, target_g, target_q) = (
            sec.freq.get_value(),
            sec.gain.value_or(0.0),
            sec.q.value_or(DEFAULT_Q),
        );
        s.smooth_freq
            .update(target_f, sec.freq.glide_mark(), sample_rate);
        s.smooth_gain
            .update(target_g, sec.gain.glide_mark(), sample_rate);
        s.smooth_q.update(target_q, sec.q.glide_mark(), sample_rate);
        let arrived = !s.smooth_freq.gliding() && !s.smooth_gain.gliding() && !s.smooth_q.gliding();
        if arrived
            && sec.freq.constant_value().is_some()
            && fixed_or(&sec.gain, 0.0).is_some()
            && fixed_or(&sec.q, DEFAULT_Q).is_some()
        {
            self.settled = true;
        }
        let f = *s.smooth_freq;
        let g = (*s.smooth_gain).clamp(-24.0, 24.0);
        let q = (*s.smooth_q).clamp(MIN_Q, MAX_Q);
        if changed(f, s.last_freq) || changed(g, s.last_gain) || changed(q, s.last_q) {
            let q_eff = effective_q(sec.r#type, q, g, adaptive_q);
            self.coeffs = compute_section(sec.r#type, f, g, q_eff, sample_rate);
            s.last_freq = f;
            s.last_gain = g;
            s.last_q = q;
        }
    }

    /// The values the filter is running at: glided freq (V/Oct), and gain
    /// and q after gliding and clamping.
    pub(super) fn live_values(&self) -> [f32; 3] {
        let s = &self.smooth;
        [
            *s.smooth_freq,
            (*s.smooth_gain).clamp(-24.0, 24.0),
            (*s.smooth_q).clamp(MIN_Q, MAX_Q),
        ]
    }

    /// Forces the next refresh to run its glides and recompute coefficients.
    pub(super) fn invalidate(&mut self) {
        self.settled = false;
        self.smooth.last_freq = f32::NAN;
        self.smooth.last_gain = f32::NAN;
        self.smooth.last_q = f32::NAN;
    }
}

fn compute_section(
    kind: EqSectionType,
    freq_volts: f32,
    gain_db: f32,
    q: f32,
    sample_rate: f32,
) -> SvfCoeffs {
    let freq = voct_to_hz(freq_volts).clamp(20.0, sample_rate * 0.495);
    let a = 10f32.powf(gain_db / 40.0);
    let g0 = (std::f32::consts::PI * freq / sample_rate).tan();

    let (g, k, m0, m1, m2) = match kind {
        EqSectionType::Peak => {
            let k = 1.0 / (q * a);
            (g0, k, 1.0, k * (a * a - 1.0), 0.0)
        }
        EqSectionType::LowShelf => {
            let k = 1.0 / q;
            (g0 / a.sqrt(), k, 1.0, k * (a - 1.0), a * a - 1.0)
        }
        EqSectionType::HighShelf => {
            let k = 1.0 / q;
            (g0 * a.sqrt(), k, a * a, k * (1.0 - a) * a, 1.0 - a * a)
        }
        EqSectionType::Lowpass => (g0, 1.0 / q, 0.0, 0.0, 1.0),
        EqSectionType::Highpass => {
            let k = 1.0 / q;
            (g0, k, 1.0, -k, -1.0)
        }
        EqSectionType::Notch => {
            let k = 1.0 / q;
            (g0, k, 1.0, -k, 0.0)
        }
    };

    SvfCoeffs::new(g, k, m0, m1, m2)
}
