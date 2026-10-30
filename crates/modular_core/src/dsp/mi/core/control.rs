//! Plaits computes its controls once per 12-sample block at 48 kHz and ramps
//! them across the block. The ports keep that 4 kHz control rate at any sample
//! rate without buffering output: a [`ControlClock`] says when a tick is due,
//! the module recomputes its controls there, and each [`Ramp`] reaches its new
//! target on the sample before the next tick.

use std::ops::Range;

/// Control ticks per second.
pub const CONTROL_RATE: f32 = 4000.0;

/// Schedules control ticks. Tick lengths are whole samples whose average is
/// `sample_rate / CONTROL_RATE`, so at 48 kHz every tick is 12 samples, aligned
/// with Plaits' blocks from the first sample.
#[derive(Clone, Copy, Default)]
pub struct ControlClock {
    /// Samples left in the current tick.
    remaining: u32,
    /// Scheduling error carried between ticks, in samples × `CONTROL_RATE`.
    phase: f32,
}

impl ControlClock {
    /// Advances by up to `max` samples, stopping before the next tick. Returns
    /// the length of the tick that starts at the first of those samples, if one
    /// does, and how many samples were advanced.
    #[inline]
    pub fn advance(&mut self, max: usize, sample_rate: f32) -> (Option<f32>, usize) {
        let tick = if self.remaining == 0 {
            self.phase += sample_rate;
            let length = (self.phase / CONTROL_RATE).ceil().max(1.0);
            self.phase -= length * CONTROL_RATE;
            self.remaining = length as u32;
            Some(length)
        } else {
            None
        };
        let run = (self.remaining as usize).min(max);
        self.remaining -= run as u32;
        (tick, run)
    }
}

/// `stmlib::ParameterInterpolator` without the block: a linear ramp from the
/// value it has reached to the target set at the last tick.
#[derive(Clone, Copy, Default)]
pub struct Ramp {
    value: f32,
    increment: f32,
}

impl Ramp {
    /// Ramps to `target` over a tick of `length` samples, or jumps there when
    /// `length` is `None` (a voice's first tick). A NaN target holds the ramp
    /// where it is.
    #[inline]
    pub fn set(&mut self, target: f32, length: Option<f32>) {
        let target = if target.is_nan() { self.value } else { target };
        match length {
            Some(length) => self.increment = (target - self.value) / length,
            None => {
                self.value = target;
                self.increment = 0.0;
            }
        }
    }

    #[inline(always)]
    pub fn next_value(&mut self) -> f32 {
        self.value += self.increment;
        self.value
    }

    /// The value `t` samples past the last [`next_value`](Self::next_value).
    #[inline(always)]
    pub fn subsample(&self, t: f32) -> f32 {
        self.value + self.increment * t
    }
}

/// Splits `range` at control ticks, calling `f` with each stretch and the
/// length of the tick that starts it, if one does.
#[inline]
pub fn for_each_stretch(
    clock: &mut ControlClock,
    range: Range<usize>,
    sample_rate: f32,
    mut f: impl FnMut(Option<f32>, Range<usize>),
) {
    let mut start = range.start;
    while start < range.end {
        let (tick, run) = clock.advance(range.end - start, sample_rate);
        f(tick, start..start + run);
        start += run;
    }
}
