//! Per-channel noise sources for `$noise`. Every source draws one value per
//! clock tick, normalized to `[-1, 1]`.

use crate::dsp::utils::rng::LcgRng;
use crate::poly::PORT_MAX_CHANNELS;

/// Paul Kellett's pink filter bank: −3 dB/octave from white input.
#[derive(Default)]
pub(super) struct PinkFilter {
    b0: f32,
    b1: f32,
    b2: f32,
    b3: f32,
    b4: f32,
    b5: f32,
    b6: f32,
}

impl PinkFilter {
    /// Unscaled filter output (RMS ≈ 1.8 for uniform white input).
    pub(super) fn process(&mut self, white: f32) -> f32 {
        self.b0 = 0.99886 * self.b0 + white * 0.0555179;
        self.b1 = 0.99332 * self.b1 + white * 0.0750759;
        self.b2 = 0.96900 * self.b2 + white * 0.153_852;
        self.b3 = 0.86650 * self.b3 + white * 0.3104856;
        self.b4 = 0.55000 * self.b4 + white * 0.5329522;
        self.b5 = -0.7616 * self.b5 - white * 0.0168980;
        self.b6 = white * 0.5362;

        self.b0 + self.b1 + self.b2 + self.b3 + self.b4 + self.b5 + self.b6 + white * 0.115926
    }
}

/// Scales raw pink output into `[-1, 1]` without clipping in practice.
pub(super) const PINK_GAIN: f32 = 0.11;

/// Scales the first difference of raw pink (+3 dB/octave) to roughly the RMS
/// of violet noise.
pub(super) const BLUE_GAIN: f32 = 0.35;

/// Brown random-walk step per tick.
pub(super) const BROWN_STEP: f32 = 0.02;

/// Fold a value that stepped past ±1 back inside, so a random walk bounces off
/// the rails instead of sticking to them. Its values stay uniformly spread
/// over `[-1, 1]`.
pub(super) fn reflect(x: f32) -> f32 {
    if x > 1.0 {
        2.0 - x
    } else if x < -1.0 {
        -2.0 - x
    } else {
        x
    }
}

/// ±1 for a high or low rail.
pub(super) fn rail(high: bool) -> f32 {
    if high { 1.0 } else { -1.0 }
}

/// A fair coin flip, ±1.
pub(super) fn coin(rng: &mut LcgRng) -> f32 {
    rail(rng.next_unit() >= 0.5)
}

/// Mean of `x(1 + x(1/2 + x(1/3 + x/4)))` for `x` uniform in `[0.1, 1]`.
const DUST_MEAN_GAP: f32 = 0.883_138_3;

/// Gap to the next dust trigger, in average gaps. Mutable Instruments Beads
/// spaces its random grain triggers this way: a truncated series for
/// `−ln(1 − x)`, so gaps fall between about 0.12× and 2.36× the average and
/// short gaps are more likely than long ones.
pub(super) fn dust_gap(rng: &mut LcgRng) -> f32 {
    let x = 0.1 + 0.9 * rng.next_unit();
    x * (1.0 + x * (0.5 + x * (1.0 / 3.0 + x * 0.25))) / DUST_MEAN_GAP
}

/// Standard deviation of a sum of four uniform `[-1, 1)` draws.
const SUM4_STD: f32 = 1.154_700_5;

/// Bell-curve noise with σ = 1/3: a sum of four uniform draws (Irwin–Hall),
/// clipped to `[-1, 1]`, which it exceeds about 0.05% of the time.
pub(super) fn gaussian(rng: &mut LcgRng) -> f32 {
    let sum = rng.next_bipolar() + rng.next_bipolar() + rng.next_bipolar() + rng.next_bipolar();
    (sum / (3.0 * SUM4_STD)).clamp(-1.0, 1.0)
}

/// Length of the short-mode cycles that [`Lfsr::start_short`] starts on. The one
/// other short-mode cycle has 31 steps, and plays at three times the pitch.
pub(super) const LFSR_SHORT_CYCLE: u32 = 93;

/// Number of 93-step short-mode cycles.
const LFSR_SHORT_CYCLE_COUNT: usize = 352;

/// Period of the 15-bit LFSR's long mode: every nonzero state.
pub(super) const LFSR_LONG_PERIOD: u32 = 32767;

/// Ticks between neighbouring entries of [`LFSR_LONG_STARTS`], which bounds
/// the steps a long-mode start takes.
const LFSR_LONG_STRIDE: u32 = 64;

const _: () = assert!(LFSR_SHORT_CYCLE_COUNT >= PORT_MAX_CHANNELS);

const fn lfsr_step(state: u16, tap: u32) -> u16 {
    let bit = (state ^ (state >> tap)) & 1;
    (state >> 1) | (bit << 14)
}

/// The long-mode state at every [`LFSR_LONG_STRIDE`]th tick from state 1.
static LFSR_LONG_STARTS: [u16; LFSR_LONG_PERIOD.div_ceil(LFSR_LONG_STRIDE) as usize] = {
    let mut starts = [0; LFSR_LONG_PERIOD.div_ceil(LFSR_LONG_STRIDE) as usize];
    let mut state = 1;
    let mut i = 0;
    while i < starts.len() {
        starts[i] = state;
        let mut step = 0;
        while step < LFSR_LONG_STRIDE {
            state = lfsr_step(state, 1);
            step += 1;
        }
        i += 1;
    }
    starts
};

