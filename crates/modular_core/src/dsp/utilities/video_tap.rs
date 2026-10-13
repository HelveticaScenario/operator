use std::sync::atomic::{AtomicU32, Ordering};

use deserr::Deserr;
use schemars::JsonSchema;

use crate::poly::MonoSignal;

/// Number of tap slots the video renderer can read.
pub const MAX_VIDEO_TAPS: usize = 64;

/// Samples each tap remembers; a power of two so the ring index is a mask.
pub const VIDEO_HISTORY_LEN: usize = 4096;

/// The last [`VIDEO_HISTORY_LEN`] samples of each tap, as `f32` bits, written
/// as a ring.
static VIDEO_HISTORY: [[AtomicU32; VIDEO_HISTORY_LEN]; MAX_VIDEO_TAPS] =
    [const { [const { AtomicU32::new(0) }; VIDEO_HISTORY_LEN] }; MAX_VIDEO_TAPS];

/// How many samples each tap has written, wrapping; the next write goes to
/// `head % VIDEO_HISTORY_LEN`.
static VIDEO_HEADS: [AtomicU32; MAX_VIDEO_TAPS] = [const { AtomicU32::new(0) }; MAX_VIDEO_TAPS];

/// Samples read from a tap, with the running count of samples it has written.
pub struct VideoTapChunk {
    /// Samples written so far, wrapping; pass it as `since` on the next read to
    /// continue exactly where this one ended.
    pub head: u32,
    /// The new samples, oldest first, in volts.
    pub samples: Vec<f32>,
}

/// Samples returned the first time a tap is read.
const FIRST_READ_SAMPLES: usize = 1024;

/// The samples tap `slot` has written since `since`, a `head` from an earlier
/// read, or its latest few when `since` is `None`. At most
/// [`VIDEO_HISTORY_LEN`] samples come back, so a reader that falls further
/// behind than the ring loses the oldest. Main thread.
pub fn read_video_since(slot: usize, since: Option<u32>) -> VideoTapChunk {
    let slot = slot.min(MAX_VIDEO_TAPS - 1);
    let head = VIDEO_HEADS[slot].load(Ordering::Acquire);
    let wanted = since.map_or(FIRST_READ_SAMPLES, |since| {
        head.wrapping_sub(since) as usize
    });
    let count = wanted.min(VIDEO_HISTORY_LEN);
    let samples = (0..count)
        .map(|i| {
            let index =
                (head as usize).wrapping_sub(count).wrapping_add(i) & (VIDEO_HISTORY_LEN - 1);
            f32::from_bits(VIDEO_HISTORY[slot][index].load(Ordering::Relaxed))
        })
        .collect();
    VideoTapChunk { head, samples }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct VideoTapParams {
    /// signal published to the video renderer
    input: MonoSignal,
    /// index of the tap slot the renderer reads (0–63)
    slot: usize,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct VideoTapOutputs {
    #[output("output", "input signal passthrough", default)]
    sample: f32,
}

/// Publishes its input to the video renderer, which polls the latest value
/// and the recent samples of each tap slot. The DSL inserts one per audio
/// signal a `$v` input reads.
#[module(name = "_videoTap", args(input, slot))]
pub struct VideoTap {
    outputs: VideoTapOutputs,
    params: VideoTapParams,
}

impl VideoTap {
    fn update(&mut self, _sample_rate: f32) {
        let value = self.params.input.get_value();
        self.outputs.sample = value;
        let slot = self.params.slot.min(MAX_VIDEO_TAPS - 1);
        let head = VIDEO_HEADS[slot].load(Ordering::Relaxed);
        VIDEO_HISTORY[slot][head as usize & (VIDEO_HISTORY_LEN - 1)]
            .store(value.to_bits(), Ordering::Relaxed);
        VIDEO_HEADS[slot].store(head.wrapping_add(1), Ordering::Release);
    }
}

message_handlers!(impl VideoTap {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::PolySignal;
    use crate::types::{OutputStruct, Signal};

    const SR: f32 = 48_000.0;

    fn make(slot: usize, volts: f32) -> VideoTap {
        let mut outputs = VideoTapOutputs::default();
        outputs.set_all_channels(1);
        VideoTap {
            params: VideoTapParams {
                input: MonoSignal::from_poly(PolySignal::mono(Signal::Volts(volts))),
                slot,
            },
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
        }
    }

    #[test]
    fn passes_its_input_through() {
        let mut tap = make(60, 2.5);
        tap.update(SR);
        assert_eq!(tap.outputs.sample, 2.5);
    }

    #[test]
    fn slots_beyond_the_array_share_the_last_slot() {
        let mut tap = make(MAX_VIDEO_TAPS + 5, -1.5);
        tap.update(SR);
        let chunk = read_video_since(MAX_VIDEO_TAPS - 1, Some(0));
        assert_eq!(*chunk.samples.last().unwrap(), -1.5);
    }

    fn drive(tap: &mut VideoTap, values: impl Iterator<Item = f32>) {
        for v in values {
            tap.params.input = MonoSignal::from_poly(PolySignal::mono(Signal::Volts(v)));
            tap.update(SR);
        }
    }

    #[test]
    fn first_read_returns_the_latest_samples_oldest_first() {
        let mut tap = make(50, 0.0);
        drive(&mut tap, (0..2000).map(|v| v as f32));
        let chunk = read_video_since(50, None);
        assert_eq!(chunk.samples.len(), FIRST_READ_SAMPLES);
        assert_eq!(*chunk.samples.last().unwrap(), 1999.0);
        assert_eq!(chunk.samples[0], 2000.0 - FIRST_READ_SAMPLES as f32);
    }

    #[test]
    fn later_reads_continue_where_the_last_ended() {
        let mut tap = make(52, 0.0);
        drive(&mut tap, (0..10).map(|v| v as f32));
        let first = read_video_since(52, None);
        drive(&mut tap, (10..16).map(|v| v as f32));
        let second = read_video_since(52, Some(first.head));
        assert_eq!(second.samples, vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0]);
        assert!(read_video_since(52, Some(second.head)).samples.is_empty());
    }

    #[test]
    fn a_reader_that_falls_behind_the_ring_loses_the_oldest() {
        let mut tap = make(53, 0.0);
        let total = VIDEO_HISTORY_LEN + 500;
        drive(&mut tap, (0..total).map(|v| v as f32));
        let chunk = read_video_since(53, Some(0));
        assert_eq!(chunk.samples.len(), VIDEO_HISTORY_LEN);
        assert_eq!(*chunk.samples.last().unwrap(), (total - 1) as f32);
        assert_eq!(chunk.samples[0], (total - VIDEO_HISTORY_LEN) as f32);
    }

    #[test]
    fn reads_across_the_ring_boundary_stay_in_order() {
        let mut tap = make(54, 0.0);
        drive(&mut tap, (0..(VIDEO_HISTORY_LEN - 2)).map(|v| v as f32));
        let before = read_video_since(54, None);
        drive(&mut tap, (0..6).map(|v| 100.0 + v as f32));
        let across = read_video_since(54, Some(before.head));
        assert_eq!(
            across.samples,
            vec![100.0, 101.0, 102.0, 103.0, 104.0, 105.0]
        );
    }
}
