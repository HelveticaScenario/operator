use std::ops::Range;
use std::sync::atomic::{AtomicU32, Ordering, fence};

use deserr::Deserr;
use schemars::JsonSchema;

use crate::block_input::InputScratch;
use crate::{param_errors::ModuleParamErrors, poly::MonoSignal};

/// Number of tap slots the video renderer can read.
pub const MAX_VIDEO_TAPS: usize = 64;

/// Samples each tap remembers; a power of two so the ring index is a mask.
pub const VIDEO_HISTORY_LEN: usize = 4096;

/// The last [`VIDEO_HISTORY_LEN`] samples of each tap, as `f32` bits, written
/// as a ring.
static VIDEO_HISTORY: [[AtomicU32; VIDEO_HISTORY_LEN]; MAX_VIDEO_TAPS] =
    [const { [const { AtomicU32::new(0) }; VIDEO_HISTORY_LEN] }; MAX_VIDEO_TAPS];

/// How many samples each tap has published, wrapping; the next write goes to
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

/// Most samples a tap writes past its published head before publishing them,
/// so a reader treats that many of the oldest ring slots as possibly
/// overwritten.
const PUBLISH_CHUNK: usize = 64;

/// Samples returned the first time a tap is read.
const FIRST_READ_SAMPLES: usize = 1024;

/// Most samples one read returns. The rest of the ring is headroom for the
/// writer to advance into while a read is in progress.
pub const MAX_READ_SAMPLES: usize = VIDEO_HISTORY_LEN - 1024;

/// The samples tap `slot` has written since `since`, a `head` from an earlier
/// read, or its latest few when `since` is `None`. At most
/// [`MAX_READ_SAMPLES`] samples come back, so a reader that falls further
/// behind loses the oldest. `None` when `slot` is not a tap slot. Main thread.
pub fn read_video_since(slot: usize, since: Option<u32>) -> Option<VideoTapChunk> {
    let ring = VIDEO_HISTORY.get(slot)?;
    let head = VIDEO_HEADS[slot].load(Ordering::Acquire);
    let wanted = since.map_or(FIRST_READ_SAMPLES.min(head as usize), |since| {
        head.wrapping_sub(since) as usize
    });
    let count = wanted.min(MAX_READ_SAMPLES);
    let first = head.wrapping_sub(count as u32);
    let mut samples: Vec<f32> = (0..count as u32)
        .map(|i| {
            let index = first.wrapping_add(i) as usize & (VIDEO_HISTORY_LEN - 1);
            f32::from_bits(ring[index].load(Ordering::Relaxed))
        })
        .collect();
    // A slot the writer overwrote during the read shows a head past it here
    // (the writer's Release stores pair with this fence), so those samples
    // are dropped rather than returned out of order.
    fence(Ordering::Acquire);
    let after = VIDEO_HEADS[slot].load(Ordering::Relaxed);
    let oldest_intact = after.wrapping_sub((VIDEO_HISTORY_LEN - 1 - PUBLISH_CHUNK) as u32);
    let torn = oldest_intact.wrapping_sub(first) as i32;
    if torn > 0 {
        samples.drain(..(torn as usize).min(count));
    }
    Some(VideoTapChunk { head, samples })
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields, validate = video_tap_validate_params -> ModuleParamErrors)]
struct VideoTapParams {
    /// signal published to the video renderer
    input: MonoSignal,
    /// index of the tap slot the renderer reads (0–63)
    slot: usize,
}

