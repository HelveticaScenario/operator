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
    /// samples before engine sample `now` has reached, oldest first. A
    /// trigger `accept` declines stays queued in order for a later sample,
    /// without holding back the triggers behind it. `delay` is the
    /// resampler's input delay in engine samples and `ratio` converts engine
    /// samples to inner samples.
    pub fn take_due(
        &mut self,
        now: u64,
        ago: f32,
        delay: f32,
        ratio: f32,
        mut accept: impl FnMut(GrainTrigger) -> bool,
    ) {
        // Onsets only get later along the queue, so the due triggers lead it.
        let mut kept = [0u64; CAPACITY / 64];
        let mut due = 0;
        while due < self.len {
            let p = self.items[self.slot(due)];
            let late = (now - p.sample) as f32 + p.age - ago - delay;
            if late < 0.0 {
                break;
            }
            let grain = GrainTrigger {
                fractional_sample: (late * ratio).min(1.0),
                ..p.grain
            };
            if !accept(grain) {
                kept[due / 64] |= 1 << (due % 64);
            }
            due += 1;
        }
        // Close the gaps from the back, so the kept triggers end just before
        // the ones not yet due and keep their order.
        let mut write = due;
        for read in (0..due).rev() {
            if kept[read / 64] & (1 << (read % 64)) != 0 {
                write -= 1;
                self.items[self.slot(write)] = self.items[self.slot(read)];
            }
        }
        self.head = self.slot(write);
        self.len -= write;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trigger(voice: usize) -> GrainTrigger {
        GrainTrigger {
            voice,
            ..Default::default()
        }
    }

    #[test]
    fn a_declined_trigger_does_not_hold_back_the_rest() {
        let mut q = TriggerQueue::default();
        for (i, voice) in [0, 0, 1, 2].into_iter().enumerate() {
            q.push(i as u64, 1.0, trigger(voice));
        }
        // Voice 0 takes one grain start per sample.
        let mut taken = Vec::new();
        let mut busy = false;
        q.take_due(10, 0.0, 0.0, 1.0, |t| {
            if t.voice == 0 && busy {
                return false;
            }
            busy |= t.voice == 0;
            taken.push(t.voice);
            true
        });
        assert_eq!(taken, vec![0, 1, 2]);
        let mut rest = Vec::new();
        q.take_due(11, 0.0, 0.0, 1.0, |t| {
            rest.push(t.voice);
            true
        });
        assert_eq!(rest, vec![0]);
        assert!(q.is_empty());
    }

    #[test]
    fn triggers_not_yet_due_stay_queued_behind_declined_ones() {
        let mut q = TriggerQueue::default();
        q.push(0, 1.0, trigger(0));
        q.push(1, 1.0, trigger(1));
        q.push(5, 1.0, trigger(2));
        // With a 3-sample input delay, the trigger from sample 5 is not due
        // at sample 5.
        let mut first = Vec::new();
        q.take_due(5, 0.0, 3.0, 1.0, |t| {
            first.push(t.voice);
            t.voice != 0
        });
        assert_eq!(first, vec![0, 1]);
        let mut later = Vec::new();
        q.take_due(10, 0.0, 0.0, 1.0, |t| {
            later.push(t.voice);
            true
        });
        assert_eq!(later, vec![0, 2]);
        assert!(q.is_empty());
    }
}
