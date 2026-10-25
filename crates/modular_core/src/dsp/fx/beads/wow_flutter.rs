//! Dirty-tape wow and flutter: a short modulated delay driven by a slow wow
//! LFO, a faster flutter LFO and a random modulator, then a one-pole low-pass.

use super::primitives::{Ramp, SineOscillator};
use crate::dsp::utils::rng::LcgRng;

/// Write `x` into the circular `line` (a power-of-two length) at
/// `write_ptr`, step backwards, and read it back `delay` samples later with
/// linear interpolation.
#[inline]
pub fn write_read(line: &mut [f32], write_ptr: &mut usize, x: f32, delay: f32) -> f32 {
    let mask = line.len() - 1;
    line[*write_ptr] = x;
    *write_ptr = (*write_ptr + mask) & mask;
    let integral = delay as usize;
    let fractional = delay - integral as f32;
    let a = line[(*write_ptr + integral) & mask];
    let b = line[(*write_ptr + integral + 1) & mask];
    a + (b - a) * fractional
}

/// The wow, flutter and random modulators driving any number of delay
/// lines. Delays are in samples at 24 kHz, the rate the hardware runs them
/// at.
#[derive(Clone, Copy, Default)]
pub struct WowModulation {
    one_hertz: f32,
    random_frequency: f32,
    delay: Ramp,
    random_modulation: SineOscillator,
    wow: SineOscillator,
    flutter: SineOscillator,
}

impl WowModulation {
    pub fn init(&mut self, sample_rate: f32) {
        *self = Self::default();
        self.one_hertz = 1.0 / sample_rate;
        self.random_frequency = 0.125 * self.one_hertz;
        self.delay.reset(0.0);
    }

    /// Advance the modulators by one control block of `block_size` samples.
    pub fn start_block(&mut self, block_size: usize, rng: &mut LcgRng) {
        let size = block_size as f32;
        let previous_phase = self.random_modulation.phase();
        let random_modulation = self.random_modulation.render(self.random_frequency * size);
        if self.random_modulation.phase() >= 0.25 && previous_phase < 0.25 {
            self.random_frequency = self.one_hertz * (0.05 + rng.next_unit() * 0.2);
        }

        let fm = 0.5 + 1.5 * random_modulation * random_modulation;
        let amount = 1.0 - 0.4 * random_modulation;
        let wow = self.wow.render(self.one_hertz * 0.23 * size * fm);
        let flutter = self.flutter.render(self.one_hertz * 1.99 * size);
        let delay = 1.0 + (105.0 * wow + 17.0 * flutter) * amount;
        self.delay.start(delay, block_size);
    }

    /// The delay for the next sample, shared by every line it drives.
    #[inline]
    pub fn next(&mut self) -> f32 {
        self.delay.next()
    }
}
