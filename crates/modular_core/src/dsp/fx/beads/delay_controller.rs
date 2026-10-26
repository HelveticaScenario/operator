//! Beads' delay controller for one voice: the taps the delay reads each
//! sample, with their delay times and windowed gains.
//!
//! Two intervals, the current delay time and the one before it, crossfade
//! when the time changes, so a new tempo or `bars` swaps the repeats over at
//! an unchanged pitch. Each interval has two taps, at neighbouring multiples
//! of the interval, crossfaded by the multiplier. While frozen, play heads
//! loop slices of the held buffer.

use super::delay_windows::{HysteresisFilter, HysteresisQuantizer, PlayHead, TapWindow};
use super::primitives::Ramp;

pub const NUM_INTERVALS: usize = 2;
pub const NUM_TAPS: usize = NUM_INTERVALS * 2;
/// The running taps, then the frozen play heads, which crossfade as
/// `freeze` changes.
pub const RENDERED_TAPS: usize = NUM_TAPS * 2;

/// Seconds for a tap window's phase to settle onto the playhead.
const LOCK_TIME: f32 = 0.05;
/// Seconds to crossfade between the running taps and the frozen play heads.
const FREEZE_FADE_TIME: f32 = 0.01;

/// One tap: its delay in samples in 10-bit fixed point, 0 when silent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DelayTap {
    pub delay: i32,
    pub gain: f32,
}

/// Where the playhead is, in bars, and whether it is moving.
#[derive(Clone, Copy, Debug, Default)]
pub struct Grid {
    pub position: f64,
    /// Groove position minus `position`, in bars.
    pub groove: f64,
    pub moving: bool,
}

impl Grid {
    /// This grid with `amount` (0–1) of the groove added to the position.
    #[inline]
    pub fn grooved(self, amount: f32) -> Self {
        Self {
            position: self.position + amount as f64 * self.groove,
            ..self
        }
    }

    /// Phase within the current window of `period` bars.
    #[inline]
    pub fn phase(&self, period: f32) -> f32 {
        let p = self.position / period as f64;
        (p - p.floor()) as f32
    }
}

/// Crossfades between the old and new interval when the delay time changes.
#[derive(Default)]
struct IntervalClock {
    /// Old and new interval in samples.
    interval: [f32; 2],
    /// Bars in the old and new interval.
    bars: [f32; 2],
    fade: f32,
    fade_increment: f32,
    min_fade_increment: f32,
    max_fade_increment: f32,
    /// Offset from logical to stored interval, so a tap keeps its interval
    /// across a swap.
    rotate: usize,
    /// Interval and bars to swap to once the running fade ends.
    pending: Option<(f32, f32)>,
    /// Whether the swap happened on this sample.
    retrig: bool,
    started: bool,
}

impl IntervalClock {
    fn init(&mut self, sample_rate: f32) {
        *self = Self {
            fade: 1.0,
            min_fade_increment: 1.0 / (sample_rate * 0.25),
            max_fade_increment: 1.0 / (sample_rate * 0.02),
            ..Self::default()
        };
    }

    /// Head for `interval` samples spanning `bars`.
    fn set_target(&mut self, interval: f32, bars: f32) {
        if !self.started {
            self.started = true;
            self.interval = [interval; 2];
            self.bars = [bars; 2];
            return;
        }
        // Tempo estimates wander by far less than this, so only a real
        // change swaps the interval.
        let current = self.interval[1];
        if (interval - current).abs() > 1e-3 * current {
            self.pending = Some((interval, bars));
        } else {
            // A clamped interval holds while the bars it spans change.
            self.pending = None;
            self.bars[1] = bars;
        }
    }

    fn process(&mut self) {
        self.retrig = false;
        self.fade = (self.fade + self.fade_increment).min(1.0);
        if self.fade < 1.0 {
            return;
        }
        if let Some((interval, bars)) = self.pending.take() {
            self.rotate += 1;
            self.interval[0] = self.interval[1];
            self.bars[0] = self.bars[1];
            self.interval[1] = interval;
            self.bars[1] = bars;
            self.fade = 0.0;
            self.fade_increment =
                (2.0 / interval).clamp(self.min_fade_increment, self.max_fade_increment);
            self.retrig = true;
        }
    }

