//! Per-channel noise sources for `$noise`. Every source draws one value per
//! clock tick, normalized to `[-1, 1]`.

use crate::dsp::utils::rng::LcgRng;

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
pub(super) const BROWN_STEP: f32 = 0.024;

/// Per-tick decay that pulls the brown walk back toward 0, keeping its RMS
/// near 0.3 so it rarely reaches the rails.
pub(super) const BROWN_LEAK: f32 = 0.999;

/// Standard deviation of a sum of four uniform `[-1, 1)` draws.
const SUM4_STD: f32 = 1.154_700_5;

/// Bell-curve noise with σ = 1/3: a sum of four uniform draws (Irwin–Hall),
/// clipped to `[-1, 1]`, which it exceeds about 0.05% of the time.
pub(super) fn gaussian(rng: &mut LcgRng) -> f32 {
    let sum = rng.next_bipolar() + rng.next_bipolar() + rng.next_bipolar() + rng.next_bipolar();
    (sum / (3.0 * SUM4_STD)).clamp(-1.0, 1.0)
}

/// Length of the 15-bit LFSR's short-mode cycle that contains state 1.
const LFSR_SHORT_CYCLE: u32 = 93;

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
    /// Restart at a random point of the 93-step short cycle. Every state on
    /// that cycle is nonzero, so long mode runs from it as well.
    pub(super) fn reset(&mut self, rng: &mut LcgRng) {
        self.state = 1;
        let skip = (rng.next_unit() * LFSR_SHORT_CYCLE as f32) as u32;
        for _ in 0..skip {
            self.step(1);
        }
    }

    pub(super) fn step(&mut self, tap: u32) -> f32 {
        let bit = (self.state ^ (self.state >> tap)) & 1;
        self.state = (self.state >> 1) | (bit << 14);
        if self.state & 1 == 1 { 1.0 } else { -1.0 }
    }
}

/// Catmull–Rom spline through `p0..p3`, evaluated between `p1` (t = 0) and
/// `p2` (t = 1).
pub(super) fn catmull_rom(p: [f32; 4], t: f32) -> f32 {
    let [p0, p1, p2, p3] = p;
    let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = 0.5 * (p2 - p0);
    ((a * t + b) * t + c) * t + p1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfsr_short_mode_repeats_every_93_ticks() {
        let mut rng = LcgRng::default();
        rng.seed(0x5eed, 0);
        let mut lfsr = Lfsr::default();
        lfsr.reset(&mut rng);
        let first: Vec<f32> = (0..93).map(|_| lfsr.step(6)).collect();
        let second: Vec<f32> = (0..93).map(|_| lfsr.step(6)).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn catmull_rom_hits_its_knots() {
        let p = [0.3, -0.2, 0.8, 0.1];
        assert!((catmull_rom(p, 0.0) - p[1]).abs() < 1e-6);
        assert!((catmull_rom(p, 1.0) - p[2]).abs() < 1e-6);
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
