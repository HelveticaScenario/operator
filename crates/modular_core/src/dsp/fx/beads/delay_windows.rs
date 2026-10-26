//! Shapes over the delay's taps: the windows of the running taps, locked to
//! the playhead, and the play heads that loop slices while frozen.

use super::delay_controller::{DelayTap, Grid};

/// Linear interpolation into `table` at `x` (0–1) scaled by `scale`.
#[inline]
fn interpolate(table: &[f32], x: f32, scale: f32) -> f32 {
    let x = x * scale;
    let i = x as usize;
    let a = table[i];
    let b = table[i + 1];
    a + (b - a) * (x - i as f32)
}

/// Triangle rising over `[0, pw)` and falling over `[pw, 1)`.
#[inline]
fn triangle(phase: f32, pw: f32) -> f32 {
    if phase < pw {
        phase / pw
    } else {
        1.0 - (phase - pw) / (1.0 - pw)
    }
}

/// Window peak positions for `shape` from 0.2 to 1.
const PEAKS: [f32; 6] = [0.0, 0.001, 0.01, 0.5, 0.75, 0.999];

/// A window of `shape` (0–1) at `phase`, morphing from `flat` through a
/// sharp attack to a triangle and a ramp.
#[inline]
fn window(phase: f32, shape: f32, flat: f32) -> f32 {
    if shape < 0.2 {
        flat + (triangle(phase, PEAKS[1]) - flat) * shape * 5.0
    } else {
        triangle(phase, interpolate(&PEAKS, shape, 5.0))
    }
}

/// A tap's window, repeating once per delay time and locked to the playhead
/// while it moves.
#[derive(Clone, Copy, Default)]
pub struct TapWindow {
    phase: f32,
    /// Multiple of the interval the tap plays; 0 before its first sample.
    pub multiple: usize,
}

impl TapWindow {
    /// The window at `shape` for this sample, advanced by `increment` and
    /// pulled toward the playhead's phase in windows of `period` bars.
    #[inline]
    pub fn render(
        &mut self,
        shape: f32,
        increment: f32,
        grid: Grid,
        period: f32,
        lock: f32,
    ) -> f32 {
        let envelope = window(self.phase, shape, 1.0);
        self.phase += increment;
        if grid.moving {
            let error = grid.phase(period) - self.phase;
            self.phase += (error - error.round()) * lock;
        }
        self.phase -= self.phase.floor();
        envelope
    }

    /// Start at the playhead's phase, or from 0 when it is stopped.
    pub fn snap(&mut self, grid: Grid, period: f32) {
        self.phase = if grid.moving { grid.phase(period) } else { 0.0 };
    }
}

/// One slice of the frozen buffer being played.
#[derive(Clone, Copy, Default)]
pub struct PlayHead {
    start: i32,
    remaining: i32,
    mid_point: i32,
    phase: f32,
    phase_increment: f32,
    gain: f32,
}

impl PlayHead {
    #[inline]
    pub fn done(&self) -> bool {
        self.remaining == 0
    }

    #[inline]
    pub fn half_way(&self) -> bool {
        self.remaining != 0 && self.remaining == self.mid_point
    }

    /// Keep the head's place in the held audio as the record head moves on
    /// by a sample.
    pub fn hold(&mut self) {
        self.start += 1;
    }

    pub fn start(&mut self, start: i32, duration: i32, gain: f32) {
        self.start = start;
        self.remaining = duration;
        self.mid_point = duration >> 1;
        self.phase = 0.0;
        self.phase_increment = 1.0 / duration as f32;
        self.gain = gain;
    }

    /// The tap for this sample. A `slave` head fills the gap halfway
    /// through its master's slice, sounding only around the triangle window.
    #[inline]
    pub fn render(&mut self, shape: f32, slave: bool) -> DelayTap {
        if self.done() {
            return DelayTap::default();
        }
        let envelope = if slave {
            let t = triangle(self.phase, 0.5);
            interpolate(&[0.0, 0.0, 0.0, t, 0.0, 0.0], shape, 5.0)
        } else {
            window(
                self.phase,
                shape,
                (100.0 * triangle(self.phase, 0.5)).min(1.0),
            )
        };
        let tap = DelayTap {
            delay: (self.start + self.remaining) << 10,
            gain: self.gain * envelope,
        };
        self.remaining -= 1;
        self.phase += self.phase_increment;
        tap
    }
}

/// Follows its input only once it moves further than `threshold` away.
#[derive(Clone, Copy, Default)]
pub struct HysteresisFilter {
    value: f32,
}

impl HysteresisFilter {
    #[inline]
    pub fn process(&mut self, x: f32, threshold: f32) -> f32 {
        let error = x - self.value;
        if error > threshold {
            self.value = x - threshold;
        } else if error < -threshold {
            self.value = x + threshold;
        }
        self.value
    }
}

/// Picks one of `steps` steps for a 0–1 value, holding the step until the
/// value moves `hysteresis` steps past its edge.
#[derive(Clone, Copy, Default)]
pub struct HysteresisQuantizer {
    pub steps: i32,
    hysteresis: f32,
    value: i32,
}

impl HysteresisQuantizer {
    pub fn init(&mut self, steps: i32, hysteresis: f32) {
        *self = Self {
            steps,
            hysteresis,
            value: 0,
        };
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> i32 {
        let x = x * self.steps as f32 - 0.5;
        let sign = if x > self.value as f32 { -1.0 } else { 1.0 };
        self.value = ((x + sign * self.hysteresis + 0.5) as i32).clamp(0, self.steps - 1);
        self.value
    }
}
