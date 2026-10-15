//! Building blocks shared by the drum voices: strike detection with a held
//! accent, an exponential decay envelope, and a resonant state-variable filter.

use crate::dsp::consts::{LUT_SINE, LUT_SINE_SIZE};
use crate::dsp::utils::{SchmittTrigger, interpolate, one_pole::CachedCoeff, sanitize};

/// ln(1000): converts a time to −60 dB into a one-pole time constant.
pub const T60_TO_TAU: f32 = 6.907_755;

/// Below this level an envelope counts as finished.
pub const SILENCE: f32 = 1.0e-6;

/// Sine of `phase` in cycles.
#[inline]
pub fn sine(phase: f32) -> f32 {
    interpolate(LUT_SINE, phase, LUT_SINE_SIZE)
}

/// Rising-edge detector that samples the accent at each strike.
#[derive(Default, Clone, Copy)]
pub struct Strike {
    trigger: SchmittTrigger,
    /// Strength of the current hit, 0–1, from the accent at the strike.
    pub velocity: f32,
}

impl Strike {
    /// Returns true on a rising edge of `trigger`, latching `accent` (0–5 V)
    /// as the hit's velocity.
    #[inline]
    pub fn process(&mut self, trigger: f32, accent: f32) -> bool {
        let struck = self.trigger.process(trigger);
        if struck {
            self.velocity = accent.clamp(0.0, 5.0) / 5.0;
        }
        struck
    }

    /// Output level for the current hit: `floor` for the softest accent,
    /// rising linearly to 1 at full accent.
    #[inline]
    pub fn gain(&self, floor: f32) -> f32 {
        floor + (1.0 - floor) * self.velocity
    }
}

/// Exponential decay from 1 to silence, falling 60 dB over a given time.
#[derive(Default, Clone, Copy)]
pub struct Decay {
    pub level: f32,
    coeff: CachedCoeff,
}

impl Decay {
    #[inline]
    pub fn strike(&mut self) {
        self.level = 1.0;
    }

    /// Advance one sample with `t60` seconds to −60 dB and return the level.
    #[inline]
    pub fn next(&mut self, t60: f32, sample_rate: f32) -> f32 {
        let tau = t60.max(1.0e-4) / T60_TO_TAU;
        self.level *= 1.0 - self.coeff.get(tau, sample_rate);
        if self.level < SILENCE {
            self.level = 0.0;
        }
        self.level
    }

    #[inline]
    pub fn active(&self) -> bool {
        self.level > 0.0
    }
}

/// Attack curve: an RC charge, `(1 − e^(−kt/T)) / (1 − e^(−k))`, with this `k`.
const ATTACK_CURVE: f32 = 2.0;
/// e^(−ATTACK_CURVE): the charge left when the attack reaches full level.
const ATTACK_END: f32 = 0.135_335_28;

/// Swell to full level over an attack time, then decay exponentially. A
/// retrigger swells from the current level.
#[derive(Default, Clone, Copy)]
pub struct AttackDecay {
    pub level: f32,
    attacking: bool,
    from: f32,
    /// Remaining part of the attack: the RC charge left, from 1 down to
    /// `ATTACK_END`, or the straight-line share left, from 1 down to 0.
    left: f32,
    attack_coeff: CachedCoeff,
    decay: Decay,
}

impl AttackDecay {
    #[inline]
    pub fn strike(&mut self) {
        self.attacking = true;
        self.from = self.level;
        self.left = 1.0;
    }

    /// Advance one sample with `attack` seconds of RC-charge swell to full
    /// level and `t60` seconds from there to −60 dB, returning the level.
    #[inline]
    pub fn next(&mut self, attack: f32, t60: f32, sample_rate: f32) -> f32 {
        if self.attacking {
            let secs = attack.max(1.0e-4) / ATTACK_CURVE;
            self.left *= 1.0 - self.attack_coeff.get(secs, sample_rate);
            self.swell((self.left - ATTACK_END) / (1.0 - ATTACK_END));
        } else if self.decay.active() {
            self.level = self.decay.next(t60, sample_rate);
        }
        self.level
    }

    /// [`next`](Self::next) with a straight-line swell.
    #[inline]
    pub fn next_linear(&mut self, attack: f32, t60: f32, sample_rate: f32) -> f32 {
        if self.attacking {
            self.left -= 1.0 / (attack.max(1.0e-4) * sample_rate);
            self.swell(self.left);
        } else if self.decay.active() {
            self.level = self.decay.next(t60, sample_rate);
        }
        self.level
    }

