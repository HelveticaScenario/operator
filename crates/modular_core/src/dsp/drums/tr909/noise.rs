//! The 909's noise generator, shared by the voices that use noise.

use super::circuit::RAIL;
use crate::dsp::utils::rng::LcgRng;

/// The 909's noise generator: two 4006 shift registers chained into 32
/// stages with exclusive-or feedback, clocked by an RC oscillator on IC31
/// (R185 33 kΩ, C47 100 pF), its output swinging the full 15 V.
#[derive(Clone, Copy)]
pub struct Noise {
    register: u32,
    /// Clock phase, in clock periods.
    phase: f32,
    seeded: bool,
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            register: TOP,
            phase: 0.0,
            seeded: false,
        }
    }
}

/// A 31-stage maximal-length register stands in for the 909's 32 stages:
/// its sequence repeats only after 2^31 clocks, some hours.
const STAGES_MASK: u32 = (1 << 31) - 1;
const TOP: u32 = 1 << 30;

/// The shift registers' clock, Hz.
const NOISE_CLOCK_HZ: f32 = 1.0 / (2.2 * 33.0e3 * 100.0e-12);

impl Noise {
    /// Seeds the register unless it already is. The register rides patch
    /// updates with its channel, so a carried-over channel keeps its
    /// sequence and a channel added by an update gets its own.
    pub fn seed(&mut self, base: u64, stream: usize) {
        if self.seeded {
            return;
        }
        self.seeded = true;
        let mut rng = LcgRng::default();
        rng.seed(base, stream);
        // Any state but all zeros runs; the top bit keeps it off zero.
        self.register = (((rng.next_bipolar() * 1.0e9) as i64 as u32) & STAGES_MASK) | TOP;
    }

    /// Clock periods per sample at `sample_rate`.
    pub fn rate(sample_rate: f32) -> f32 {
        NOISE_CLOCK_HZ / sample_rate
    }

    /// The register's output averaged over one sample, in volts: the
    /// filters that follow it are all far below the clock.
    #[inline]
    pub fn next(&mut self, rate: f32) -> f32 {
        let end = self.phase + rate;
        let mut high = 0.0;
        let mut t = self.phase;
        while t < end {
            let edge = t.floor() + 1.0;
            let span = edge.min(end) - t;
            if self.register & 1 != 0 {
                high += span;
            }
            if edge <= end {
                // Feedback from stages 31 and 28.
                let bit = (self.register ^ (self.register >> 3)) & 1;
                self.register = (self.register >> 1) | (bit << 30);
            }
            t = edge;
        }
        self.phase = end.fract();
        RAIL * high / rate
    }
}