    #[inline]
    fn slot(&self, i: usize) -> usize {
        (i + self.rotate) % 2
    }

    #[inline]
    fn interval(&self, i: usize) -> f32 {
        self.interval[self.slot(i)]
    }

    #[inline]
    fn bars(&self, i: usize) -> f32 {
        self.bars[self.slot(i)]
    }

    #[inline]
    fn gain(&self, i: usize) -> f32 {
        if self.slot(i) == 1 {
            self.fade
        } else {
            1.0 - self.fade
        }
    }

    /// Logical index of the interval fading in.
    #[inline]
    fn new_interval_index(&self) -> usize {
        (self.rotate + 1) % 2
    }
}

/// Controls read once per control block, in hardware units.
#[derive(Clone, Copy, Debug, Default)]
pub struct DelayControls {
    /// Base delay time in samples.
    pub interval: f32,
    /// Bars the base delay time spans.
    pub bars: f32,
    /// Multiplier, or slice while frozen (0–1).
    pub time: f32,
    pub shape: f32,
}

#[derive(Default)]
pub struct DelayController {
    min_delay: f32,
    max_delay: f32,
    lock: f32,
    clock: IntervalClock,
    windows: [TapWindow; NUM_TAPS],
    /// Windows to start at the playhead's phase on their next sample.
    snap: [bool; NUM_TAPS],
    heads: [PlayHead; NUM_INTERVALS],
    slave_heads: [PlayHead; NUM_INTERVALS],
    quantizers: [HysteresisQuantizer; NUM_INTERVALS],
    slice_filter: HysteresisFilter,
    multiplier_lp: f32,
    shape: Ramp,
    time: f32,
    /// 0 with only the running taps heard, 1 with only the play heads.
    freeze_mix: f32,
    freeze_fade: f32,
    /// Whether each set rendered on the last sample; a set restarts when it
    /// is heard again.
    taps_running: bool,
    heads_running: bool,
}

impl DelayController {
    /// Configure for `sample_rate`, with delays from `min_delay` to
    /// `max_delay` samples.
    pub fn init(&mut self, sample_rate: f32, min_delay: f32, max_delay: f32) {
        *self = Self {
            min_delay,
            max_delay,
            lock: 1.0 - (-1.0 / (LOCK_TIME * sample_rate)).exp(),
            snap: [true; NUM_TAPS],
            freeze_fade: 1.0 / (FREEZE_FADE_TIME * sample_rate),
            ..Self::default()
        };
        self.clock.init(sample_rate);
        for q in self.quantizers.iter_mut() {
            q.init(2, 0.05);
        }
    }

    pub fn start_block(&mut self, controls: DelayControls, block_size: usize) {
        let interval = controls.interval.clamp(self.min_delay, self.max_delay);
        // A clamped interval spans proportionally fewer or more bars.
        let bars = controls.bars * interval / controls.interval;
        self.clock.set_target(interval, bars);
        self.shape.start(controls.shape * 0.995, block_size);
        self.time = 1.005 * controls.time;
    }

    /// The taps for one sample.
    pub fn render(&mut self, freeze: bool, grid: Grid) -> [DelayTap; RENDERED_TAPS] {
        self.clock.process();
        let shape = self.shape.next();
        self.freeze_mix = if freeze {
            (self.freeze_mix + self.freeze_fade).min(1.0)
        } else {
            (self.freeze_mix - self.freeze_fade).max(0.0)
        };
        let m = self.freeze_mix;
        let mut taps = [DelayTap::default(); RENDERED_TAPS];
        if m < 1.0 {
            let gain = (1.0 - m) * (1.0 + m);
            let running = self.render_taps(shape, grid);
            for (out, tap) in taps[..NUM_TAPS].iter_mut().zip(running) {
                *out = DelayTap {
                    gain: tap.gain * gain,
                    ..tap
                };
            }
        }
        self.taps_running = m < 1.0;
        if m > 0.0 {
            let gain = m * (2.0 - m);
            let heads = self.render_frozen(shape, !freeze);
            for (out, tap) in taps[NUM_TAPS..].iter_mut().zip(heads) {
                *out = DelayTap {
                    gain: tap.gain * gain,
                    ..tap
                };
            }
        }
        self.heads_running = m > 0.0;
        taps
    }

