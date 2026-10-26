//! Per-channel generator state for `$noise`.

use super::generators::{
    BLUE_GAIN, BROWN_STEP, Lfsr, LfsrLoop, PINK_GAIN, PinkFilter, coin, dust_gap, gaussian, rail,
    reflect,
};
use super::{NoiseInterp, NoiseKind};
use crate::dsp::utils::rng::LcgRng;
use crate::dsp::utils::{CatmullRom, TempGate, TempGateState, trigger_samples};

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
    lfsr_loop: LfsrLoop,
    /// Clock phase in `[0, 1)`; a tick fires each time it wraps. f64, so the
    /// tiny increments of slow rates still advance it near 1.
    phase: f64,
    /// The last four drawn values, oldest first.
    history: [f32; 4],
    /// The curve through `history`, from the third value toward the fourth.
    curve: CatmullRom,
    /// Telegraph level: +1 when set, −1 otherwise.
    telegraph_high: bool,
    /// Velvet: clock phase at which the scheduled impulse fires.
    impulse_at: f32,
    /// Velvet: sign of the scheduled impulse.
    impulse_sign: f32,
    /// Velvet: the scheduled impulse has not fired yet.
    impulse_pending: bool,
    /// Velvet: clock phase of the next tick's impulse, drawn a tick ahead so
    /// each impulse knows the gap to the next.
    next_impulse_at: f32,
    /// Velvet: sign of the next tick's impulse.
    next_impulse_sign: f32,
    /// The pulse dust or velvet is holding.
    pulse: TempGate,
    /// Dust: time left until the next trigger, in average gaps. f64 for the
    /// same reason as `phase`.
    dust_wait: f64,
}

impl NoiseChannelState {
    /// Seed this channel's RNG from a [`seed_base`] and its channel index, set
    /// where its LFSR starts, then start `kind` fresh.
    ///
    /// [`seed_base`]: crate::dsp::utils::rng::seed_base
    pub(super) fn seed(
        &mut self,
        base: u64,
        channel: usize,
        lfsr_phase: u16,
        lfsr_loop: LfsrLoop,
        kind: NoiseKind,
    ) {
        self.generator.seed(base, channel);
        self.lfsr.set_phase(lfsr_phase);
        self.lfsr_loop = lfsr_loop;
        self.seeded = true;
        self.reset(kind);
    }

    /// The LFSR's current long-mode phase; see [`Lfsr::phase`].
    pub(super) fn lfsr_phase(&self) -> u16 {
        self.lfsr.phase()
    }

    /// Start `kind` fresh, already mid-stream: filters are settled and the
    /// history is full, so output is noise from the first sample.
    pub(super) fn reset(&mut self, kind: NoiseKind) {
        self.pink = PinkFilter::default();
        self.pink_prev = 0.0;
        self.white_prev = 0.0;
        self.brown = self.generator.next_bipolar();
        match kind {
            NoiseKind::Lfsr => self.lfsr.start_long(),
            NoiseKind::LfsrShort => self.lfsr.start_short(self.lfsr_loop),
            _ => {}
        }
        self.telegraph_high = self.generator.next_unit() >= 0.5;
        self.phase = 0.0;
        self.schedule_impulse();
        self.schedule_impulse();
        self.pulse = TempGate::new(TempGateState::Low, 0.0, 0.0);

        match kind {
            // A random fraction of a gap, so the first trigger can come at any
            // moment.
            NoiseKind::Dust => {
                self.pulse = TempGate::new(TempGateState::Low, -1.0, -1.0);
                self.dust_wait =
                    f64::from(dust_gap(&mut self.generator) * self.generator.next_unit());
            }
            NoiseKind::Velvet | NoiseKind::Telegraph => {}
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
            NoiseKind::Lfsr => self.lfsr.step_long(),
            NoiseKind::LfsrShort => self.lfsr.step_short(),
            NoiseKind::Velvet | NoiseKind::Telegraph | NoiseKind::Dust => {
                unreachable!("{kind:?} is not a clocked value kind")
            }
        }
    }

    fn push(&mut self, kind: NoiseKind) {
        let next = self.draw(kind);
        self.history = [self.history[1], self.history[2], self.history[3], next];
        self.curve = CatmullRom::new(self.history);
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
            NoiseInterp::Smooth => self.curve.at(t).clamp(-1.0, 1.0),
        }
    }

    /// Move the next tick's impulse into the current tick and draw the one
    /// after it.
    fn schedule_impulse(&mut self) {
        self.impulse_at = self.next_impulse_at;
        self.impulse_sign = self.next_impulse_sign;
        self.impulse_pending = true;
        self.next_impulse_at = self.generator.next_unit();
        self.next_impulse_sign = coin(&mut self.generator);
    }

    /// Start a pulse from `rest` to `level` that lasts the standard trigger
    /// length, shortened when the next pulse is `gap_ticks` away at `inc`
    /// ticks per sample.
    fn start_pulse(&mut self, rest: f32, level: f32, gap_ticks: f64, inc: f32, sample_rate: f32) {
        let gap_samples = (gap_ticks / f64::from(inc)) as f32;
        self.pulse = TempGate::new(TempGateState::Low, rest, level);
        self.pulse.set_state(
            TempGateState::High,
            TempGateState::Low,
            trigger_samples(sample_rate, gap_samples),
        );
    }

    /// Advance the clock by `inc` (≤ 1) ticks and return the velvet output.
    /// Each tick schedules one impulse at a random phase within it. An impulse
    /// still pending when its tick ends fires on that final sample, and one
    /// that would land on the same sample fires on the next.
    pub(super) fn velvet(&mut self, inc: f32, sample_rate: f32) -> f32 {
        self.phase += f64::from(inc);
        let mut fired = false;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            let late_sign = self.impulse_pending.then_some(self.impulse_sign);
            self.schedule_impulse();
            if let Some(sign) = late_sign {
                fired = true;
                let gap = (f64::from(self.impulse_at) - self.phase).max(0.0);
                self.start_pulse(0.0, sign, gap, inc, sample_rate);
            }
        }
        if !fired && self.impulse_pending && self.phase > f64::from(self.impulse_at) {
            self.impulse_pending = false;
            let gap = 1.0 - self.phase + f64::from(self.next_impulse_at);
            self.start_pulse(0.0, self.impulse_sign, gap, inc, sample_rate);
        }
        self.pulse.process()
    }

    /// Telegraph: flip with probability `p`, return the level.
    pub(super) fn telegraph(&mut self, p: f32) -> f32 {
        self.telegraph_high ^= self.generator.next_unit() < p;
        rail(self.telegraph_high)
    }

    /// Dust: count down `inc` average gaps and return the output, −1 at rest
    /// and +1 during a trigger. Triggers that fall within one sample merge.
    pub(super) fn dust(&mut self, inc: f32, sample_rate: f32) -> f32 {
        self.dust_wait -= f64::from(inc);
        if self.dust_wait <= 0.0 {
            while self.dust_wait <= 0.0 {
                self.dust_wait += f64::from(dust_gap(&mut self.generator));
            }
            self.start_pulse(-1.0, 1.0, self.dust_wait, inc, sample_rate);
        }
        self.pulse.process()
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
        state.seed(0x5eed, 0, 0, LfsrLoop::default(), NoiseKind::White);
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
