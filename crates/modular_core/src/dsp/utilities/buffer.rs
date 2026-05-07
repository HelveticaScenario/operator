use deserr::Deserr;
use schemars::JsonSchema;
use std::sync::Arc;

use crate::{
    Buffer, BufferData, MonoSignal,
    poly::{PolyOutput, PolySignal},
};

// ─── $buffer (BufferWrite) ────────────────────────────────────────────────────

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct BufferWriteParams {
    input: PolySignal,
    #[serde(default = "default_buffer_length")]
    #[deserr(default = default_buffer_length())]
    #[schemars(skip)]
    #[signal(skip)]
    length: f64,
}

fn default_buffer_length() -> f64 {
    1.0
}

/// Outputs for the $buffer module.
/// Manually implements OutputStruct because the buffer field is `Arc<BufferData>`,
/// not the usual `PolyOutput` / `f32` that `#[derive(Outputs)]` handles.
#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BufferWriteOutputs {
    /// Passthrough of the input signal
    sample: PolyOutput,
    /// The circular audio buffer — not a sample output, accessed via get_buffer_output
    #[serde(skip)]
    #[schemars(skip)]
    buffer: Arc<BufferData>,
}

impl Default for BufferWriteOutputs {
    fn default() -> Self {
        Self {
            sample: PolyOutput::default(),
            // Minimal placeholder — will be replaced by init()
            buffer: Arc::new(BufferData::new_zeroed(1, 1)),
        }
    }
}

impl crate::types::OutputStruct for BufferWriteOutputs {
    fn copy_from(&mut self, other: &Self) {
        self.sample = other.sample;
        // buffer is not copied via copy_from — it's transferred via transfer_buffers_from
    }

    fn get_poly_sample(&self, port: &str) -> Option<PolyOutput> {
        match port {
            "output" => Some(self.sample),
            _ => None,
        }
    }

    fn set_all_channels(&mut self, channels: usize) {
        self.sample.set_channels(channels);
    }

    fn schemas() -> Vec<crate::types::OutputSchema> {
        vec![crate::types::OutputSchema {
            name: "output".to_string(),
            description: "input signal passthrough".to_string(),
            polyphonic: true,
            default: true,
            min_value: None,
            max_value: None,
        }]
    }

    fn transfer_buffers_from(&mut self, old: &mut Self) {
        self.buffer.copy_overlap_from(&old.buffer);
    }

    fn get_buffer_output(&self, port: &str) -> Option<&Arc<BufferData>> {
        match port {
            "buffer" => Some(&self.buffer),
            _ => None,
        }
    }
}

#[derive(Default)]
struct BufferWriteState {}

/// Writes an input signal into a circular buffer each sample tick.
/// The buffer is owned by this module and exposed as a buffer output.
/// Readers ($delayRead, $bufRead) connect to it via buffer params.
#[module(name = "$buffer", has_init, args(input, length))]
pub struct BufferWrite {
    params: BufferWriteParams,
    outputs: BufferWriteOutputs,
    state: BufferWriteState,
}

impl BufferWrite {
    fn init(&mut self, sample_rate: f32) {
        let frame_count = (self.params.length * sample_rate as f64).ceil() as usize;
        let channel_count = self._channel_count.max(1);
        self.outputs.buffer = Arc::new(BufferData::new_zeroed(channel_count, frame_count));
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let frame_count = self.outputs.buffer.frame_count();

        if frame_count == 0 {
            for channel in 0..channels {
                self.outputs
                    .sample
                    .set(channel, self.params.input.get_value(channel));
            }
            return;
        }

        let write_index = self.outputs.buffer.read_write_index().wrapping_add(1);
        self.outputs.buffer.set_write_index(write_index);
        let frame = write_index % frame_count;
        let buffer_channels = self.outputs.buffer.channel_count();

        for channel in 0..channels {
            let input_val = self.params.input.get_value(channel);
            if channel < buffer_channels {
                self.outputs.buffer.write(channel, frame, input_val);
            }
            self.outputs.sample.set(channel, input_val);
        }
    }
}

message_handlers!(impl BufferWrite {});

fn buf_read_derive_channel_count(params: &BufReadParams) -> usize {
    params.buffer.channel_count()
}

fn read_interpolated(buffer: &Buffer, channel: usize, frame: f32) -> f32 {
    if !frame.is_finite() {
        return 0.0;
    }

    let frame_count = buffer.frame_count();
    if frame_count == 0 {
        return 0.0;
    }

    let max_frame = (frame_count - 1) as f32;
    if frame < 0.0 || frame > max_frame {
        return 0.0;
    }

    let left = frame.floor() as usize;
    let frac = frame - left as f32;
    if frac <= f32::EPSILON {
        return buffer.read(channel, left);
    }

    let i0 = left.saturating_sub(1);
    let i1 = left;
    let i2 = (left + 1).min(frame_count - 1);
    let i3 = (left + 2).min(frame_count - 1);

    let y0 = buffer.read(channel, i0);
    let y1 = buffer.read(channel, i1);
    let y2 = buffer.read(channel, i2);
    let y3 = buffer.read(channel, i3);

    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);

    ((c3 * frac + c2) * frac + c1) * frac + c0
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct BufReadParams {
    buffer: Buffer,
    /// read position (0 to 5V scales to 0 to buffer length)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    frame: MonoSignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BufReadOutputs {
    #[output("output", "buffer sample output", default)]
    sample: PolyOutput,
}