    fn render_taps(&mut self, shape: f32, grid: Grid) -> [DelayTap; NUM_TAPS] {
        if !self.taps_running {
            self.snap = [true; NUM_TAPS];
        }
        if self.clock.retrig {
            let start = self.clock.new_interval_index() * 2;
            self.snap[start..start + 2].fill(true);
        }

        self.multiplier_lp += 0.01 * (self.time - self.multiplier_lp);
        let multiplier = (self.multiplier_lp * 15.0 + 1.0).clamp(1.0, 16.0);
        let integral = multiplier as usize;
        let fractional = (0.5 + 8.0 * (multiplier - integral as f32 - 0.5)).clamp(0.0, 1.0);

        let mut taps = [DelayTap::default(); NUM_TAPS];
        for t in 0..NUM_TAPS {
            let interval = t / 2;
            // A silent interval is the next to fade in, and its windows snap
            // to the playhead when it does.
            if self.clock.gain(interval) == 0.0 {
                continue;
            }
            let tap = t & 1;
            // Each multiple keeps its window as the multiplier passes it.
            let i = if integral & 1 == 1 { tap } else { 1 - tap } + interval * 2;
            let multiple = integral + tap;

            let delay_time = (self.clock.interval(interval) * multiple as f32)
                .clamp(self.min_delay, self.max_delay);
            // The window repeats with the tap, whose delay may be clamped.
            let period = delay_time * self.clock.bars(interval) / self.clock.interval(interval);
            let window = &mut self.windows[i];
            if self.snap[i] || window.multiple != multiple {
                window.snap(grid, period);
                window.multiple = multiple;
                self.snap[i] = false;
            }

            let ratio_fade = if tap == 1 {
                fractional
            } else {
                1.0 - fractional
            };
            let gain = ratio_fade * self.clock.gain(interval);
            taps[i] = DelayTap {
                delay: (delay_time * 1024.0) as i32,
                gain: gain * window.render(shape, 1.0 / delay_time, grid, period, self.lock),
            };
        }
        taps
    }

    /// The play heads' taps. While `recording`, the record head moves away
    /// from the heads, so they keep their place in the held audio as they
    /// fade out.
    fn render_frozen(&mut self, shape: f32, recording: bool) -> [DelayTap; NUM_TAPS] {
        if !self.heads_running {
            self.heads = Default::default();
            self.slave_heads = Default::default();
        }
        if self.clock.retrig {
            let i = self.clock.new_interval_index();
            self.heads[i] = PlayHead::default();
            self.slave_heads[i] = PlayHead::default();
        }

        if recording {
            for head in self.heads.iter_mut().chain(self.slave_heads.iter_mut()) {
                head.hold();
            }
        }
        let slice = self.slice_filter.process(self.time, 0.0025);
        let mut taps = [DelayTap::default(); NUM_TAPS];
        for i in 0..NUM_INTERVALS {
            let head = if self.heads[i].done() {
                Some(&mut self.heads[i])
            } else if self.heads[i].half_way() {
                Some(&mut self.slave_heads[i])
            } else {
                None
            };
            if let Some(head) = head {
                let duration = (self.clock.interval(i) as i32).clamp(
                    self.min_delay as i32,
                    (self.max_delay - self.min_delay) as i32,
                );
                let slices = self.max_delay as i32 / duration;
                let quantizer = &mut self.quantizers[i];
                if quantizer.steps != slices {
                    quantizer.init(slices, 0.01 * slices as f32);
                }
                let index = quantizer.process(slice);
                head.start(
                    duration * index + self.min_delay as i32,
                    duration,
                    self.clock.gain(i),
                );
            }
            taps[2 * i] = self.heads[i].render(shape, false);
            taps[2 * i + 1] = self.slave_heads[i].render(shape, true);
        }
        // Held audio this far back is being recorded over.
        let longest = (self.max_delay as i32 - 1) << 10;
        for tap in taps.iter_mut() {
            tap.delay = tap.delay.min(longest);
        }
        taps
    }
}
