//! The delay's read side: each tap from the controller read straight and
//! through a two-grain pitch shifter, crossfaded by how far `speed` is from
//! 1, then through the low- or high-pass `bandwidth` filter.

use super::buffer::{Codec, GrainBuffer};
use super::delay_controller::{DelayTap, RENDERED_TAPS};
use super::primitives::Svf;

/// Pitch shifter window in samples, in the 10-bit fixed point of tap delays.
const WINDOW: i32 = 2048 << 10;
const HALF_WINDOW: i32 = WINDOW >> 1;

#[inline(always)]
fn read<C: Codec>(buffer: &GrainBuffer, delay: i32) -> f32 {
    buffer.read_hermite::<C>(delay >> 10, (delay & 1023) as f32 / 1024.0)
}

/// Two triangle-windowed read heads half a window apart, sliding through the
/// window at `speed − 1` to shift the tap's pitch.
#[derive(Clone, Copy, Default)]
struct PitchShifter {
    /// Head position within the window, in 10-bit fixed point.
    phase: i32,
}

impl PitchShifter {
    /// Slide the heads one sample at `speed`.
    #[inline(always)]
    fn advance(&mut self, speed: f32) {
        self.phase += (1024.0 * (speed - 1.0)) as i32;
        if self.phase >= WINDOW {
            self.phase -= WINDOW;
        } else if self.phase < 0 {
            self.phase += WINDOW;
        }
    }

    #[inline(always)]
    fn read<C: Codec>(&mut self, buffer: &GrainBuffer, delay: i32, speed: f32) -> f32 {
        self.advance(speed);
        // The heads trail the tap by up to half a window, kept inside the
        // recording when the tap is near the longest delay.
        let delay = (WINDOW + 1)
            .max(delay + HALF_WINDOW)
            .min((buffer.max_delay() << 10) - 1);
        let phase = self.phase as f32 / WINDOW as f32;
        let triangle = if phase < 0.5 {
            2.0 * phase
        } else {
            2.0 - 2.0 * phase
        };
        let head_1 = read::<C>(buffer, delay - self.phase);
        let head_2 = read::<C>(buffer, delay - (self.phase + HALF_WINDOW) % WINDOW);
        head_2 + (head_1 - head_2) * triangle
    }
}

#[derive(Default)]
pub struct MultitapDelay {
    shifters: [PitchShifter; RENDERED_TAPS],
    filter: Svf,
    /// The `bandwidth` the filter is tuned to, and the wet gain it implies.
    tuned: Option<f32>,
    wet_amount: f32,
}

impl MultitapDelay {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The sum of `taps` read from `buffer` at `speed`, filtered by
    /// `bandwidth` (0–1; 0.5 leaves it unfiltered).
    #[inline]
    pub fn render<C: Codec>(
        &mut self,
        buffer: &GrainBuffer,
        taps: &[DelayTap; RENDERED_TAPS],
        speed: f32,
        bandwidth: f32,
    ) -> f32 {
        let shifted = (100.0 * ((speed - 1.0).abs() - 0.01)).clamp(0.0, 1.0);
        let mut mix = 0.0;
        for (tap, shifter) in taps.iter().zip(self.shifters.iter_mut()) {
            if tap.delay == 0 {
                continue;
            }
            // The shifter's heads keep their place while unheard.
            if tap.gain == 0.0 {
                shifter.advance(speed);
                continue;
            }
            let x = if shifted >= 1.0 {
                shifter.read::<C>(buffer, tap.delay, speed)
            } else {
                let straight = read::<C>(buffer, tap.delay);
                if shifted > 0.0 {
                    let pitched = shifter.read::<C>(buffer, tap.delay, speed);
                    straight + (pitched - straight) * shifted
                } else {
                    shifter.advance(speed);
                    straight
                }
            };
            mix += x * tap.gain;
        }

        if self.tuned != Some(bandwidth) {
            self.tune(bandwidth);
        }
        let (lp, hp) = self.filter.process(mix);
        let wet = if bandwidth < 0.5 { lp } else { hp };
        mix + (wet - mix) * self.wet_amount
    }

    /// Low-pass from 7 to 2 octaves below Nyquist, high-pass from 8 to 3.
    fn tune(&mut self, bandwidth: f32) {
        let amount = (2.0 * bandwidth - 1.0).abs();
        let octave = if bandwidth < 0.5 {
            -7.0 + bandwidth * 10.0
        } else {
            -8.0 + (bandwidth - 0.5) * 10.0
        };
        let f = 0.49 * octave.exp2();
        self.filter.set_f_q(f, 1.0 + 16.0 * amount * f);
        let wet_sqrt = amount * (2.0 - amount);
        self.wet_amount = wet_sqrt * (2.0 - wet_sqrt);
        self.tuned = Some(bandwidth);
    }
}
