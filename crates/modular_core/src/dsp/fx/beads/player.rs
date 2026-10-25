//! Grain scheduler for one voice: starts grains on triggers and steals the
//! least audible ones when all of its grains are busy.

use super::buffer::{Codec, GrainBuffer};
use super::grain::{Grain, GrainStart, HEAD_CROSSING_SAFETY_ZONE};
use crate::dsp::utils::rng::LcgRng;
use crate::dsp::utils::semitones_to_ratio;

/// Grains per voice, as on the hardware.
pub const NUM_GRAINS: usize = 32;

/// The scheduler relies on INSTANT < SIMPLE_SCHEDULER_THRESHOLD < PREEMPTIVE.
const INSTANT_CROSSFADE_TIME: f32 = 0.005;
const SIMPLE_SCHEDULER_THRESHOLD: f32 = 0.025;
const PREEMPTIVE_CROSSFADE_TIME: f32 = 0.1;
const MAX_GRAIN_DURATION: f32 = 4.0;
const NUM_SAFETY_GRAINS: usize = 4;

/// Grain controls sampled at the trigger, in hardware units.
#[derive(Clone, Copy, Debug, Default)]
pub struct GrainParams {
    /// Position in the recorded span, 0 (newest) to 1 (oldest).
    pub time: f32,
    /// Length and direction: −1 = long reversed, 0 = shortest, 1 = long
    /// forward.
    pub size: f32,
    pub shape: f32,
    /// Transposition in semitones.
    pub pitch: f32,
    /// Random stereo spread, 0 to 1.
    pub spread: f32,
}

/// A grain start for one voice.
#[derive(Clone, Copy, Debug, Default)]
pub struct GrainTrigger {
    pub voice: usize,
    /// Grain-engine samples elapsed since the onset (0..1).
    pub fractional_sample: f32,
    pub params: GrainParams,
}

pub struct GranularPlayer {
    grains: [Grain; NUM_GRAINS],
    /// Grain indices ordered from best to worst candidate for stealing.
    priority: [usize; NUM_GRAINS],
    instant_crossfade_time: i32,
    preemptive_crossfade_time: i32,
    simple_scheduler_threshold: i32,
    max_grain_duration: f32,
    sample_rate: f32,
    grain_interval: i32,
    /// Bit `i` is set while grain `i` may still be sounding.
    active: u32,
}

impl Default for GranularPlayer {
    fn default() -> Self {
        Self {
            grains: [Grain::default(); NUM_GRAINS],
            priority: std::array::from_fn(|i| i),
            instant_crossfade_time: 1,
            preemptive_crossfade_time: 1,
            simple_scheduler_threshold: 1,
            max_grain_duration: 1.0,
            sample_rate: 1.0,
            grain_interval: 1,
            active: 0,
        }
    }
}

/// Uniform value in `[0.5 - s/2, 0.5 + s/2]` whose width is itself random.
pub fn bounded_random(rng: &mut LcgRng, spread: f32) -> f32 {
    let u = rng.next_unit();
    let v = rng.next_unit();
    0.5 + (u - 0.5) * (spread * v).min(1.0)
}

impl GranularPlayer {
    pub fn init(&mut self, sample_rate: f32) {
        self.instant_crossfade_time = (sample_rate * INSTANT_CROSSFADE_TIME) as i32;
        self.preemptive_crossfade_time = (sample_rate * PREEMPTIVE_CROSSFADE_TIME) as i32;
        self.simple_scheduler_threshold = (sample_rate * SIMPLE_SCHEDULER_THRESHOLD) as i32;
        self.grain_interval = 1;
        self.max_grain_duration = sample_rate * MAX_GRAIN_DURATION;
        self.sample_rate = sample_rate;
        self.reset();
    }

    pub fn reset(&mut self) {
        for (i, grain) in self.grains.iter_mut().enumerate() {
            grain.init(self.sample_rate);
            self.priority[i] = i;
        }
        self.active = 0;
    }

    fn grain_duration(&self, size: f32) -> f32 {
        // Lengths near 0 all give the shortest grain.
        let shortness = (1.025 * (1.0 - size.abs())).min(1.0);
        semitones_to_ratio(shortness * -108.0) * self.max_grain_duration
    }

    /// Grains that may still be sounding; equal to the number not done, as a
    /// grain only finishes while rendering, which clears its bit.
    #[inline]
    pub fn active_count(&self) -> u32 {
        self.active.count_ones()
    }

    fn sort_grain_list(&mut self) {
        let mut fade_time = [0i32; NUM_SAFETY_GRAINS];
        if self.grain_interval < self.simple_scheduler_threshold {
            // Grains arrive fast: assume the oldest is nearly done, steal it,
            // and move the new grain to the back.
            self.priority.rotate_left(1);
            fade_time[0] = self.grain_interval + 1;
            fade_time[1] = self.instant_crossfade_time;
            fade_time[2] = self.instant_crossfade_time;
            fade_time[3] = self.preemptive_crossfade_time;
            if self.grain_interval > self.instant_crossfade_time {
                fade_time.swap(0, 2);
            }
        } else {
            // Order the front of the list by remaining play time, so the
            // grains stolen next are the ones closest to finishing.
            let grains = &self.grains;
            for i in 0..NUM_SAFETY_GRAINS {
                let mut best = i;
                for j in (i + 1)..NUM_GRAINS {
                    if grains[self.priority[j]].remaining_samples()
                        < grains[self.priority[best]].remaining_samples()
                    {
                        best = j;
                    }
                }
                self.priority.swap(i, best);
            }
            fade_time[0] = self.instant_crossfade_time;
            fade_time[1] = self.instant_crossfade_time;
            fade_time[2] = self.grain_interval;
            fade_time[3] = self.preemptive_crossfade_time;
            if self.grain_interval > self.preemptive_crossfade_time {
                fade_time.swap(2, 3);
            }
        }
        for (i, &time) in fade_time.iter().enumerate() {
            self.grains[self.priority[i]].fade_out(time);
        }
        self.grain_interval = 0;
    }

    /// Start a grain at the slot that is the best candidate for stealing.
    pub fn start(
        &mut self,
        buffer: &GrainBuffer,
        p: &GrainParams,
        fractional_sample: f32,
        gain: f32,
        pan: f32,
    ) {
        let reverse = p.size < 0.0;
        let safety = HEAD_CROSSING_SAFETY_ZONE + 1;
        let max_delay = (buffer.duration() - 2 * safety) as f32;
        let speed = semitones_to_ratio(p.pitch);
        let start = GrainStart {
            delay: p.time * max_delay + safety as f32,
            speed: if reverse { -speed } else { speed },
            duration: self.grain_duration(p.size),
            shape: p.shape,
            gain,
            pan,
            fractional_sample,
        };
        let index = self.priority[0];
        self.grains[index].start(buffer, start);
        self.active |= 1 << index;
        self.sort_grain_list();
    }

    /// Advance every sounding grain by one sample, handing each grain and its
    /// sample to `mix`.
    #[inline]
    pub fn render<C: Codec>(&mut self, buffer: &GrainBuffer, mut mix: impl FnMut(&Grain, f32)) {
        // Ascending index order keeps the mix's rounding identical to
        // rendering every grain.
        let mut bits = self.active;
        while bits != 0 {
            let i = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let grain = &mut self.grains[i];
            if let Some(sample) = grain.next_sample::<C>(buffer) {
                mix(grain, sample);
            }
            if grain.done() {
                self.active &= !(1 << i);
            }
        }
        self.grain_interval = self.grain_interval.saturating_add(1);
    }
}
