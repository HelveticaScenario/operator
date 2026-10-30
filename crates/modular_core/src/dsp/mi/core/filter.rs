//! Ports of `stmlib::OnePole` and `stmlib::Svf`.

use std::f64::consts::PI;

/// `OnePole::tan<FREQUENCY_DIRTY>`: `tan(π f)` for a frequency `f` in cycles
/// per sample, accurate below about f = 1/6.
#[inline(always)]
pub fn tan_dirty(f: f32) -> f32 {
    const A: f32 = (3.736e-01 * PI * PI * PI) as f32;
    f * (std::f32::consts::PI + A * f * f)
}

/// Topology-preserving one-pole filter.
#[derive(Clone, Copy, Default)]
pub struct OnePole {
    g: f32,
    gi: f32,
    state: f32,
}

impl OnePole {
    /// Sets the coefficient from `g = tan(π f)`.
    #[inline]
    pub fn set_g(&mut self, g: f32) {
        self.g = g;
        self.gi = 1.0 / (1.0 + g);
    }

    #[inline(always)]
    pub fn low_pass(&mut self, x: f32) -> f32 {
        let lp = (self.g * x + self.state) * self.gi;
        self.state = self.g * (x - lp) + lp;
        lp
    }

    #[inline(always)]
    pub fn high_pass(&mut self, x: f32) -> f32 {
        x - self.low_pass(x)
    }
}

/// Chamberlin-style state-variable filter with zero-delay feedback.
#[derive(Clone, Copy, Default)]
pub struct Svf {
    g: f32,
    r: f32,
    h: f32,
    state_1: f32,
    state_2: f32,
}

impl Svf {
    /// Sets the frequency from `g = tan(π f)` and the resonance `q`.
    #[inline]
    pub fn set_g_q(&mut self, g: f32, q: f32) {
        self.g = g;
        self.r = 1.0 / q;
        self.h = 1.0 / (1.0 + self.r * g + g * g);
    }

    #[inline(always)]
    pub fn low_pass(&mut self, x: f32) -> f32 {
        let hp = (x - self.r * self.state_1 - self.g * self.state_1 - self.state_2) * self.h;
        let bp = self.g * hp + self.state_1;
        self.state_1 = self.g * hp + bp;
        let lp = self.g * bp + self.state_2;
        self.state_2 = self.g * bp + lp;
        lp
    }
}
