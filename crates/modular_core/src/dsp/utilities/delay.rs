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
        // Ensure the $buffer module has processed this frame first.
        // This triggers $buffer.update() which writes the sample and increments write_index.
        self.params.buffer.ensure_source_updated();

        let write_index = self.params.buffer.read_write_index() as f64;
        let frame_count = self.params.buffer.frame_count();
        let channels = self.channel_count();

        for channel in 0..channels {
            let delay_time_secs = (self.params.time.get_value(channel) as f64).max(0.0);
            let delay_frames = delay_time_secs * (sample_rate as f64);
            let read_frame = write_index - delay_frames;

            let wrapped_frame = if frame_count > 0 {
                read_frame.rem_euclid(frame_count as f64) as f32
            } else {
                0.0
            };

            let buf_channels = self.params.buffer.channel_count().max(1);
            let buf_channel = channel % buf_channels;
            let value = self
                .params
                .buffer
                .read_hermite_wrapped(buf_channel, wrapped_frame);
            self.outputs.sample.set(channel, value);
        }
    }
}

message_handlers!(impl DelayRead {});
