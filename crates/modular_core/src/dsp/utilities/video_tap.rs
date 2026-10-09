use std::sync::atomic::{AtomicU32, Ordering};

use deserr::Deserr;
use schemars::JsonSchema;

use crate::poly::MonoSignal;

/// Number of tap slots the video renderer can read.
pub const MAX_VIDEO_TAPS: usize = 64;

/// Latest value of each tap, as `f32` bits. The audio thread stores and the
/// main thread loads, so a read never blocks or allocates on the audio side.
static VIDEO_TAPS: [AtomicU32; MAX_VIDEO_TAPS] = [const { AtomicU32::new(0) }; MAX_VIDEO_TAPS];

/// Latest values of taps `0..count`, in volts. Main thread.
pub fn read_video_taps(count: usize) -> Vec<f64> {
    VIDEO_TAPS
        .iter()
        .take(count)
        .map(|tap| f32::from_bits(tap.load(Ordering::Relaxed)) as f64)
        .collect()
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
/// of each tap slot. The DSL inserts one per audio signal a `$v` input reads.
#[module(name = "_videoTap", args(input, slot))]
pub struct VideoTap {
    outputs: VideoTapOutputs,
    params: VideoTapParams,
}

impl VideoTap {
    fn update(&mut self, _sample_rate: f32) {
        let value = self.params.input.get_value();
        self.outputs.sample = value;
        VIDEO_TAPS[self.params.slot.min(MAX_VIDEO_TAPS - 1)]
            .store(value.to_bits(), Ordering::Relaxed);
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
    fn publishes_the_input_to_its_slot() {
        let mut tap = make(60, 2.5);
        tap.update(SR);
        assert_eq!(read_video_taps(61)[60], 2.5);
        assert_eq!(tap.outputs.sample, 2.5);
    }

    #[test]
    fn slots_beyond_the_array_share_the_last_slot() {
        let mut tap = make(MAX_VIDEO_TAPS + 5, -1.5);
        tap.update(SR);
        assert_eq!(read_video_taps(MAX_VIDEO_TAPS)[MAX_VIDEO_TAPS - 1], -1.5);
    }

    #[test]
    fn reads_only_the_requested_count() {
        assert_eq!(read_video_taps(3).len(), 3);
        assert_eq!(read_video_taps(MAX_VIDEO_TAPS + 10).len(), MAX_VIDEO_TAPS);
    }
}
