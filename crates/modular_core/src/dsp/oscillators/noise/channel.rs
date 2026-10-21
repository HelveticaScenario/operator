//! Per-channel generator state for `$noise`.

use super::generators::{
    BLUE_GAIN, BROWN_STEP, Lfsr, PINK_GAIN, PinkFilter, coin, gaussian, rail, reflect,
};
use super::{NoiseInterp, NoiseKind};
use crate::dsp::utils::catmull_rom;
use crate::dsp::utils::rng::LcgRng;

/// Draws that settle the pink filter bank before its first output. Its slowest
/// pole has a time constant of about 880 draws, so a cold filter would start
/// near-silent for that long — minutes at slow rates.
const PINK_WARMUP: usize = 1024;

/// Each channel runs its own RNG and filters so every channel produces an
/// independent sequence.
#[derive(Default)]
pub(super) struct NoiseChannelState {
    generator: LcgRng,
    /// Set once this channel's RNG is seeded. Per channel, because channels
    /// added by a patch update arrive fresh beside transferred, seeded ones.
    pub(super) seeded: bool,
    pink: PinkFilter,
    /// Previous raw pink value, differentiated for blue.
    pink_prev: f32,
    /// Previous white value, differentiated for violet.
    white_prev: f32,
    brown: f32,
    lfsr: Lfsr,
    /// Clock phase in `[0, 1)`; a tick fires each time it wraps. f64, so the
    /// tiny increments of slow rates still advance it near 1.
    phase: f64,
    /// The last four drawn values, oldest first.
    history: [f32; 4],
    /// Telegraph level: +1 when set, −1 otherwise.
    telegraph_high: bool,
    /// Velvet: clock phase at which the scheduled impulse fires.
    impulse_at: f32,
    /// Velvet: sign of the scheduled impulse.
    impulse_sign: f32,
    /// Velvet: the scheduled impulse has not fired yet.
    impulse_pending: bool,
}

impl NoiseChannelState {
    /// Seed this channel's RNG from a [`seed_base`] and its channel index,
    /// then start `kind` fresh.
    ///
    /// [`seed_base`]: crate::dsp::utils::rng::seed_base
    pub(super) fn seed(&mut self, base: u64, channel: usize, kind: NoiseKind) {
        self.generator.seed(base, channel);
        self.seeded = true;
        self.reset(kind);
    }

    /// Start `kind` fresh, already mid-stream: filters are settled and the
    /// history is full, so output is noise from the first sample.
    pub(super) fn reset(&mut self, kind: NoiseKind) {
        self.pink = PinkFilter::default();
        self.pink_prev = 0.0;
        self.white_prev = 0.0;
        self.brown = self.generator.next_bipolar();
        self.lfsr.reset(&mut self.generator);
        self.telegraph_high = self.generator.next_unit() >= 0.5;
        self.phase = 0.0;
        self.schedule_impulse();

        match kind {
            NoiseKind::Velvet | NoiseKind::Telegraph | NoiseKind::Dust => {}
            _ => {
                if matches!(kind, NoiseKind::Pink | NoiseKind::Blue) {
                    for _ in 0..PINK_WARMUP {
                        self.draw(kind);
                    }
                }
                for _ in 0..self.history.len() {
                    self.push(kind);
                }
            }
        }
    }

    /// Draw the next value of a clocked value kind, in `[-1, 1]`.
    fn draw(&mut self, kind: NoiseKind) -> f32 {
        let rng = &mut self.generator;
        match kind {
            NoiseKind::White => rng.next_bipolar(),
            NoiseKind::Pink => (self.pink.process(rng.next_bipolar()) * PINK_GAIN).clamp(-1.0, 1.0),
            NoiseKind::Brown => {
                self.brown = reflect(self.brown + rng.next_bipolar() * BROWN_STEP);
                self.brown
            }
            NoiseKind::Blue => {
                let pink = self.pink.process(rng.next_bipolar());
                let blue = (pink - self.pink_prev) * BLUE_GAIN;
                self.pink_prev = pink;
                blue.clamp(-1.0, 1.0)
            }
            NoiseKind::Violet => {
                let white = rng.next_bipolar();
                let violet = (white - self.white_prev) * 0.5;
                self.white_prev = white;
                violet
            }
            NoiseKind::Binary => coin(rng),
            NoiseKind::Gaussian => gaussian(rng),
            NoiseKind::Lfsr => self.lfsr.step(1),
            NoiseKind::LfsrShort => self.lfsr.step(6),
            NoiseKind::Velvet | NoiseKind::Telegraph | NoiseKind::Dust => {
                unreachable!("{kind:?} is not a clocked value kind")
            }
        }
    }

    fn push(&mut self, kind: NoiseKind) {
        let next = self.draw(kind);
        self.history = [self.history[1], self.history[2], self.history[3], next];
    }

    /// Advance the clock by `inc` ticks and return the output for a clocked
    /// value kind. Every tick that elapses within the sample is drawn.
    pub(super) fn clocked(&mut self, kind: NoiseKind, inc: f32, interp: NoiseInterp) -> f32 {
        self.phase += f64::from(inc);
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            self.push(kind);
        }
        let t = self.phase as f32;
        let h = self.history;
        match interp {
            NoiseInterp::Step => h[3],
            NoiseInterp::Linear => h[2] + (h[3] - h[2]) * t,
            NoiseInterp::Smooth => catmull_rom(h, t).clamp(-1.0, 1.0),
        }
    }

    fn schedule_impulse(&mut self) {
        self.impulse_at = self.generator.next_unit();
        self.impulse_sign = coin(&mut self.generator);
        self.impulse_pending = true;
    }

    /// Advance the clock by `inc` (≤ 1) ticks and return the velvet output.
    /// Each tick schedules one impulse at a random phase within it. An impulse
    /// still pending when its tick ends fires on that final sample, and one
    /// that would land on the same sample fires on the next.
    pub(super) fn velvet(&mut self, inc: f32) -> f32 {
        let mut out = 0.0;
        self.phase += f64::from(inc);
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            if self.impulse_pending {
                out = self.impulse_sign;
            }
            self.schedule_impulse();
        }
        if out == 0.0 && self.impulse_pending && self.phase > f64::from(self.impulse_at) {
            self.impulse_pending = false;
            out = self.impulse_sign;
        }
        out
    }

    /// Telegraph: flip with probability `p`, return the level.
    pub(super) fn telegraph(&mut self, p: f32) -> f32 {
        self.telegraph_high ^= self.generator.next_unit() < p;
        rail(self.telegraph_high)
    }

    /// Dust: +1 with probability `p`, else −1.
    pub(super) fn dust(&mut self, p: f32) -> f32 {
        rail(self.generator.next_unit() < p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An increment below an f32 step near 1 still advances the clock at its
    /// full speed.
    #[test]
    fn tiny_increments_keep_advancing() {
        let mut state = NoiseChannelState::default();
        state.seed(0x5eed, 0, NoiseKind::White);
        state.phase = 0.75;
        let inc = 2e-8;
        let samples = 48_000;
        for _ in 0..samples {
            state.clocked(NoiseKind::White, inc, NoiseInterp::Linear);
        }
        let expected = 0.75 + f64::from(inc) * samples as f64;
        assert!((state.phase - expected).abs() < 1e-9);
    }
}
