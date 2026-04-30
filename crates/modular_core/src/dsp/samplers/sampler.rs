use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    Wav,
    dsp::utils::SchmittTrigger,
    poly::{MonoSignal, MonoSignalExt, PolyOutput},
};

fn sampler_derive_channel_count(params: &SamplerParams) -> usize {
    params.wav.channel_count()
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct SamplerParams {
    wav: Wav,
    /// Gate input — rising edge starts playback from the beginning.
    #[signal(type = gate, range = (0.0, 5.0))]
    gate: MonoSignal,
    /// Playback speed. 1.0 = normal, 2.0 = double speed, negative = reverse.
    #[signal(default = 1.0, range = (-4.0, 4.0))]
    #[deserr(default)]
    speed: Option<MonoSignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SamplerOutputs {
    #[output("output", "sample playback output", default)]
    sample: PolyOutput,
}

#[derive(Default)]
struct SamplerState {
    position: f64,
    playing: bool,
    gate_trigger: SchmittTrigger,
}

/// One-shot sample player. Plays a loaded WAV file from the beginning on each
/// gate rising edge. Speed control allows pitch-shifting and reverse playback.
///
/// ```js
/// $sampler($wavs().kick, $pulse('4hz'))
/// $sampler($wavs().tables.pad, $clock.beat, { speed: 0.5 })
/// ```
#[module(name = "$sampler", channels_derive = sampler_derive_channel_count, args(wav, gate))]
pub struct Sampler {
    params: SamplerParams,
    outputs: SamplerOutputs,
    state: SamplerState,
}

impl Sampler {
    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        let frame_count = self.params.wav.frame_count();

        // Detect gate rising edge
        let gate_val = self.params.gate.get_value();
        if self.state.gate_trigger.process(gate_val) {
            let speed = self.params.speed.value_or(1.0) as f64;
            // Start from the appropriate end based on playback direction
            if speed < 0.0 && frame_count > 0 {
                self.state.position = (frame_count - 1) as f64;
            } else {
                self.state.position = 0.0;
            }
            self.state.playing = true;
        }

        if !self.state.playing || !self.params.wav.is_loaded() || frame_count == 0 {
            for ch in 0..channels {
                self.outputs.sample.set(ch, 0.0);
            }
            return;
        }

        let speed = self.params.speed.value_or(1.0) as f64;
        let max_frame = (frame_count - 1) as f64;

        // Check bounds
        if self.state.position < 0.0 || self.state.position > max_frame {
            self.state.playing = false;
            for ch in 0..channels {
                self.outputs.sample.set(ch, 0.0);
            }
            return;
        }

        // Read with Hermite interpolation
        let pos = self.state.position as f32;
        for ch in 0..channels {
            let value = self.params.wav.read_hermite_clamped(ch, pos);
            self.outputs.sample.set(ch, value);
        }

        // Advance position, compensating for sample rate difference
        let wav_rate = self.params.wav.sample_rate() as f64;
        let rate_ratio = if wav_rate > 0.0 && sample_rate > 0.0 {
            wav_rate / sample_rate as f64
        } else {
            1.0
        };
        self.state.position += speed * rate_ratio;
    }
}

message_handlers!(impl Sampler {});

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::dsp::{get_constructors, get_params_deserializers};
    use crate::params::DeserializedParams;
    use crate::patch::Patch;
    use crate::types::{SampleBuffer, Sampleable, WavData};

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

    fn make_test_wav(samples: Vec<Vec<f32>>) -> Arc<WavData> {
        Arc::new(WavData::new(
            SampleBuffer::from_samples(samples, SAMPLE_RATE),
            None,
        ))
    }

    #[test]
    fn sampler_outputs_silence_when_not_triggered() {
        let wav_data = make_test_wav(vec![vec![1.0, 2.0, 3.0, 4.0]]);
        let module = make_module(
            "$sampler",
            "s1",
            serde_json::json!({
                "wav": { "type": "wav_ref", "path": "test", "channels": 1 },
                "gate": 0.0,
                "speed": 1.0,
            }),
        );

        // Connect with wav_data in patch
        let mut patch = Patch::new();
        patch.wav_data.insert("test".to_string(), wav_data);
        module.connect(&patch);

        // Run a few samples — no trigger, should output silence
        for _ in 0..4 {
            step(module.as_ref());
        }

        let output = module.get_poly_sample("output").unwrap();
        assert_eq!(output.get(0), 0.0);
    }

    #[test]
    fn sampler_plays_on_gate_rising_edge() {
        // 4-frame mono WAV: [1.0, 2.0, 3.0, 4.0]
        let wav_data = make_test_wav(vec![vec![1.0, 2.0, 3.0, 4.0]]);
        let module = make_module(
            "$sampler",
            "s2",
            serde_json::json!({
                "wav": { "type": "wav_ref", "path": "test", "channels": 1 },
                "gate": 5.0,
                "speed": 1.0,
            }),
        );

        let mut patch = Patch::new();
        patch.wav_data.insert("test".to_string(), wav_data);
        module.connect(&patch);

        // First tick: gate is high, Schmitt trigger detects rising edge, position resets to 0
        step(module.as_ref());
        let out = module.get_poly_sample("output").unwrap();
        assert!(
            (out.get(0) - 1.0).abs() < 1e-6,
            "expected 1.0, got {}",
            out.get(0)
        );
    }

    #[test]
    fn sampler_outputs_silence_after_sample_ends() {
        let wav_data = make_test_wav(vec![vec![1.0, 2.0]]);
        let module = make_module(
            "$sampler",
            "s3",
            serde_json::json!({
                "wav": { "type": "wav_ref", "path": "test", "channels": 1 },
                "gate": 5.0,
                "speed": 1.0,
            }),
        );

        let mut patch = Patch::new();
        patch.wav_data.insert("test".to_string(), wav_data);
        module.connect(&patch);

        // Play through the 2-frame sample
        step(module.as_ref()); // frame 0 -> output 1.0
        step(module.as_ref()); // frame 1 -> output 2.0
        step(module.as_ref()); // frame 2 -> past end -> silence

        let out = module.get_poly_sample("output").unwrap();
        assert_eq!(out.get(0), 0.0, "should be silent after sample ends");
    }

    #[test]
    fn sampler_plays_reverse_with_negative_speed() {
        // 4-frame mono WAV: [1.0, 2.0, 3.0, 4.0]
        let wav_data = make_test_wav(vec![vec![1.0, 2.0, 3.0, 4.0]]);
        let module = make_module(
            "$sampler",
            "s_rev",
            serde_json::json!({
                "wav": { "type": "wav_ref", "path": "test", "channels": 1 },
                "gate": 5.0,
                "speed": -1.0,
            }),
        );

        let mut patch = Patch::new();
        patch.wav_data.insert("test".to_string(), wav_data);
        module.connect(&patch);

        // With negative speed, gate trigger should start from end of sample.
        // Frame 3 = 4.0, frame 2 = 3.0, frame 1 = 2.0, frame 0 = 1.0
        step(module.as_ref()); // trigger + play from end: frame 3
        let out = module.get_poly_sample("output").unwrap();
        assert!(
            (out.get(0) - 4.0).abs() < 1e-6,
            "reverse frame 0: expected 4.0, got {}",
            out.get(0)
        );

        step(module.as_ref()); // frame 2
        let out = module.get_poly_sample("output").unwrap();
        assert!(
            (out.get(0) - 3.0).abs() < 1e-6,
            "reverse frame 1: expected 3.0, got {}",
            out.get(0)
        );

        step(module.as_ref()); // frame 1
        let out = module.get_poly_sample("output").unwrap();
        assert!(
            (out.get(0) - 2.0).abs() < 1e-6,
            "reverse frame 2: expected 2.0, got {}",
            out.get(0)
        );

        step(module.as_ref()); // frame 0
        let out = module.get_poly_sample("output").unwrap();
        assert!(
            (out.get(0) - 1.0).abs() < 1e-6,
            "reverse frame 3: expected 1.0, got {}",
            out.get(0)
        );

        // After passing frame 0, should be silent
        step(module.as_ref());
        let out = module.get_poly_sample("output").unwrap();
        assert_eq!(
            out.get(0),
            0.0,
            "should be silent after reverse playback ends"
        );
    }

    #[test]
    fn sampler_plays_stereo_wav() {
        // 3-frame stereo WAV: L=[1.0, 2.0, 3.0], R=[4.0, 5.0, 6.0]
        let wav_data = make_test_wav(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
        let module = make_module(
            "$sampler",
            "s4",
            serde_json::json!({
                "wav": { "type": "wav_ref", "path": "stereo", "channels": 2 },
                "gate": 5.0,
                "speed": 1.0,
            }),
        );

        let mut patch = Patch::new();
        patch.wav_data.insert("stereo".to_string(), wav_data);
        module.connect(&patch);

        // First tick: gate rises, plays frame 0
        step(module.as_ref());
        let out = module.get_poly_sample("output").unwrap();
        assert!(
            (out.get(0) - 1.0).abs() < 1e-6,
            "L ch should be 1.0, got {}",
            out.get(0)
        );
        assert!(
            (out.get(1) - 4.0).abs() < 1e-6,
            "R ch should be 4.0, got {}",
            out.get(1)
        );

        // Second tick: frame 1
        step(module.as_ref());
        let out = module.get_poly_sample("output").unwrap();
        assert!(
            (out.get(0) - 2.0).abs() < 1e-6,
            "L ch should be 2.0, got {}",
            out.get(0)
        );
        assert!(
            (out.get(1) - 5.0).abs() < 1e-6,
            "R ch should be 5.0, got {}",
            out.get(1)
        );
    }
}
