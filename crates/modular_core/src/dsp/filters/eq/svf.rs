//! Andy Simper's (Cytomic) trapezoidal SVF as an EQ section, with
//! gain-parameterized output mixes from "Solving the continuous SVF equations
//! using trapezoidal integration and equivalent currents" (2016).

use super::params::{DEFAULT_Q, EqSection, EqSectionType, MAX_Q, MIN_Q, effective_q};
use crate::dsp::utils::{changed, sanitize, voct_to_hz};
use crate::poly::{MonoSignal, MonoSignalExt};
use crate::types::Clickless;

/// Per-section smoothing and change-detection state. `NaN` = never computed.
#[derive(Clone, Copy)]
struct SectionSmooth {
    last_freq: f32,
    last_gain: f32,
    last_q: f32,
    smooth_freq: Clickless,
    smooth_gain: Clickless,
    smooth_q: Clickless,
}

impl Default for SectionSmooth {
    fn default() -> Self {
        Self {
            last_freq: f32::NAN,
            last_gain: f32::NAN,
            last_q: f32::NAN,
            smooth_freq: Clickless::default(),
            smooth_gain: Clickless::default(),
            smooth_q: Clickless::default(),
        }
    }
}

/// Trapezoidal-integration gains plus the per-type output mix.
#[derive(Clone, Copy, Default)]
pub(super) struct SvfCoeffs {
    a1: f32,
    a2: f32,
    a3: f32,
    m0: f32,
    m1: f32,
    m2: f32,
}

/// One section's smoothed params and the coefficients derived from them.
#[derive(Clone, Copy, Default)]
pub(super) struct SectionRuntime {
    smooth: SectionSmooth,
    pub(super) coeffs: SvfCoeffs,
    /// Every param is a fixed value its smoother has reached, so refreshing
    /// can change nothing until the next patch update.
    settled: bool,
}

/// Smoothed values this close to their fixed targets count as arrived.
const SETTLE_EPSILON: f32 = 1e-4;

/// An optional param's fixed value (its default when absent), or None when
/// it is signal-driven.
fn fixed_or(signal: &Option<MonoSignal>, default: f32) -> Option<f32> {
    match signal {
        Some(s) => s.constant_value(),
        None => Some(default),
    }
}

impl SectionRuntime {
    /// Smooths the section's params and recomputes coefficients when they move.
    pub(super) fn refresh(&mut self, sec: &EqSection, sample_rate: f32, adaptive_q: bool) {
        if self.settled {
            return;
        }
        self.advance(
            sec,
            [
                sec.freq.get_value(),
                sec.gain.value_or(0.0),
                sec.q.value_or(DEFAULT_Q),
            ],
            sample_rate,
            adaptive_q,
        );
    }

    /// One sample of [`refresh`](Self::refresh) toward `targets`, the
    /// section's freq, gain and q, on a runtime that is not settled.
    pub(super) fn advance(
        &mut self,
        sec: &EqSection,
        [target_f, target_g, target_q]: [f32; 3],
        sample_rate: f32,
        adaptive_q: bool,
    ) {
        let s = &mut self.smooth;
        s.smooth_freq.update(target_f);
        s.smooth_gain.update(target_g);
        s.smooth_q.update(target_q);
        let arrived = (*s.smooth_freq - target_f).abs() < SETTLE_EPSILON
            && (*s.smooth_gain - target_g).abs() < SETTLE_EPSILON
            && (*s.smooth_q - target_q).abs() < SETTLE_EPSILON;
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

    /// The values the filter is running at: smoothed freq (V/Oct), and gain
    /// and q after smoothing and clamping.
    pub(super) fn live_values(&self) -> [f32; 3] {
        let s = &self.smooth;
        [
            *s.smooth_freq,
            (*s.smooth_gain).clamp(-24.0, 24.0),
            (*s.smooth_q).clamp(MIN_Q, MAX_Q),
        ]
    }

    pub(super) fn is_settled(&self) -> bool {
        self.settled
    }

    /// Forces the next refresh to recompute coefficients, keeping the smoothers.
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

    let a1 = 1.0 / (1.0 + g * (g + k));
    let a2 = g * a1;
    let a3 = g * a2;
    SvfCoeffs {
        a1,
        a2,
        a3,
        m0,
        m1,
        m2,
    }
}

/// Runs one sample through a section, updating its two integrator states.
/// `x` must be finite: a chain sanitizes its input once, and each section's
/// output stays finite because its coefficients and states are.
#[inline]
pub(super) fn process(c: &SvfCoeffs, ic1: &mut f32, ic2: &mut f32, x: f32) -> f32 {
    let v3 = x - *ic2;
    let v1 = c.a1 * *ic1 + c.a2 * v3;
    let v2 = *ic2 + c.a2 * *ic1 + c.a3 * v3;
    *ic1 = sanitize(2.0 * v1 - *ic1);
    *ic2 = sanitize(2.0 * v2 - *ic2);
    c.m0 * x + c.m1 * v1 + c.m2 * v2
}
