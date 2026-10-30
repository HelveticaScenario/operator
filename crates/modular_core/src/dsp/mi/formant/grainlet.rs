//! `plaits::GrainletOscillator`, two at a time: a phase-distorted single-cycle
//! sine window (the carrier) times a sine at the formant frequency, restarted
//! on every carrier cycle. The two grainlets of Plaits' granular formant
//! engine share their carrier, so it is computed once for both.

use crate::dsp::mi::core::control::Ramp;
use crate::dsp::mi::core::phase::{fraction, wrap_below};
use crate::dsp::mi::core::polyblep::{next_blep_sample, this_blep_sample};
use crate::dsp::mi::core::sine::sine;

/// Control targets for one tick.
pub struct GrainletControls {
    /// Formant frequencies in cycles per sample.
    pub formant: [f32; 2],
    /// Carrier window shape, 0–1.
    pub shape: f32,
    /// How much of the bare carrier bleeds through, 0–1.
    pub bleed: f32,
}

#[derive(Clone, Copy, Default)]
pub struct GrainletPair {
    carrier_phase: f32,
    formant_phase: [f32; 2],
    next_sample: [f32; 2],
    formant_frequency: [Ramp; 2],
    shape: Ramp,
    bleed: Ramp,
}

impl GrainletPair {
    /// Ramps to `controls` over a tick of `length` samples, or jumps there when
    /// `length` is `None`.
    pub fn set(&mut self, controls: &GrainletControls, length: Option<f32>) {
        for (ramp, &f) in self.formant_frequency.iter_mut().zip(&controls.formant) {
            ramp.set(f, length);
        }
        self.shape.set(controls.shape, length);
        self.bleed.set(controls.bleed, length);
    }

    /// One sample of both grainlets at carrier frequency `f0` (cycles per
    /// sample, finite; negative runs the carrier backward).
    #[inline(always)]
    pub fn step(&mut self, f0: f32) -> f32 {
        let mut this_sample = self.next_sample;
        let mut next_sample = [0.0; 2];
        let f1 = [
            self.formant_frequency[0].next_value(),
            self.formant_frequency[1].next_value(),
        ];

        self.carrier_phase += f0;
        // Backward, a grain plays in reverse: its formant runs backward too and
        // restarts at the phase a forward grain ends on.
        if self.carrier_phase >= 1.0 {
            self.carrier_phase -= 1.0;
            let reset_time = self.carrier_phase / f0;
            let carrier_before = carrier(1.0, self.shape.subsample(1.0 - reset_time));
            let bleed_before = self.bleed.subsample(1.0 - reset_time);
            let after = grainlet(
                carrier(0.0, self.shape.subsample(1.0)),
                0.0,
                self.bleed.subsample(1.0),
            );
            for i in 0..2 {
                let before = grainlet(
                    carrier_before,
                    self.formant_phase[i] + (1.0 - reset_time) * f1[i],
                    bleed_before,
                );
                let discontinuity = after - before;
                this_sample[i] += discontinuity * this_blep_sample(reset_time);
                next_sample[i] += discontinuity * next_blep_sample(reset_time);
                self.formant_phase[i] = reset_time * f1[i];
            }
        } else if self.carrier_phase < 0.0 {
            self.carrier_phase += 1.0;
            let reset_time = (1.0 - self.carrier_phase) / -f0;
            let carrier_before = carrier(0.0, self.shape.subsample(1.0 - reset_time));
            let bleed_before = self.bleed.subsample(1.0 - reset_time);
            let carrier_after = carrier(1.0, self.shape.subsample(1.0));
            let bleed_after = self.bleed.subsample(1.0);
            for i in 0..2 {
                let end = fraction(f1[i] / -f0);
                let before = grainlet(
                    carrier_before,
                    self.formant_phase[i] - (1.0 - reset_time) * f1[i],
                    bleed_before,
                );
                let after = grainlet(carrier_after, end, bleed_after);
                let discontinuity = after - before;
                this_sample[i] += discontinuity * this_blep_sample(reset_time);
                next_sample[i] += discontinuity * next_blep_sample(reset_time);
                self.formant_phase[i] = wrap_below(end - reset_time * f1[i]);
            }
        } else if f0 >= 0.0 {
            for (phase, f) in self.formant_phase.iter_mut().zip(f1) {
                *phase += f;
                if *phase >= 1.0 {
                    *phase -= 1.0;
                }
            }
        } else {
            for (phase, f) in self.formant_phase.iter_mut().zip(f1) {
                *phase = wrap_below(*phase - f);
            }
        }

        let carrier = carrier(self.carrier_phase, self.shape.next_value());
        let bleed = self.bleed.next_value();
        for (next, &phase) in next_sample.iter_mut().zip(&self.formant_phase) {
            *next += grainlet(carrier, phase, bleed);
        }
        self.next_sample = next_sample;
        this_sample[0] + this_sample[1]
    }
}

/// The window: a sine cycle whose phase is distorted by `shape` (0–1), from a
/// narrow pulse through a skewed bump to a broad, truncated one.
#[inline(always)]
fn carrier(phase: f32, shape: f32) -> f32 {
    let shape = shape * 3.0;
    let shape_integral = shape as i32;
    let t = 1.0 - (shape - shape_integral as f32);
    let phase = if shape_integral == 0 {
        (phase * (1.0 + t * t * t * 15.0)).min(1.0) + 0.75
    } else if shape_integral == 1 {
        let breakpoint = 0.001 + 0.499 * t * t * t;
        let phase = if phase < breakpoint {
            phase * (0.5 / breakpoint)
        } else {
            0.5 + (phase - breakpoint) * 0.5 / (1.0 - breakpoint)
        };
        phase + 0.75
    } else {
        let t = 1.0 - t;
        (0.25 + phase * (0.5 + t * t * t * 14.5)).min(0.75)
    };
    (sine(phase) + 1.0) * 0.25
}

#[inline(always)]
fn grainlet(carrier: f32, formant_phase: f32, bleed: f32) -> f32 {
    carrier * (sine(formant_phase) + bleed) / (1.0 + bleed)
}