    /// Set the level with `remaining` (1 down to 0) of the swell to go,
    /// handing over to the decay once it is done.
    #[inline]
    fn swell(&mut self, remaining: f32) {
        if remaining <= 0.0 {
            self.attacking = false;
            self.decay.strike();
            self.level = 1.0;
        } else {
            self.level = self.from + (1.0 - self.from) * (1.0 - remaining);
        }
    }

    #[inline]
    pub fn active(&self) -> bool {
        self.attacking || self.decay.active()
    }
}

/// Lowpass, bandpass (unity gain at the centre) and highpass outputs of one
/// filter step.
#[derive(Clone, Copy)]
pub struct SvfOut {
    pub lp: f32,
    pub bp: f32,
    pub hp: f32,
}

/// Trapezoidal state-variable filter. Coefficients are recomputed only when
/// the cutoff or Q changes.
#[derive(Clone, Copy)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    cutoff: f32,
    q: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Default for Svf {
    fn default() -> Self {
        // NaN never compares equal, so the first step always computes coefficients.
        Self {
            ic1: 0.0,
            ic2: 0.0,
            cutoff: f32::NAN,
            q: f32::NAN,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
        }
    }
}

impl Svf {
    /// Filter one sample with `cutoff` in Hz (clamped below Nyquist) and resonance `q`.
    #[inline]
    pub fn process(&mut self, input: f32, cutoff: f32, q: f32, sample_rate: f32) -> SvfOut {
        if cutoff != self.cutoff || q != self.q {
            self.cutoff = cutoff;
            self.q = q;
            let fc = cutoff.clamp(10.0, sample_rate * 0.45);
            let g = (std::f32::consts::PI * fc / sample_rate).tan();
            self.k = 1.0 / q.max(0.05);
            self.a1 = 1.0 / (1.0 + g * (g + self.k));
            self.a2 = g * self.a1;
            self.a3 = g * self.a2;
        }
        let v3 = input - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = sanitize(2.0 * v1 - self.ic1);
        self.ic2 = sanitize(2.0 * v2 - self.ic2);
        SvfOut {
            lp: v2,
            bp: self.k * v1,
            hp: input - self.k * v1 - v2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn tone_gain(hz: f32, pick: impl Fn(SvfOut) -> f32) -> f32 {
        let mut f = Svf::default();
        let mut peak = 0.0f32;
        for i in 0..48_000 {
            let x = sine((i as f32 * hz / SR).fract());
            let y = pick(f.process(x, 1000.0, 4.0, SR));
            if i > 24_000 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn bandpass_is_unity_at_the_centre() {
        assert!((tone_gain(1000.0, |o| o.bp) - 1.0).abs() < 0.02);
        assert!(tone_gain(100.0, |o| o.bp) < 0.1);
    }

    #[test]
    fn lowpass_and_highpass_split_the_band() {
        assert!(tone_gain(50.0, |o| o.lp) > 0.95);
        assert!(tone_gain(50.0, |o| o.hp) < 0.05);
        assert!(tone_gain(15_000.0, |o| o.hp) > 0.95);
    }

    #[test]
    fn decay_reaches_minus_60_db_at_t60() {
        let mut d = Decay::default();
        d.strike();
        let mut level = 1.0;
        for _ in 0..24_000 {
            level = d.next(0.5, SR);
        }
        let db = 20.0 * level.log10();
        assert!((db + 60.0).abs() < 0.5, "{db}");
    }

    #[test]
    fn strike_latches_accent_on_the_rising_edge() {
        let mut s = Strike::default();
        assert!(s.process(5.0, 2.5));
        assert!((s.velocity - 0.5).abs() < 1e-6);
        assert!(!s.process(5.0, 5.0));
        assert!((s.velocity - 0.5).abs() < 1e-6);
    }

    #[test]
    fn strike_gain_rises_from_its_floor() {
        let mut s = Strike::default();
        s.process(5.0, 0.0);
        assert!((s.gain(0.1) - 0.1).abs() < 1e-6);
        s.process(0.0, 0.0);
        s.process(5.0, 5.0);
        assert!((s.gain(0.1) - 1.0).abs() < 1e-6);
    }
}
