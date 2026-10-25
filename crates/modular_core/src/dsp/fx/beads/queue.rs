//! Grain starts waiting for the resampled input to reach their onset.

use super::player::GrainTrigger;

const CAPACITY: usize = 256;

#[derive(Clone, Copy, Default)]
struct Pending {
    /// Engine sample the trigger was detected in.
    sample: u64,
    /// Engine samples from the onset to the end of that sample (0..1].
    age: f32,
    grain: GrainTrigger,
}

/// Fixed-capacity FIFO kept in onset order; triggers past capacity are
/// dropped.
pub struct TriggerQueue {
    items: [Pending; CAPACITY],
    head: usize,
    len: usize,
}

impl Default for TriggerQueue {
    fn default() -> Self {
        Self {
            items: [Pending::default(); CAPACITY],
            head: 0,
            len: 0,
        }
    }
}

impl TriggerQueue {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    #[inline]
    fn slot(&self, i: usize) -> usize {
        (self.head + i) % CAPACITY
    }

    /// Queue `grain`, detected in engine sample `sample` with its onset `age`
    /// samples before that sample's end.
    pub fn push(&mut self, sample: u64, age: f32, grain: GrainTrigger) {
        if self.len == CAPACITY {
            return;
        }
        let mut i = self.len;
        self.items[self.slot(i)] = Pending { sample, age, grain };
        self.len += 1;
        // Triggers detected in the same sample are ordered by onset.
        while i > 0 {
            let (prev, cur) = (self.slot(i - 1), self.slot(i));
            let (a, b) = (self.items[prev], self.items[cur]);
            if a.sample != b.sample || a.age >= b.age {
                break;
            }
            self.items.swap(prev, cur);
            i -= 1;
        }
    }

    /// Hand `accept` the triggers whose onset the inner sample `ago` engine
    /// samples before engine sample `now` has reached, oldest first, until
    /// it declines one. `delay` is the resampler's input delay in engine
    /// samples and `ratio` converts engine samples to inner samples.
    pub fn take_due(
        &mut self,
        now: u64,
        ago: f32,
        delay: f32,
        ratio: f32,
        mut accept: impl FnMut(GrainTrigger) -> bool,
    ) {
        while self.len > 0 {
            let p = self.items[self.head];
            let late = (now - p.sample) as f32 + p.age - ago - delay;
            if late < 0.0 {
                break;
            }
            let grain = GrainTrigger {
                fractional_sample: (late * ratio).min(1.0),
                ..p.grain
            };
            if !accept(grain) {
                break;
            }
            self.head = self.slot(1);
            self.len -= 1;
        }
    }
}
