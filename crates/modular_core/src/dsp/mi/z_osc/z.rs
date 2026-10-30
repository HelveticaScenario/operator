//! `plaits::ZOscillator`: a sine at the cutoff frequency, restarted every half
//! carrier cycle and multiplied by a falling ramp and a carrier contour, which
//! sounds like a resonant filter swept by a waveform.

use crate::dsp::mi::core::control::Ramp;
use crate::dsp::mi::core::phase::{fraction, wrap_below};
use crate::dsp::mi::core::polyblep::{next_blep_sample, this_blep_sample};
use crate::dsp::mi::core::sine::sine;

/// Control targets for one tick.
pub struct ZControls {
    /// Formant (cutoff) frequency in cycles per sample.
    pub formant: f32,
    /// Carrier contour, 0–1.
    pub shape: f32,
    /// Filter response, 0–1.
    pub mode: f32,
}

#[derive(Clone, Copy, Default)]
pub struct ZOscillator {
    carrier_phase: f32,
    discontinuity_phase: f32,
    formant_phase: f32,
    next_sample: f32,
    formant_frequency: Ramp,
    shape: Ramp,
    mode: Ramp,
}

impl ZOscillator {
    /// Ramps to `controls` over a tick of `length` samples, or jumps there when
    /// `length` is `None`.
    pub fn set(&mut self, controls: &ZControls, length: Option<f32>) {
        self.formant_frequency.set(controls.formant, length);
        self.shape.set(controls.shape, length);
        self.mode.set(controls.mode, length);
    }

    /// One sample at carrier frequency `f0` (cycles per sample, finite;
    /// negative runs the carrier backward).
    #[inline(always)]
    pub fn step(&mut self, f0: f32) -> f32 {
        let mut this_sample = self.next_sample;
        let mut next_sample = 0.0;
        let f1 = self.formant_frequency.next_value();

        self.discontinuity_phase += 2.0 * f0;
        self.carrier_phase += f0;
        // The discontinuity phase wraps every half carrier cycle; a carrier near
        // 0 or 1 marks the end of a whole cycle, whichever of the two phases
        // rounding wrapped first. Backward, a half cycle plays in reverse: its
        // formant runs backward too and restarts at the phase a forward half
        // cycle ends on.
        let whole_cycle = (self.carrier_phase - 0.5).abs() >= 0.25;
        // A backward wrap can round to exactly 1, so only a forward carrier
        // resets at 1.
        if self.discontinuity_phase >= 1.0 && f0 > 0.0 {
            self.discontinuity_phase -= 1.0;
            let reset_time = self.discontinuity_phase / (2.0 * f0);
            let (c_before, c_after) = if whole_cycle { (1.0, 0.0) } else { (0.5, 0.5) };
            let before = z(
                c_before,
                1.0,
                self.formant_phase + (1.0 - reset_time) * f1,
                self.shape.subsample(1.0 - reset_time),
                self.mode.subsample(1.0 - reset_time),
            );
            let after = z(
                c_after,
                0.0,
                0.0,
                self.shape.subsample(1.0),
                self.mode.subsample(1.0),
            );
            let discontinuity = after - before;
            this_sample += discontinuity * this_blep_sample(reset_time);
            next_sample += discontinuity * next_blep_sample(reset_time);
            self.formant_phase = reset_time * f1;
            if whole_cycle {
                self.carrier_phase = self.discontinuity_phase * 0.5;
            }
        } else if self.discontinuity_phase < 0.0 {
            self.discontinuity_phase += 1.0;
            let reset_time = (1.0 - self.discontinuity_phase) / (-2.0 * f0);
            // Through a half cycle, the carrier lands just below 0.5, outside
            // the contour's upper half.
            let (c_before, c_after) = if whole_cycle {
                (0.0, 1.0)
            } else {
                (0.5, 0.5_f32.next_down())
            };
            let end = fraction(f1 / (-2.0 * f0));
            let before = z(
                c_before,
                0.0,
                self.formant_phase - (1.0 - reset_time) * f1,
                self.shape.subsample(1.0 - reset_time),
                self.mode.subsample(1.0 - reset_time),
            );
            let after = z(
                c_after,
                1.0,
                end,
                self.shape.subsample(1.0),
                self.mode.subsample(1.0),
            );
            let discontinuity = after - before;
            this_sample += discontinuity * this_blep_sample(reset_time);
            next_sample += discontinuity * next_blep_sample(reset_time);
            self.formant_phase = wrap_below(end - reset_time * f1);
            if whole_cycle {
                self.carrier_phase = 0.5 + self.discontinuity_phase * 0.5;
            }
        } else if f0 >= 0.0 {
            self.formant_phase += f1;
            if self.formant_phase >= 1.0 {
                self.formant_phase -= 1.0;
            }
        } else {
            self.formant_phase = wrap_below(self.formant_phase - f1);
        }

        if self.carrier_phase >= 1.0 {
            self.carrier_phase -= 1.0;
        } else if self.carrier_phase < 0.0 {
            self.carrier_phase += 1.0;
        }

        next_sample += z(
            self.carrier_phase,
            self.discontinuity_phase,
            self.formant_phase,
            self.shape.next_value(),
            self.mode.next_value(),
        );
        self.next_sample = next_sample;
        this_sample
    }
}

#[inline(always)]
fn z(c: f32, d: f32, f: f32, shape: f32, mode: f32) -> f32 {
    let mut ramp_down = 0.5 * (1.0 + sine(0.5 * d + 0.25));

    let (offset, phase_shift) = if mode < 0.333 {
        (1.0, 0.25 + mode * 1.50)
    } else if mode < 0.666 {
        let phase_shift = 0.7495 - (mode - 0.33) * 0.75;
        (-sine(phase_shift), phase_shift)
    } else {
        (0.001, 0.7495 - (mode - 0.33) * 0.75)
    };

    let discontinuity = sine(f + phase_shift);
    let contour = if shape < 0.5 {
        let shape = shape * 2.0;
        if c >= 0.5 {
            ramp_down *= shape;
        }
        1.0 + (sine(c + 0.25) - 1.0) * shape
    } else {
        sine(c + shape * 0.5)
    };
    (ramp_down * (offset + discontinuity) - offset) * contour
}
