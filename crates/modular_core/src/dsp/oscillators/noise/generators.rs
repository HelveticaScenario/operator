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

/// Standard deviation of a sum of four uniform `[-1, 1)` draws.
const SUM4_STD: f32 = 1.154_700_5;

/// Bell-curve noise with σ = 1/3: a sum of four uniform draws (Irwin–Hall),
/// clipped to `[-1, 1]`, which it exceeds about 0.05% of the time.
pub(super) fn gaussian(rng: &mut LcgRng) -> f32 {
    let sum = rng.next_bipolar() + rng.next_bipolar() + rng.next_bipolar() + rng.next_bipolar();
    (sum / (3.0 * SUM4_STD)).clamp(-1.0, 1.0)
}

/// Length of the short-mode cycles that [`Lfsr::start`] starts on. The one
/// other short-mode cycle has 31 steps, and plays at three times the pitch.
pub(super) const LFSR_SHORT_CYCLE: u32 = 93;

/// Number of 93-step short-mode cycles.
const LFSR_SHORT_CYCLE_COUNT: usize = 352;

/// Period of the 15-bit LFSR's long mode: every nonzero state.
pub(super) const LFSR_LONG_PERIOD: u32 = 32767;

/// Ticks between neighbouring entries of [`LFSR_LONG_STARTS`], which bounds
/// the steps a long-mode start takes.
const LFSR_LONG_STRIDE: u32 = 64;

/// Long-mode ticks between the starts of neighbouring channels of one module.
const LFSR_CHANNEL_SPACING: u32 = LFSR_LONG_PERIOD / PORT_MAX_CHANNELS as u32;

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

/// Where a channel's LFSR starts in each mode.
#[derive(Default, Clone, Copy)]
pub(super) struct LfsrStart {
    /// Long-mode ticks from state 1, below [`LFSR_LONG_PERIOD`].
    phase: u32,
    /// Index into [`LFSR_SHORT_STARTS`].
    cycle: usize,
}

impl LfsrStart {
    /// The start of channel `channel` in a module with random offset `offset`.
    /// Channels of one module sit at least [`LFSR_CHANNEL_SPACING`] ticks
    /// apart in long mode and on distinct 93-step cycles in short mode, so no
    /// channel is a delayed copy of another.
    pub(super) fn for_channel(offset: u32, channel: usize) -> Self {
        Self {
            phase: (offset + channel as u32 * LFSR_CHANNEL_SPACING) % LFSR_LONG_PERIOD,
            cycle: (offset as usize + channel) % LFSR_SHORT_CYCLE_COUNT,
        }
    }
}

/// 15-bit linear-feedback shift register in the style of the NES noise
/// channel. Long mode (tap 1) cycles through all 32767 nonzero states; short
/// mode (tap 6) loops a 93-step cycle, which sounds pitched and metallic.
pub(super) struct Lfsr {
    state: u16,
}

impl Default for Lfsr {
    fn default() -> Self {
        Self { state: 1 }
    }
}

impl Lfsr {
    /// Jump to `start` for the given mode. A long-mode start takes fewer than
    /// [`LFSR_LONG_STRIDE`] steps; a short-mode start takes none.
    pub(super) fn start(&mut self, start: LfsrStart, short: bool) {
        if short {
            self.state = LFSR_SHORT_STARTS[start.cycle];
        } else {
            self.state = LFSR_LONG_STARTS[(start.phase / LFSR_LONG_STRIDE) as usize];
            for _ in 0..start.phase % LFSR_LONG_STRIDE {
                self.state = lfsr_step(self.state, 1);
            }
        }
    }

    pub(super) fn step(&mut self, tap: u32) -> f32 {
        self.state = lfsr_step(self.state, tap);
        rail(self.state & 1 == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every long-mode phase starts at the state that many ticks from state 1.
    #[test]
    fn lfsr_long_start_reaches_its_phase() {
        let mut expected = 1;
        for phase in 0..LFSR_LONG_PERIOD {
            let mut lfsr = Lfsr::default();
            lfsr.start(LfsrStart { phase, cycle: 0 }, false);
            assert_eq!(lfsr.state, expected, "phase {phase}");
            expected = lfsr_step(expected, 1);
        }
        assert_eq!(expected, 1, "long mode has period {LFSR_LONG_PERIOD}");
    }

    /// The short-mode table holds the lowest state of every 93-step cycle,
    /// and the states it leaves out form the single 31-step cycle.
    #[test]
    fn lfsr_short_starts_cover_every_93_step_cycle() {
        let mut covered = std::collections::HashSet::new();
        for (cycle, &start) in LFSR_SHORT_STARTS.iter().enumerate() {
            let mut lfsr = Lfsr::default();
            lfsr.start(LfsrStart { phase: 0, cycle }, true);
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
