//! Polyphase IIR halfband filter for 2× oversampling.
//!
//! Two parallel branches, each a cascade of two first-order allpass sections.
//! Coefficients from Olli Niemitalo, "Polyphase IIR Filters" (4-coef halfband,
//! ~24 dB stopband attenuation, ~0.05·fs transition band).
//!
//! Operating at the lower (base) rate, each branch is a first-order allpass:
//!   y[n] = a·(x[n] − y[n−1]) + x[n−1]
//! State per stage: previous input and previous output.

use super::sanitize;

/// Coefficients for the cascade in branch A. Cascaded as stage1 → stage2.
const BRANCH_A: [f32; 2] = [0.07471, 0.42943];

/// Coefficients for the cascade in branch B.
const BRANCH_B: [f32; 2] = [0.24847, 0.79847];

#[derive(Clone, Copy, Default)]
struct Allpass1 {
    coeff: f32,
    x_prev: f32,
    y_prev: f32,
}

impl Allpass1 {
    fn new(coeff: f32) -> Self {
        Self {
            coeff,
            x_prev: 0.0,
            y_prev: 0.0,
        }
    }

    /// The stored state is sanitized every sample so a non-finite input cannot
    /// lodge in the recursion — the filter recovers as soon as the input does.
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = sanitize(self.coeff * (x - self.y_prev) + self.x_prev);
        self.x_prev = sanitize(x);
        self.y_prev = y;
        y
    }

    fn reset(&mut self) {
        self.x_prev = 0.0;
        self.y_prev = 0.0;
    }
}

/// 2× upsampler: one input sample → two output samples.
#[derive(Clone, Copy)]
pub struct Halfband2xUp {
    a1: Allpass1,
    a2: Allpass1,
    b1: Allpass1,
    b2: Allpass1,
}

impl Default for Halfband2xUp {
    fn default() -> Self {
        Self {
            a1: Allpass1::new(BRANCH_A[0]),
            a2: Allpass1::new(BRANCH_A[1]),
            b1: Allpass1::new(BRANCH_B[0]),
            b2: Allpass1::new(BRANCH_B[1]),
        }
    }
}

impl Halfband2xUp {
    /// Returns (even_sample, odd_sample) at the upper rate.
    #[inline]
    pub fn process(&mut self, x: f32) -> (f32, f32) {
        let even = self.a2.process(self.a1.process(x));
        let odd = self.b2.process(self.b1.process(x));
        (even, odd)
    }

    pub fn reset(&mut self) {
        self.a1.reset();
        self.a2.reset();
        self.b1.reset();
        self.b2.reset();
    }
}

/// 2× downsampler: two input samples → one output sample.
#[derive(Clone, Copy)]
pub struct Halfband2xDown {
    a1: Allpass1,
    a2: Allpass1,
    b1: Allpass1,
    b2: Allpass1,
}

impl Default for Halfband2xDown {
    fn default() -> Self {
        Self {
            a1: Allpass1::new(BRANCH_A[0]),
            a2: Allpass1::new(BRANCH_A[1]),
            b1: Allpass1::new(BRANCH_B[0]),
            b2: Allpass1::new(BRANCH_B[1]),
        }
    }
}

impl Halfband2xDown {
    /// Consume an even/odd pair at the upper rate (even first in time), return
    /// one sample at the lower rate. Branch A takes the newer sample and branch
    /// B the older one, the polyphase counterpart of the upsampler's phases.
    #[inline]
    pub fn process(&mut self, x_even: f32, x_odd: f32) -> f32 {
        let a = self.a2.process(self.a1.process(x_odd));
        let b = self.b2.process(self.b1.process(x_even));
        (a + b) * 0.5
    }