fn video_tap_validate_params(
    params: VideoTapParams,
    _location: deserr::ValuePointerRef,
) -> Result<VideoTapParams, ModuleParamErrors> {
    if params.slot < MAX_VIDEO_TAPS {
        return Ok(params);
    }
    let mut errors = ModuleParamErrors::default();
    errors.add(
        "slot".to_string(),
        format!("slot must be below {MAX_VIDEO_TAPS}"),
    );
    Err(errors)
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
#[module(name = "_videoTap", args(input, slot), block)]
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
            .store(value.to_bits(), Ordering::Release);
        VIDEO_HEADS[slot].store(head.wrapping_add(1), Ordering::Release);
    }

    fn process_block(
        &mut self,
        outputs: &mut VideoTapBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let mut scratch = InputScratch::new();
        let poly = self.params.input.poly();
        let input = poly.block_input(range.clone(), poly.channels(), &mut scratch)?;
        let slot = self.params.slot.min(MAX_VIDEO_TAPS - 1);
        let ring = &VIDEO_HISTORY[slot];
        let mut head = VIDEO_HEADS[slot].load(Ordering::Relaxed);
        let end = range.end;
        for start in range.step_by(PUBLISH_CHUNK) {
            // A reader that sees any write below synchronizes with this fence,
            // so it sees every head published before the chunk.
            fence(Ordering::Release);
            for i in start..(start + PUBLISH_CHUNK).min(end) {
                // Summed as `MonoSignal::get_value` sums.
                let value = input.row(i).iter().sum();
                outputs.sample.slot_mut(i)[0] = value;
                ring[head as usize & (VIDEO_HISTORY_LEN - 1)]
                    .store(value.to_bits(), Ordering::Relaxed);
                head = head.wrapping_add(1);
            }
            VIDEO_HEADS[slot].store(head, Ordering::Release);
        }
        Some(())
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

    fn read(slot: usize, since: Option<u32>) -> VideoTapChunk {
        read_video_since(slot, since).unwrap()
    }

    #[test]
    fn passes_its_input_through() {
        let mut tap = make(60, 2.5);
        tap.update(SR);
        assert_eq!(tap.outputs.sample, 2.5);
    }

    #[test]
    fn a_slot_beyond_the_taps_is_a_param_error() {
        let deserializers = crate::dsp::get_params_deserializers();
        let try_slot = |slot: usize| {
            deserializers["_videoTap"](serde_json::json!({ "input": 0.0, "slot": slot }))
        };
        assert!(try_slot(MAX_VIDEO_TAPS - 1).is_ok());
        assert!(try_slot(MAX_VIDEO_TAPS).is_err());
    }

    #[test]
    fn reading_a_slot_beyond_the_taps_is_none() {
        assert!(read_video_since(MAX_VIDEO_TAPS, None).is_none());
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
        let chunk = read(50, None);
        assert_eq!(chunk.samples.len(), FIRST_READ_SAMPLES);
        assert_eq!(*chunk.samples.last().unwrap(), 1999.0);
        assert_eq!(chunk.samples[0], 2000.0 - FIRST_READ_SAMPLES as f32);
    }

    #[test]
    fn first_read_returns_only_samples_the_tap_has_written() {
        let mut tap = make(51, 0.0);
        drive(&mut tap, (0..10).map(|v| v as f32));
        let chunk = read(51, None);
        assert_eq!(chunk.samples, (0..10).map(|v| v as f32).collect::<Vec<_>>());
    }

    #[test]
    fn later_reads_continue_where_the_last_ended() {
        let mut tap = make(52, 0.0);
        drive(&mut tap, (0..10).map(|v| v as f32));
        let first = read(52, None);
        drive(&mut tap, (10..16).map(|v| v as f32));
        let second = read(52, Some(first.head));
        assert_eq!(second.samples, vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0]);
        assert!(read(52, Some(second.head)).samples.is_empty());
    }

    #[test]
    fn a_reader_that_falls_behind_gets_the_newest_samples() {
        let mut tap = make(53, 0.0);
        let total = VIDEO_HISTORY_LEN + 500;
        drive(&mut tap, (0..total).map(|v| v as f32));
        let chunk = read(53, Some(0));
        assert_eq!(chunk.samples.len(), MAX_READ_SAMPLES);
        assert_eq!(*chunk.samples.last().unwrap(), (total - 1) as f32);
        assert_eq!(chunk.samples[0], (total - MAX_READ_SAMPLES) as f32);
    }

    #[test]
    fn reads_across_the_ring_boundary_stay_in_order() {
        let mut tap = make(54, 0.0);
        drive(&mut tap, (0..(VIDEO_HISTORY_LEN - 2)).map(|v| v as f32));
        let before = read(54, None);
        drive(&mut tap, (0..6).map(|v| 100.0 + v as f32));
        let across = read(54, Some(before.head));
        assert_eq!(
            across.samples,
            vec![100.0, 101.0, 102.0, 103.0, 104.0, 105.0]
        );
    }

    #[test]
    fn samples_a_concurrent_writer_overwrites_are_never_returned_out_of_order() {
        let writer = std::thread::spawn(|| {
            let mut tap = make(55, 0.0);
            drive(&mut tap, (0..2_000_000).map(|v| v as f32));
        });
        while !writer.is_finished() {
            let chunk = read(55, Some(0));
            for pair in chunk.samples.windows(2) {
                assert_eq!(pair[1], pair[0] + 1.0);
            }
        }
        writer.join().unwrap();
    }

    fn render_blocks(tap: &mut VideoTap, blocks: impl Iterator<Item = f32>) {
        let mut outputs = VideoTapBlockOutputs::new(256, 1);
        for v in blocks {
            tap.params.input = MonoSignal::from_poly(PolySignal::mono(Signal::Volts(v)));
            tap.process_block(&mut outputs, 0..256, SR);
        }
    }

    #[test]
    fn block_writes_read_back_like_sample_writes() {
        let mut tap = make(56, 0.0);
        render_blocks(&mut tap, [1.0, 2.0].into_iter());
        drive(&mut tap, [3.0].into_iter());
        let chunk = read(56, Some(0));
        let mut want = vec![1.0; 256];
        want.extend([2.0; 256]);
        want.push(3.0);
        assert_eq!(chunk.samples, want);
    }

    #[test]
    fn block_writes_a_concurrent_reader_overtakes_are_never_returned() {
        let writer = std::thread::spawn(|| {
            let mut tap = make(57, 0.0);
            render_blocks(&mut tap, (0..100_000).map(|v| v as f32));
        });
        while !writer.is_finished() {
            let chunk = read(57, Some(0));
            for pair in chunk.samples.windows(2) {
                assert!(pair[1] == pair[0] || pair[1] == pair[0] + 1.0, "{pair:?}");
            }
        }
        writer.join().unwrap();
    }
}