/// Reads a sample frame from a buffer and outputs its channels as a poly signal.
#[module(name = "$bufRead", channels_derive = buf_read_derive_channel_count, args(buffer, frame))]
pub struct BufRead {
    outputs: BufReadOutputs,
    params: BufReadParams,
}

impl BufRead {
    fn update(&mut self, _sample_rate: f32) {
        let frame_volts = self.params.frame.get_value().rem_euclid(5.0);
        let frame_count = self.params.buffer.frame_count();
        let frame = (frame_volts / 5.0) * frame_count as f32;
        let channels = self.channel_count();

        for channel in 0..channels {
            let value = read_interpolated(&self.params.buffer, channel, frame);
            self.outputs.sample.set(channel, value);
        }
    }
}

message_handlers!(impl BufRead {});

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::{
        BufferData, Patch,
        types::{Connect, OutputStruct, Signal},
    };

    fn mono(value: f32) -> MonoSignal {
        MonoSignal::from_poly(PolySignal::mono(Signal::Volts(value)))
    }

    /// A minimal mock module that owns a buffer and exposes it via get_buffer_output.
    /// Used in BufRead tests to provide buffer data without the old patch.buffers path.
    struct MockBufferOwner {
        id: String,
        buffer: Arc<BufferData>,
    }

    impl crate::types::MessageHandler for MockBufferOwner {}

    impl crate::types::Sampleable for MockBufferOwner {
        fn get_id(&self) -> &str {
            &self.id
        }
        fn tick(&self) {}
        fn update(&self) {}
        fn get_poly_sample(&self, _port: &str) -> napi::Result<PolyOutput> {
            Ok(PolyOutput::default())
        }
        fn get_module_type(&self) -> &str {
            "$buffer"
        }
        fn connect(&self, _patch: &Patch) {}
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn get_buffer_output(&self, port: &str) -> Option<&Arc<BufferData>> {
            match port {
                "buffer" => Some(&self.buffer),
                _ => None,
            }
        }
    }

    const MOCK_BUFFER_MODULE_ID: &str = "test-buffer-owner";
    const MOCK_BUFFER_PORT: &str = "buffer";

    fn make_connected_buffer(samples: Vec<Vec<f32>>) -> (Patch, Buffer) {
        let channels = samples.len();
        let mut patch = Patch::new();

        let owner = MockBufferOwner {
            id: MOCK_BUFFER_MODULE_ID.to_string(),
            buffer: Arc::new(BufferData::from_samples(samples)),
        };
        patch
            .sampleables
            .insert(MOCK_BUFFER_MODULE_ID.to_string(), Box::new(owner));

        let mut buffer = Buffer::new(
            MOCK_BUFFER_MODULE_ID.to_string(),
            MOCK_BUFFER_PORT.to_string(),
            channels,
        );
        buffer.connect(&patch);
        (patch, buffer)
    }

    fn make_buf_read(params: BufReadParams) -> BufRead {
        let channels = buf_read_derive_channel_count(&params);
        let mut outputs = BufReadOutputs::default();
        outputs.set_all_channels(channels);
        BufRead {
            params,
            outputs,
            _channel_count: channels,
        }
    }

    #[test]
    fn buf_read_channel_count_matches_buffer_channels() {
        let params = BufReadParams {
            buffer: Buffer::new("some-module".to_string(), "buffer".to_string(), 2),
            frame: mono(0.0),
        };

        assert_eq!(buf_read_derive_channel_count(&params), 2);
    }

    #[test]
    fn buf_read_outputs_all_buffer_channels() {
        // 0-5V maps across the full buffer length. With 2 frames, 2.5V = frame 1.0.
        // Channel 0: [1.0, 3.0], Channel 1: [2.0, 4.0]
        // At frame 1.0: ch0=3.0, ch1=4.0
        let (_patch, buffer) = make_connected_buffer(vec![vec![1.0, 3.0], vec![2.0, 4.0]]);
        let mut module = make_buf_read(BufReadParams {
            buffer,
            frame: mono(2.5), // 2.5V = midpoint of 0-5V range = frame 1.0
        });

        module.update(48_000.0);

        assert_eq!(module.outputs.sample.channels(), 2);
        assert!((module.outputs.sample.get(0) - 3.0).abs() < 1e-6);
        assert!((module.outputs.sample.get(1) - 4.0).abs() < 1e-6);
    }

    #[test]
    fn buf_read_returns_zero_out_of_range() {
        let (_patch, buffer) = make_connected_buffer(vec![vec![1.0, 3.0], vec![2.0, 4.0]]);
        let mut module = make_buf_read(BufReadParams {
            buffer,
            frame: mono(9.0),
        });

        module.update(48_000.0);

        assert_eq!(module.outputs.sample.get(0), 0.0);
        assert_eq!(module.outputs.sample.get(1), 0.0);
    }

    // ─── BufferWrite integration tests ───────────────────────────────────────

    use crate::dsp::{get_constructors, get_params_deserializers};
    use crate::params::DeserializedParams;
    use crate::types::Sampleable;

    const SAMPLE_RATE: f32 = 48000.0;

    fn make_module(module_type: &str, id: &str, params: serde_json::Value) -> Box<dyn Sampleable> {
        let constructors = get_constructors();
        let deserializers = get_params_deserializers();
        let deserializer = deserializers
            .get(module_type)
            .unwrap_or_else(|| panic!("no params deserializer for '{module_type}'"));
        let cached = deserializer(params)
            .unwrap_or_else(|e| panic!("params deserialization for '{module_type}' failed: {e}"));
        let deserialized = DeserializedParams {
            params: cached.params,
            argument_spans: Default::default(),
            channel_count: cached.channel_count,
        };
        constructors
            .get(module_type)
            .unwrap_or_else(|| panic!("no constructor for '{module_type}'"))(
            &id.to_string(),
            SAMPLE_RATE,
            deserialized,
        )
        .unwrap_or_else(|e| panic!("constructor for '{module_type}' failed: {e}"))
    }

    fn step(module: &dyn Sampleable) {
        module.tick();
        module.update();
    }

    #[test]
    fn buffer_write_passthrough_matches_input() {
        let module = make_module(
            "$buffer",
            "bw-passthrough",
            serde_json::json!({ "input": 3.0, "length": 0.01 }),
        );

        step(module.as_ref());

        let output = module
            .get_poly_sample("output")
            .expect("get_poly_sample failed");
        assert!(
            (output.get(0) - 3.0).abs() < 1e-6,
            "expected passthrough of 3.0, got {}",
            output.get(0)
        );
    }

    #[test]
    fn buffer_write_increments_write_index() {
        let module = make_module(
            "$buffer",
            "bw-index",
            serde_json::json!({ "input": 1.0, "length": 0.01 }),
        );

        let n = 10;
        for _ in 0..n {
            step(module.as_ref());
        }

        let buffer = module
            .get_buffer_output("buffer")
            .expect("no buffer output");
        assert_eq!(
            buffer.read_write_index(),
            n,
            "write_index should equal number of steps"
        );
    }

    #[test]
    fn buffer_write_writes_to_buffer() {
        let input_val = 7.5;
        let module = make_module(
            "$buffer",
            "bw-write",
            serde_json::json!({ "input": input_val, "length": 0.01 }),
        );

        step(module.as_ref());

        let buffer = module
            .get_buffer_output("buffer")
            .expect("no buffer output");
        let frame = buffer.read_write_index() % buffer.frame_count();
        let written = buffer.read(0, frame);
        assert!(
            (written - input_val as f32).abs() < 1e-6,
            "expected buffer[0][{frame}] = {input_val}, got {written}"
        );
    }

    #[test]
    fn buffer_write_wraps_circularly() {
        // At 48 kHz, length=0.0001s → frame_count = ceil(48000 * 0.0001) = ceil(4.8) = 5
        let module = make_module(
            "$buffer",
            "bw-wrap",
            serde_json::json!({ "input": 1.0, "length": 0.0001 }),
        );

        let buffer = module
            .get_buffer_output("buffer")
            .expect("no buffer output");
        let frame_count = buffer.frame_count();
        assert_eq!(frame_count, 5, "expected 5 frames for 0.0001s at 48kHz");

        // Step more than frame_count times to force wrapping
        let total_steps = frame_count + 3;
        for _ in 0..total_steps {
            step(module.as_ref());
        }

        // write_index should keep incrementing past frame_count (no modular reset)
        let write_index = buffer.read_write_index();
        assert_eq!(
            write_index, total_steps,
            "write_index should be {total_steps}, got {write_index}"
        );

        let last_frame = write_index % frame_count;
        let value = buffer.read(0, last_frame);
        assert!(
            (value - 1.0).abs() < 1e-6,
            "expected latest written frame to contain 1.0, got {value}"
        );

        // All frames should contain 1.0 since we've written more than frame_count times
        // with a constant input of 1.0
        for frame in 0..frame_count {
            let v = buffer.read(0, frame);
            assert!(
                (v - 1.0).abs() < 1e-6,
                "expected buffer[0][{frame}] = 1.0, got {v}"
            );
        }
    }
}