/// The lowest state on each 93-step short-mode cycle, in ascending order.
static LFSR_SHORT_STARTS: [u16; LFSR_SHORT_CYCLE_COUNT] = {
    let mut starts = [0; LFSR_SHORT_CYCLE_COUNT];
    let mut seen = [false; 1 << 15];
    let mut count = 0;
    let mut first = 1;
    while first < seen.len() {
        if !seen[first] {
            let mut state = first as u16;
            let mut len = 0;
            loop {
                seen[state as usize] = true;
                state = lfsr_step(state, 6);
                len += 1;
                if state as usize == first {
                    break;
                }
            }
            if len == LFSR_SHORT_CYCLE {
                starts[count] = first as u16;
                count += 1;
            }
        }
        first += 1;
    }
    assert!(count == LFSR_SHORT_CYCLE_COUNT);
    starts
};

/// Distance in long-mode ticks between two phases, the shorter way around.
#[cfg(test)]
pub(super) fn lfsr_phase_distance(a: u16, b: u16) -> u32 {
    let d = u32::from(a.abs_diff(b));
    d.min(LFSR_LONG_PERIOD - d)
}

/// Long-mode phase of channel `channel` among `channels` that all start
/// together: evenly spaced from `offset`, so neighbours sit at least
/// `32767 / channels` ticks apart.
pub(super) fn lfsr_even_phase(offset: u32, channel: usize, channels: usize) -> u16 {
    let spacing = LFSR_LONG_PERIOD / channels.max(1) as u32;
    ((offset + channel as u32 * spacing) % LFSR_LONG_PERIOD) as u16
}

/// The long-mode phases taken by a module's channels, for placing channels
/// that join running ones.
pub(super) struct LfsrPhases {
    taken: [u16; PORT_MAX_CHANNELS],
    len: usize,
}

impl LfsrPhases {
    pub(super) fn new(running: impl Iterator<Item = u16>) -> Self {
        let mut taken = [0; PORT_MAX_CHANNELS];
        let mut len = 0;
        for phase in running.take(PORT_MAX_CHANNELS) {
            taken[len] = phase;
            len += 1;
        }
        Self { taken, len }
    }

    /// Claim a phase for one more channel: the middle of the widest gap
    /// between taken phases. With `n` phases taken, it lands at least
    /// `32767 / (2 * n)` ticks from every one of them.
    pub(super) fn claim(&mut self) -> u16 {
        let taken = &mut self.taken[..self.len];
        taken.sort_unstable();
        let phase = match *taken {
            [] => 0,
            [only] => (u32::from(only) + LFSR_LONG_PERIOD / 2) % LFSR_LONG_PERIOD,
            [.., last] => {
                let (mut from, mut widest, mut prev) = (last, 0, last);
                for &phase in taken.iter() {
                    let gap =
                        (u32::from(phase) + LFSR_LONG_PERIOD - u32::from(prev)) % LFSR_LONG_PERIOD;
                    if gap > widest {
                        (from, widest) = (prev, gap);
                    }
                    prev = phase;
                }
                (u32::from(from) + widest / 2) % LFSR_LONG_PERIOD
            }
        } as u16;
        if self.len < PORT_MAX_CHANNELS {
            self.taken[self.len] = phase;
            self.len += 1;
        }
        phase
    }
}

/// A short-mode loop: one of the 93-step cycles and a starting point on it.
#[derive(Default, Clone, Copy)]
pub(super) struct LfsrLoop {
    /// Index into [`LFSR_SHORT_STARTS`].
    cycle: usize,
    /// Ticks past the cycle's lowest state, below [`LFSR_SHORT_CYCLE`].
    rotation: u32,
}

impl LfsrLoop {
    /// The loop of channel `channel` in a module with random seed `seed`.
    /// Channels of one module play distinct cycles; the seed also picks where
    /// on its cycle every channel starts.
    pub(super) fn for_channel(seed: u32, channel: usize) -> Self {
        Self {
            cycle: (seed as usize + channel) % LFSR_SHORT_CYCLE_COUNT,
            rotation: seed / LFSR_SHORT_CYCLE_COUNT as u32 % LFSR_SHORT_CYCLE,
        }
    }
}

/// 15-bit linear-feedback shift register in the style of the NES noise
/// channel. Long mode (tap 1) cycles through all 32767 nonzero states; short
/// mode (tap 6) loops a 93-step cycle, which sounds pitched and metallic.
pub(super) struct Lfsr {
    state: u16,
    /// Position in long mode as ticks from state 1, below
    /// [`LFSR_LONG_PERIOD`]. Long-mode steps advance it; short mode leaves it,
    /// so long mode resumes from it.
    phase: u16,
}

impl Default for Lfsr {
    fn default() -> Self {
        Self { state: 1, phase: 0 }
    }
}

