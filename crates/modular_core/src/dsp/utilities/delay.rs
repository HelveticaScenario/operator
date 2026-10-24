use deserr::Deserr;
use schemars::JsonSchema;

use crate::{Buffer, PolySignal, poly::PolyOutput};

fn delay_read_derive_channel_count(params: &DelayReadParams) -> usize {
    params.buffer.channel_count().max(params.time.channels())
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct DelayReadParams {
    buffer: Buffer,
    /// Delay time in seconds (e.g. 0.5 for 500ms)
    #[signal(default = 0.1, range = (0.0, 5.0))]
    time: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DelayReadOutputs {
    #[output("output", "delayed signal", default)]
    sample: PolyOutput,
}

/// Reads a signal from a buffer at a specified delay time relative to the write position.
#[module(name = "$delayRead", channels_derive = delay_read_derive_channel_count, args(buffer, time))]
pub struct DelayRead {
    outputs: DelayReadOutputs,
    params: DelayReadParams,
}

impl DelayRead {
    fn update(&mut self, sample_rate: f32) {
        // Drive the source up through this reader's current slot so the
        // writer's per-slot cursor lands at the same position the reader
        // is about to read. Calling `ensure_processed()` (full block)
        // breaks feedback cycles: the writer races ahead of this reader's
        // interleave and pulls stale `block_outputs` slots back via the
        // 1-sample-delay reentrancy path.
        let slot = self.current_block_index();
        self.params.buffer.ensure_source_updated_to(slot + 1);

        let frame_count = self.params.buffer.frame_count();
        let channels = self.channel_count();
        // This slot's write position is the base the source's `tick_buffers`
        // set at block start plus `slot`. Inside a feedback cycle the source
        // may still be computing this slot, so that frame holds data from a
        // full buffer ago and the newest written frame is the one before it.
        let unwritten = usize::from(!self.params.buffer.source_wrote_slot(slot));
        let newest = (self.params.buffer.read_write_index() + slot + frame_count)
            .wrapping_sub(unwritten)
            % frame_count.max(1);

        for channel in 0..channels {
            let delay_time_secs = (self.params.time.get_value(channel) as f64).max(0.0);
            let back = (delay_time_secs * (sample_rate as f64) - unwritten as f64).max(0.0);

            let buf_channels = self.params.buffer.channel_count().max(1);
            let buf_channel = channel % buf_channels;
            let value = self
                .params
                .buffer
                .read_hermite_behind(buf_channel, newest, back as f32);
            self.outputs.sample.set(channel, value);
        }
    }
}

message_handlers!(impl DelayRead {});