    pub fn reset(&mut self) {
        self.a1.reset();
        self.a2.reset();
        self.b1.reset();
        self.b2.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn dc_unity_gain_after_settle() {
        let mut up = Halfband2xUp::default();
        let mut down = Halfband2xDown::default();
        let mut last = 0.0;
        for _ in 0..2000 {
            let (e, o) = up.process(1.0);
            last = down.process(e, o);
        }
        assert!(
            (last - 1.0).abs() < 1e-3,
            "DC should pass at unit gain, got {}",
            last
        );
    }

    #[test]
    fn zero_in_zero_out() {
        let mut up = Halfband2xUp::default();
        let mut down = Halfband2xDown::default();
        for _ in 0..100 {
            let (e, o) = up.process(0.0);
            let y = down.process(e, o);
            assert_eq!(y, 0.0);
        }
    }

    #[test]
    fn low_freq_sinusoid_preserved() {
        // Sine at 0.05 of base rate (well inside passband).
        let mut up = Halfband2xUp::default();
        let mut down = Halfband2xDown::default();
        let f_norm = 0.05;
        let n_warmup = 500;
        let n_measure = 2000;

        let mut peak = 0.0f32;
        for n in 0..(n_warmup + n_measure) {
            let x = (2.0 * PI * f_norm * n as f32).sin();
            let (e, o) = up.process(x);
            let y = down.process(e, o);
            if n >= n_warmup {
                peak = peak.max(y.abs());
            }
        }
        assert!(
            (peak - 1.0).abs() < 0.05,
            "low-freq sine should pass with ~unit amplitude, got peak {}",
            peak
        );
    }

    #[test]
    fn recovers_after_non_finite_input() {
        // A non-finite sample must not lodge in the allpass recursion: once the
        // input is finite again, the cascade settles back to passing the signal.
        let mut up = Halfband2xUp::default();
        let mut down = Halfband2xDown::default();
        for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let (e, o) = up.process(x);
            down.process(e, o);
        }
        let mut last = 0.0;
        for _ in 0..2000 {
            let (e, o) = up.process(1.0);
            last = down.process(e, o);
            assert!(last.is_finite());
        }
        assert!(
            (last - 1.0).abs() < 1e-3,
            "DC should pass at unit gain after recovery, got {last}"
        );
    }

    /// Steady-state gain (RMS ratio) for a sine at `f_norm` of the base rate
    /// after an up/down round trip.
    fn round_trip_gain(f_norm: f32) -> f32 {
        let mut up = Halfband2xUp::default();
        let mut down = Halfband2xDown::default();
        let (mut sum_x, mut sum_y) = (0.0f32, 0.0f32);
        for n in 0..8000 {
            let x = (2.0 * PI * f_norm * n as f32).sin();
            let (e, o) = up.process(x);
            let y = down.process(e, o);
            if n >= 4000 {
                sum_x += x * x;
                sum_y += y * y;
            }
        }
        (sum_y / sum_x).sqrt()
    }

    /// Gain the decimator applies to a sine at `f_norm` of the base rate, which
    /// lies above base Nyquist when `f_norm > 0.5`.
    fn decimated_gain(f_norm: f32) -> f32 {
        let mut down = Halfband2xDown::default();
        let x = |k: usize| (PI * f_norm * k as f32).sin();
        let mut sum_y = 0.0f32;
        for n in 0..4000 {
            let y = down.process(x(2 * n), x(2 * n + 1));
            if n >= 2000 {
                sum_y += y * y;
            }
        }
        (sum_y / 2000.0 * 2.0).sqrt()
    }

    #[test]
    fn round_trip_is_flat_across_the_passband() {
        for f_norm in [0.1, 0.2, 0.3, 0.4, 0.45] {
            let gain = round_trip_gain(f_norm);
            assert!((gain - 1.0).abs() < 0.01, "f={f_norm}: gain {gain}");
        }
    }

    #[test]
    fn downsampler_rejects_content_above_base_nyquist() {
        for f_norm in [0.6, 0.7, 0.8, 0.9] {
            let gain = decimated_gain(f_norm);
            assert!(gain < 0.07, "f={f_norm}: alias gain {gain}");
        }
    }

    #[test]
    fn upsample_then_downsample_finite_for_random_input() {
        let mut up = Halfband2xUp::default();
        let mut down = Halfband2xDown::default();
        // Pseudorandom-ish deterministic sequence
        let mut s: u32 = 0x1234_5678;
        for _ in 0..1000 {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let x = ((s >> 8) as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let (e, o) = up.process(x);
            let y = down.process(e, o);
            assert!(y.is_finite(), "output should remain finite");
        }
    }
}