impl Lfsr {
    pub(super) fn phase(&self) -> u16 {
        self.phase
    }

    pub(super) fn set_phase(&mut self, phase: u16) {
        self.phase = phase;
    }

    /// Jump to the state at the current long-mode phase, in fewer than
    /// [`LFSR_LONG_STRIDE`] steps.
    pub(super) fn start_long(&mut self) {
        self.state = LFSR_LONG_STARTS[usize::from(self.phase) / LFSR_LONG_STRIDE as usize];
        for _ in 0..u32::from(self.phase) % LFSR_LONG_STRIDE {
            self.state = lfsr_step(self.state, 1);
        }
    }

    /// Jump to the start of `lfsr_loop`, in fewer than [`LFSR_SHORT_CYCLE`]
    /// steps.
    pub(super) fn start_short(&mut self, lfsr_loop: LfsrLoop) {
        self.state = LFSR_SHORT_STARTS[lfsr_loop.cycle];
        for _ in 0..lfsr_loop.rotation {
            self.state = lfsr_step(self.state, 6);
        }
    }

    pub(super) fn step_long(&mut self) -> f32 {
        self.state = lfsr_step(self.state, 1);
        self.phase += 1;
        if u32::from(self.phase) == LFSR_LONG_PERIOD {
            self.phase = 0;
        }
        rail(self.state & 1 == 1)
    }

    pub(super) fn step_short(&mut self) -> f32 {
        self.state = lfsr_step(self.state, 6);
        rail(self.state & 1 == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every long-mode phase starts at the state that many ticks from state 1,
    /// and stepping keeps the phase in step with the state.
    #[test]
    fn lfsr_long_start_reaches_its_phase() {
        let mut expected = 1;
        let mut running = Lfsr::default();
        for phase in 0..LFSR_LONG_PERIOD as u16 {
            let mut lfsr = Lfsr::default();
            lfsr.set_phase(phase);
            lfsr.start_long();
            assert_eq!(lfsr.state, expected, "phase {phase}");
            assert_eq!((running.state, running.phase), (expected, phase));
            expected = lfsr_step(expected, 1);
            running.step_long();
        }
        assert_eq!(expected, 1, "long mode has period {LFSR_LONG_PERIOD}");
        assert_eq!(running.phase, 0);
    }

    /// A claimed phase sits in the middle of the widest gap, at least
    /// `32767 / (2 * n)` ticks from each of the `n` taken phases.
    #[test]
    fn lfsr_phase_claims_split_the_widest_gap() {
        let mut phases = LfsrPhases::new([100].into_iter());
        assert_eq!(phases.claim(), 100 + 16383);
        let mut phases = LfsrPhases::new([32_000, 500, 16_000].into_iter());
        assert_eq!(phases.claim(), 24_000);
        for taken in [vec![7], vec![0, 1], vec![5, 10_000, 20_000, 32_760]] {
            let n = taken.len() as u32;
            let mut phases = LfsrPhases::new(taken.iter().copied());
            let phase = phases.claim();
            for &t in &taken {
                assert!(lfsr_phase_distance(phase, t) >= LFSR_LONG_PERIOD / (2 * n));
            }
        }
    }

    /// The short-mode table holds the lowest state of every 93-step cycle,
    /// and the states it leaves out form the single 31-step cycle.
    #[test]
    fn lfsr_short_starts_cover_every_93_step_cycle() {
        let mut covered = std::collections::HashSet::new();
        for (cycle, &start) in LFSR_SHORT_STARTS.iter().enumerate() {
            let mut lfsr = Lfsr::default();
            lfsr.start_short(LfsrLoop { cycle, rotation: 0 });
            assert_eq!(lfsr.state, start);
            let mut states = vec![start];
            while lfsr_step(*states.last().unwrap(), 6) != start {
                states.push(lfsr_step(*states.last().unwrap(), 6));
            }
            assert_eq!(states.len(), LFSR_SHORT_CYCLE as usize, "cycle {cycle}");
            assert_eq!(states.iter().min(), Some(&start), "cycle {cycle}");
            assert!(states.into_iter().all(|s| covered.insert(s)));
        }
        let rest: Vec<u16> = (1..=0x7fff).filter(|s| !covered.contains(s)).collect();
        assert_eq!(rest.len(), 31);
        let mut state = rest[0];
        for _ in 0..31 {
            state = lfsr_step(state, 6);
            assert!(rest.contains(&state));
        }
        assert!(LFSR_SHORT_STARTS.is_sorted());
    }

    #[test]
    fn gaussian_is_centered_and_bounded() {
        let mut rng = LcgRng::default();
        rng.seed(0xabc, 0);
        let n = 100_000;
        let samples: Vec<f32> = (0..n).map(|_| gaussian(&mut rng)).collect();
        let mean = samples.iter().sum::<f32>() / n as f32;
        let std = (samples.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / n as f32).sqrt();
        assert!(mean.abs() < 0.01, "mean {mean}");
        assert!((std - 1.0 / 3.0).abs() < 0.01, "std {std}");
        assert!(samples.iter().all(|s| s.abs() <= 1.0));
    }
}
