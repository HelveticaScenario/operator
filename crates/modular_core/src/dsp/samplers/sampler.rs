use deserr::Deserr;
use schemars::JsonSchema;

use super::slice::SliceParam;
use crate::{
    Wav,
    dsp::utils::SchmittTrigger,
    param_errors::ModuleParamErrors,
    poly::{MonoSignal, MonoSignalExt, PORT_MAX_CHANNELS, PolyOutput},
};

/// Output width follows the WAV, capped at the port-wide channel limit — a
/// WAV header can claim any u16 channel count.
fn sampler_derive_channel_count(params: &SamplerParams) -> usize {
    params.wav.channel_count().clamp(1, PORT_MAX_CHANNELS)
}

fn default_fade() -> f64 {
    1.0
}

/// Upper bound on the declick fade time.
const MAX_FADE_MS: f64 = 1000.0;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields, validate = sampler_validate_params -> ModuleParamErrors)]
struct SamplerParams {
    wav: Wav,
    /// Gate input — rising edge starts playback from the beginning.
    #[signal(type = gate, range = (0.0, 5.0))]
    gate: MonoSignal,
    /// Playback speed. 1.0 = normal, 2.0 = double speed, negative = reverse.
    #[signal(default = 1.0, range = (-4.0, 4.0))]
    #[deserr(default)]
    speed: Option<MonoSignal>,
    /// Slice selector: a mono signal (whole file as one slice) or
    /// `[points, signal]` where points are fractions of the total length in
    /// [0, 1]. Sampled once per gate rising edge: the integer part picks the
    /// slice (wrapping), the fraction offsets the start into it.
    #[deserr(default)]
    slice: Option<SliceParam>,
    /// Declick fade in milliseconds: fade-in at onset, fade-out at the window
    /// exit edge, and retrigger crossfade. 0 disables.
    #[serde(default = "default_fade")]
    #[deserr(default = default_fade())]
    fade: f64,
}

fn sampler_validate_params(
    params: SamplerParams,
    _location: deserr::ValuePointerRef,
) -> Result<SamplerParams, ModuleParamErrors> {
    if !params.fade.is_finite() || params.fade < 0.0 || params.fade > MAX_FADE_MS {
        let mut err = ModuleParamErrors::default();
        err.add(
            "fade".to_string(),
            format!("fade must be between 0 and {MAX_FADE_MS} milliseconds"),
        );
        return Err(err);
    }
    Ok(params)
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
    /// Engine sample rate, captured in init.
    sample_rate: f32,
    /// wav_rate / engine_rate. The WAV's sample rate only resolves after
    /// connect(), so this is computed in on_patch_update, not init. Constant
    /// until the next patch update.
    rate_ratio: f64,
    /// Playback window in WAV frames, sampled at the gate rising edge. A
    /// playing voice keeps its trigger-time window across patch updates.
    win_lo: f64,
    win_hi: f64,
    /// Declick ramp half-width in WAV frames (fade seconds × wav rate).
    /// Param+wav-derived, so computed in on_patch_update; 0 disables fades.
    fade_frames: f64,
    /// Retrigger release-voice ramp length in output samples (fade seconds ×
    /// engine rate). Param-derived, so computed in on_patch_update.
    fade_out_samples: u32,
    /// Release voice: on retrigger the previous playback keeps sounding at a
    /// fading gain so the jump to the new onset doesn't click.
    rel_active: bool,
    rel_position: f64,
    rel_win_lo: f64,
    rel_win_hi: f64,
    rel_countdown: u32,
}

/// One-shot sample player. Each gate rising edge starts playback of the whole
/// file, or of one slice when `slice` is set: the slice signal is sampled at
/// the edge — integer part picks the slice point (wrapping), fraction offsets
/// the start into it. Speed control allows pitch-shifting and reverse
/// playback (a slice plays the same region backwards). A short `fade`
/// (milliseconds) declicks onsets, slice ends, and retriggers.
///
/// ```js
/// $sampler($wavs().kick, $pulse('4hz'))
/// $sampler($wavs().pad, $clock.beatTrigger, { speed: 0.5 })
/// $sampler($wavs().pad, $pulse('2hz'), { slice: [[0, 0.25, 0.5, 0.75], $cycle($p('0 1 2 3')).cv], fade: 2 })
/// ```
#[module(name = "$sampler", channels_derive = sampler_derive_channel_count, args(wav, gate), has_init, patch_update)]
pub struct Sampler {
    params: SamplerParams,
    outputs: SamplerOutputs,
    state: SamplerState,
}

impl Sampler {
    /// Capture the engine sample rate. Invoked by the `#[module]` proc macro at
    /// construction. The playback rate ratio is finished in on_patch_update,
    /// once the WAV is connected.
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;
    }

    /// Distance-to-edge declick gain: ramps linearly from 0 at either window
    /// edge to 1 a full ramp-width inside it. Position-based, so it is
    /// stateless, symmetric under reverse, and robust to live speed changes
    /// (fade time scales with |speed|). Windows shorter than two ramp widths
    /// peak below unity. `w == 0` (fade disabled) is unity.
    fn edge_gain(position: f64, lo: f64, hi: f64, w: f64) -> f32 {
        if w > 0.0 {
            let d = (position - lo).min(hi - position);
            (d / w).clamp(0.0, 1.0) as f32
        } else {
            1.0
        }
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let frame_count = self.params.wav.frame_count();

        if !self.params.wav.is_loaded() || frame_count == 0 {
            for ch in 0..channels {
                self.outputs.sample.set(ch, 0.0);
            }
            return;
        }
        let max_frame = (frame_count - 1) as f64;

        // Detect gate rising edge
        let gate_val = self.params.gate.get_value();
        if self.state.gate_trigger.process(gate_val) {
            let speed = self.params.speed.value_or(1.0) as f64;
            // Demote the current voice to the release voice so the jump to
            // the new onset crossfades instead of clicking. A retrigger
            // during an active release simply steals the slot.
            if self.state.playing && self.state.fade_out_samples > 0 {
                self.state.rel_position = self.state.position;
                self.state.rel_win_lo = self.state.win_lo;
                self.state.rel_win_hi = self.state.win_hi;
                self.state.rel_countdown = self.state.fade_out_samples;
                self.state.rel_active = true;
            }
            let (lo, hi) = match &self.params.slice {
                Some(slice) => {
                    // Sample-and-hold the slice signal at the edge.
                    let s = slice.signal.get_value_f64();
                    let n = slice.points.len();
                    let idx = (s.floor() as i64).rem_euclid(n as i64) as usize;
                    let frac = s - s.floor();
                    let start_p = slice.points[idx];
                    // Slice ends at the next point, or the end of the file
                    // for the last one; an unsorted successor clamps to a
                    // zero-length slice.
                    let end_p = if idx + 1 < n {
                        slice.points[idx + 1]
                    } else {
                        1.0
                    };
                    let end_p = end_p.clamp(0.0, 1.0).max(start_p);
                    let slice_lo = (start_p * frame_count as f64).min(max_frame);
                    let slice_hi = (end_p * frame_count as f64).min(max_frame);
                    (slice_lo + frac * (slice_hi - slice_lo), slice_hi)
                }
                None => (0.0, max_frame),
            };
            self.state.win_lo = lo;
            self.state.win_hi = hi;
            // Reverse playback enters through the window's far edge: the same
            // audible region, played backwards.
            self.state.position = if speed < 0.0 { hi } else { lo };
            self.state.playing = true;
        }

        let speed = self.params.speed.value_or(1.0) as f64;
        let advance = speed * self.state.rate_ratio;
        let w = self.state.fade_frames;

        let mut main_gain = 0.0;
        if self.state.playing {
            if self.state.position < self.state.win_lo || self.state.position > self.state.win_hi {
                self.state.playing = false;
            } else {
                main_gain =
                    Self::edge_gain(self.state.position, self.state.win_lo, self.state.win_hi, w);
            }
        }

        let mut rel_gain = 0.0;
        if self.state.rel_active {
            if self.state.rel_countdown == 0
                || self.state.rel_position < self.state.rel_win_lo
                || self.state.rel_position > self.state.rel_win_hi
            {
                self.state.rel_active = false;
            } else {
                let pg = Self::edge_gain(
                    self.state.rel_position,
                    self.state.rel_win_lo,
                    self.state.rel_win_hi,
                    w,
                );
                // Time-based ramp-down bounds the ring-out to the fade time
                // regardless of speed; clamped because a patch update can
                // shrink fade_out_samples mid-release.
                let t = (self.state.rel_countdown as f64
                    / self.state.fade_out_samples.max(1) as f64)
                    .min(1.0) as f32;
                rel_gain = pg * t;
            }
        }

        if !self.state.playing && !self.state.rel_active {
            for ch in 0..channels {
                self.outputs.sample.set(ch, 0.0);
            }
            return;
        }

        // Read with Hermite interpolation
        let pos = self.state.position as f32;
        let rel_pos = self.state.rel_position as f32;
        for ch in 0..channels {
            let mut value = 0.0;
            if self.state.playing {
                // Expected range of wav is -1.0 to 1.0, multiply by 5 to get to oscillator level
                value += self.params.wav.read_hermite_clamped(ch, pos) * 5.0 * main_gain;
            }
            if self.state.rel_active {
                value += self.params.wav.read_hermite_clamped(ch, rel_pos) * 5.0 * rel_gain;
            }
            self.outputs.sample.set(ch, value);
        }

        // Advance positions, compensating for sample rate difference (ratio
        // resolved in on_patch_update).
        if self.state.playing {
            self.state.position += advance;
        }
        if self.state.rel_active {
            self.state.rel_position += advance;
            self.state.rel_countdown -= 1;
        }
    }
}

impl crate::types::PatchUpdateHandler for Sampler {
    fn on_patch_update(&mut self) {
        // wav.sample_rate() is only valid after connect() resolves the WAV data,
        // so the rate ratio is computed here rather than in init.
        let wav_rate = self.params.wav.sample_rate() as f64;
        let engine_rate = self.state.sample_rate as f64;
        self.state.rate_ratio = if wav_rate > 0.0 && engine_rate > 0.0 {
            wav_rate / engine_rate
        } else {
            1.0
        };
        let fade_secs = self.params.fade / 1000.0;
        self.state.fade_frames = fade_secs * wav_rate;
        self.state.fade_out_samples = (fade_secs * engine_rate).round() as u32;
    }
}

message_handlers!(impl Sampler {});

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::dsp::{get_constructors, get_params_deserializers};
    use crate::params::DeserializedParams;
    use crate::patch::Patch;
    use crate::poly::PolySignal;
    use crate::types::{
        Connect, OutputStruct, PatchUpdateHandler, SampleBuffer, Sampleable, Signal, WavData,
    };

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
            channel_count: cached.channel_count,
        };
        constructors
            .get(module_type)
            .unwrap_or_else(|| panic!("no constructor for '{module_type}'"))(
            &id.to_string(),
            SAMPLE_RATE,
            deserialized,
            TEST_BLOCK_SIZE,
            crate::types::ProcessingMode::Block,
        )
        .unwrap_or_else(|e| panic!("constructor for '{module_type}' failed: {e}"))
    }

    /// Block size used at construction by every test in this module. Bumping
    /// this exercises the wrapper's per-block dispatch — `Stepper` walks all
    /// `TEST_BLOCK_SIZE` slots between `start_block` calls.
    const TEST_BLOCK_SIZE: usize = 1;

    /// Per-sample cursor that hides block boundaries from tests. Each
    /// `tick()` returns the slot index to read; when the cursor wraps past
    /// `TEST_BLOCK_SIZE`, it triggers a new `start_block` + `ensure_processed`.
    struct Stepper {
        slot: usize,
    }

    impl Stepper {
        fn new() -> Self {
            // Initialise out-of-range so the first tick triggers a block.
            Self {
                slot: TEST_BLOCK_SIZE,
            }
        }

        /// Advance one sample. Returns the slot index to read this frame's
        /// outputs from. Multiple reads in the same frame (e.g. L+R of a
        /// stereo output) share the returned slot.
        fn tick(&mut self, module: &dyn Sampleable) -> usize {
            if self.slot >= TEST_BLOCK_SIZE {
                module.start_block();
                module.ensure_processed();
                self.slot = 0;
            }
            let s = self.slot;
            self.slot += 1;
            s
        }
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
                "fade": 0.0,
            }),
        );

        // Connect with wav_data in patch
        let mut patch = Patch::new();
        patch.wav_data.insert("test".to_string(), wav_data);
        module.connect(&patch);
        module.on_patch_update();

        // Run a few samples — no trigger, should output silence
        let mut s = Stepper::new();
        let mut last = 0;
        for _ in 0..4 {
            last = s.tick(module.as_ref());
        }
        assert_eq!(module.get_value_at("output", 0, last), 0.0);
    }

    #[test]
    fn sampler_plays_on_gate_rising_edge() {
        // 4-frame mono WAV with values in -1..1 range (output is scaled by 5.0)
        let wav_data = make_test_wav(vec![vec![0.2, 0.4, 0.6, 0.8]]);
        let module = make_module(
            "$sampler",
            "s2",
            serde_json::json!({
                "wav": { "type": "wav_ref", "path": "test", "channels": 1 },
                "gate": 5.0,
                "speed": 1.0,
                "fade": 0.0,
            }),
        );

        let mut patch = Patch::new();
        patch.wav_data.insert("test".to_string(), wav_data);
        module.connect(&patch);
        module.on_patch_update();

        // First tick: gate is high, Schmitt trigger detects rising edge, position resets to 0
        // 0.2 * 5.0 = 1.0
        let mut s = Stepper::new();
        let slot = s.tick(module.as_ref());
        let v = module.get_value_at("output", 0, slot);
        assert!((v - 1.0).abs() < 1e-6, "expected 1.0, got {v}");
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
                "fade": 0.0,
            }),
        );

        let mut patch = Patch::new();
        patch.wav_data.insert("test".to_string(), wav_data);
        module.connect(&patch);
        module.on_patch_update();

        // Play through the 2-frame sample
        let mut s = Stepper::new();
        s.tick(module.as_ref()); // frame 0 -> output 1.0
        s.tick(module.as_ref()); // frame 1 -> output 2.0
        let slot = s.tick(module.as_ref()); // frame 2 -> past end -> silence
        assert_eq!(
            module.get_value_at("output", 0, slot),
            0.0,
            "should be silent after sample ends"
        );
    }

    #[test]
    fn sampler_plays_reverse_with_negative_speed() {
        // 4-frame mono WAV in -1..1 range (output scaled by 5.0)
        let wav_data = make_test_wav(vec![vec![0.2, 0.4, 0.6, 0.8]]);
        let module = make_module(
            "$sampler",
            "s_rev",
            serde_json::json!({
                "wav": { "type": "wav_ref", "path": "test", "channels": 1 },
                "gate": 5.0,
                "speed": -1.0,
                "fade": 0.0,
            }),
        );

        let mut patch = Patch::new();
        patch.wav_data.insert("test".to_string(), wav_data);
        module.connect(&patch);
        module.on_patch_update();

        // With negative speed, gate trigger should start from end of sample.
        // Frame 3 = 0.8*5=4.0, frame 2 = 0.6*5=3.0, frame 1 = 0.4*5=2.0, frame 0 = 0.2*5=1.0
        let mut s = Stepper::new();
        let expected = [4.0_f32, 3.0, 2.0, 1.0];
        for (i, &want) in expected.iter().enumerate() {
            let slot = s.tick(module.as_ref());
            let v = module.get_value_at("output", 0, slot);
            assert!(
                (v - want).abs() < 1e-6,
                "reverse frame {i}: expected {want}, got {v}"
            );
        }

        let slot = s.tick(module.as_ref());
        assert_eq!(
            module.get_value_at("output", 0, slot),
            0.0,
            "should be silent after reverse playback ends"
        );
    }

    #[test]
    fn sampler_plays_stereo_wav() {
        // 3-frame stereo WAV in -1..1 range (output scaled by 5.0)
        // L=[0.2, 0.4, 0.6], R=[0.8, 1.0, -0.4]
        let wav_data = make_test_wav(vec![vec![0.2, 0.4, 0.6], vec![0.8, 1.0, -0.4]]);
        let module = make_module(
            "$sampler",
            "s4",
            serde_json::json!({
                "wav": { "type": "wav_ref", "path": "stereo", "channels": 2 },
                "gate": 5.0,
                "speed": 1.0,
                "fade": 0.0,
            }),
        );

        let mut patch = Patch::new();
        patch.wav_data.insert("stereo".to_string(), wav_data);
        module.connect(&patch);
        module.on_patch_update();

        let mut s = Stepper::new();

        // First tick: gate rises, plays frame 0
        // L: 0.2*5=1.0, R: 0.8*5=4.0
        let slot = s.tick(module.as_ref());
        let l = module.get_value_at("output", 0, slot);
        let r = module.get_value_at("output", 1, slot);
        assert!((l - 1.0).abs() < 1e-6, "L ch should be 1.0, got {l}");
        assert!((r - 4.0).abs() < 1e-6, "R ch should be 4.0, got {r}");

        // Second tick: frame 1
        // L: 0.4*5=2.0, R: 1.0*5=5.0
        let slot = s.tick(module.as_ref());
        let l = module.get_value_at("output", 0, slot);
        let r = module.get_value_at("output", 1, slot);
        assert!((l - 2.0).abs() < 1e-6, "L ch should be 2.0, got {l}");
        assert!((r - 5.0).abs() < 1e-6, "R ch should be 5.0, got {r}");
    }

    // === slice/fade param deserialization ===

    fn try_params(params: serde_json::Value) -> Result<(), String> {
        let deserializers = get_params_deserializers();
        let de = deserializers
            .get("$sampler")
            .expect("no params deserializer for $sampler");
        de(params).map(|_| ()).map_err(|e| e.to_string())
    }

    fn params_with(extra: serde_json::Value) -> serde_json::Value {
        let mut v = json!({
            "wav": { "type": "wav_ref", "path": "test", "channels": 1 },
            "gate": 0.0,
        });
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    }

    #[test]
    fn slice_param_accepts_bare_signal_and_tuple_forms() {
        for good in [
            json!(0.5),
            json!([0.0, 1.0]), // poly-array shorthand, summed to mono
            json!([[0.0, 0.5], 1.0]),
            json!([[0.0, 0.25, 0.5, 0.75], { "type": "cable", "module": "m", "port": "p" }]),
        ] {
            let r = try_params(params_with(json!({ "slice": good.clone() })));
            assert!(r.is_ok(), "expected acceptance for slice = {good}: {r:?}");
        }
    }

    #[test]
    fn slice_param_rejects_malformed_forms() {
        for bad in [
            json!([]),
            json!([[], 0.0]),
            json!([[0.0, 1.5], 0.0]),
            json!([[-0.1], 0.0]),
            json!([[0.0]]),
            json!([[0.0], 0.0, 0.0]),
        ] {
            let r = try_params(params_with(json!({ "slice": bad.clone() })));
            assert!(r.is_err(), "expected rejection for slice = {bad}");
        }
    }

    #[test]
    fn fade_param_bounds() {
        assert!(try_params(params_with(json!({}))).is_ok()); // defaulted
        assert!(try_params(params_with(json!({ "fade": 0.0 }))).is_ok());
        assert!(try_params(params_with(json!({ "fade": 1000.0 }))).is_ok());
        assert!(try_params(params_with(json!({ "fade": -1.0 }))).is_err());
        assert!(try_params(params_with(json!({ "fade": 1000.5 }))).is_err());
    }

    // === slice playback ===

    fn make_sliced(
        id: &str,
        wav_data: Arc<WavData>,
        extra: serde_json::Value,
    ) -> Box<dyn Sampleable> {
        let module = make_module("$sampler", id, params_with(extra));
        let mut patch = Patch::new();
        patch.wav_data.insert("test".to_string(), wav_data);
        module.connect(&patch);
        module.on_patch_update();
        module
    }

    fn expect_output(module: &dyn Sampleable, s: &mut Stepper, expected: &[f32]) {
        for (i, &want) in expected.iter().enumerate() {
            let slot = s.tick(module);
            let v = module.get_value_at("output", 0, slot);
            assert!(
                (v - want).abs() < 1e-4,
                "frame {i}: expected {want}, got {v}"
            );
        }
    }

    #[test]
    fn slice_plays_selected_slice() {
        // Slice 1 of [0, 0.5] covers the second half: frames 2, 3, then silence.
        let wav_data = make_test_wav(vec![vec![0.2, 0.4, 0.6, 0.8]]);
        let module = make_sliced(
            "sl1",
            wav_data,
            json!({ "gate": 5.0, "fade": 0.0, "slice": [[0.0, 0.5], 1.0] }),
        );
        expect_output(module.as_ref(), &mut Stepper::new(), &[3.0, 4.0, 0.0]);
    }

    #[test]
    fn slice_index_wraps_including_negatives() {
        let wav_data = make_test_wav(vec![vec![0.2, 0.4, 0.6, 0.8]]);
        // 2 wraps to slice 0: frames 0..2 (window hi = 0.5 × 4 = frame 2).
        let module = make_sliced(
            "sl_wrap_pos",
            wav_data.clone(),
            json!({ "gate": 5.0, "fade": 0.0, "slice": [[0.0, 0.5], 2.0] }),
        );
        expect_output(module.as_ref(), &mut Stepper::new(), &[1.0, 2.0, 3.0, 0.0]);
        // -1 wraps to slice 1: frames 2, 3.
        let module = make_sliced(
            "sl_wrap_neg",
            wav_data,
            json!({ "gate": 5.0, "fade": 0.0, "slice": [[0.0, 0.5], -1.0] }),
        );
        expect_output(module.as_ref(), &mut Stepper::new(), &[3.0, 4.0, 0.0]);
    }

    #[test]
    fn slice_fraction_offsets_start_into_slice() {
        // Signal 0.5 on a single whole-file slice: start halfway into the
        // window [0, 4] → frame 2, play to the end.
        let wav_data = make_test_wav(vec![vec![0.1, 0.2, 0.3, 0.4, 0.5]]);
        let module = make_sliced(
            "sl_frac",
            wav_data,
            json!({ "gate": 5.0, "fade": 0.0, "slice": [[0.0], 0.5] }),
        );
        expect_output(module.as_ref(), &mut Stepper::new(), &[1.5, 2.0, 2.5, 0.0]);
    }

    #[test]
    fn slice_reverse_plays_same_window_backwards() {
        // Same window as the fraction test, negative speed: enters at the far
        // edge and exits at the fraction-offset start.
        let wav_data = make_test_wav(vec![vec![0.1, 0.2, 0.3, 0.4, 0.5]]);
        let module = make_sliced(
            "sl_rev",
            wav_data,
            json!({ "gate": 5.0, "speed": -1.0, "fade": 0.0, "slice": [[0.0], 0.5] }),
        );
        expect_output(module.as_ref(), &mut Stepper::new(), &[2.5, 2.0, 1.5, 0.0]);
    }

    #[test]
    fn zero_length_slice_is_silent_with_fade() {
        let wav_data = make_test_wav(vec![vec![0.2; 4]]);
        // Duplicate points and unsorted points both collapse to a zero-length
        // window, which the declick gain keeps silent.
        for (id, points) in [
            ("sl_dup", json!([0.5, 0.5])),
            ("sl_unsorted", json!([0.7, 0.2])),
        ] {
            let module = make_sliced(
                id,
                wav_data.clone(),
                json!({ "gate": 5.0, "fade": 1.0, "slice": [points, 0.0] }),
            );
            expect_output(module.as_ref(), &mut Stepper::new(), &[0.0, 0.0, 0.0]);
        }
    }

    // === declick fades ===

    #[test]
    fn fade_ramps_at_onset_and_window_edges() {
        // 480-frame constant wav at 48kHz with a 1ms fade: the ramp width is
        // 48 frames on each side of the window.
        let wav_data = make_test_wav(vec![vec![0.2; 480]]);
        let module = make_sliced("sl_fade", wav_data, json!({ "gate": 5.0, "fade": 1.0 }));
        let mut s = Stepper::new();
        let mut out = Vec::with_capacity(481);
        for _ in 0..481 {
            let slot = s.tick(module.as_ref());
            out.push(module.get_value_at("output", 0, slot));
        }
        for (k, want) in [
            (0, 0.0),   // onset
            (24, 0.5),  // halfway up the ramp
            (48, 1.0),  // ramp complete
            (240, 1.0), // mid-file
            (455, 0.5), // 24 frames from the window edge
            (479, 0.0), // window edge
            (480, 0.0), // past the end
        ] {
            let v = out[k];
            assert!(
                (v - want).abs() < 1e-3,
                "sample {k}: expected {want}, got {v}"
            );
        }
    }

    // === retrigger crossfade (needs a mutable gate → direct construction) ===

    fn make_direct(params_json: serde_json::Value, wav_data: Arc<WavData>) -> Sampler {
        let mut params: SamplerParams = deserr::deserialize::<_, _, ModuleParamErrors>(params_json)
            .unwrap_or_else(|e| panic!("params deserialization failed: {e}"));
        let mut patch = Patch::new();
        patch.wav_data.insert("test".to_string(), wav_data);
        params.connect(&patch);
        let mut outputs = SamplerOutputs::default();
        outputs.set_all_channels(1);
        let mut sampler = Sampler {
            params,
            outputs,
            state: SamplerState::default(),
            _channel_count: 1,
            _block_index: Default::default(),
        };
        sampler.init(SAMPLE_RATE);
        sampler.on_patch_update();
        sampler
    }

    fn set_gate(sampler: &mut Sampler, volts: f32) {
        sampler.params.gate = MonoSignal::from_poly(PolySignal::mono(Signal::Volts(volts)));
    }

    #[test]
    fn retrigger_crossfades_without_discontinuity() {
        let wav_data = make_test_wav(vec![vec![0.2; 4800]]);
        let mut sampler = make_direct(params_with(json!({ "gate": 5.0, "fade": 1.0 })), wav_data);
        // Run past the 48-sample fade-in; steady state is unity (0.2 × 5).
        for _ in 0..100 {
            sampler.update(SAMPLE_RATE);
        }
        assert!((sampler.outputs.sample.get(0) - 1.0).abs() < 1e-4);

        // Drop the gate below the Schmitt low threshold, then retrigger.
        set_gate(&mut sampler, 0.0);
        sampler.update(SAMPLE_RATE);
        set_gate(&mut sampler, 5.0);

        // On a constant-amplitude wav the old voice's ramp-down and the new
        // voice's ramp-up sum to unity across the whole crossfade.
        for i in 0..48 {
            sampler.update(SAMPLE_RATE);
            let v = sampler.outputs.sample.get(0);
            assert!(
                (v - 1.0).abs() < 1e-3,
                "crossfade sample {i}: expected ~1.0, got {v}"
            );
        }
        for _ in 0..10 {
            sampler.update(SAMPLE_RATE);
        }
        assert!(!sampler.state.rel_active, "release voice should be spent");
        assert!((sampler.outputs.sample.get(0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn channel_count_clamps_to_port_max() {
        let deserializers = get_params_deserializers();
        let de = deserializers
            .get("$sampler")
            .expect("no params deserializer for $sampler");
        for (channels, want) in [(65535, PORT_MAX_CHANNELS), (0, 1)] {
            let cached = de(json!({
                "wav": { "type": "wav_ref", "path": "test", "channels": channels },
                "gate": 0.0,
            }))
            .expect("params should deserialize");
            assert_eq!(
                cached.channel_count, want,
                "channel count for a {channels}-channel wav"
            );
        }
    }

    #[test]
    fn retrigger_during_release_steals_the_slot() {
        let wav_data = make_test_wav(vec![vec![0.2; 4800]]);
        let mut sampler = make_direct(params_with(json!({ "gate": 5.0, "fade": 1.0 })), wav_data);
        for _ in 0..100 {
            sampler.update(SAMPLE_RATE);
        }
        // First retrigger: the ~100-frame-old voice becomes the release voice.
        set_gate(&mut sampler, 0.0);
        sampler.update(SAMPLE_RATE);
        set_gate(&mut sampler, 5.0);
        for _ in 0..10 {
            sampler.update(SAMPLE_RATE);
        }
        // Second retrigger mid-crossfade: the young main voice (position ~12)
        // steals the release slot from the old voice (position ~112).
        set_gate(&mut sampler, 0.0);
        sampler.update(SAMPLE_RATE);
        set_gate(&mut sampler, 5.0);
        sampler.update(SAMPLE_RATE);
        assert!(sampler.state.rel_active);
        assert!(
            sampler.state.rel_position < 20.0,
            "release slot should hold the young voice, got position {}",
            sampler.state.rel_position
        );
        assert_eq!(
            sampler.state.rel_countdown,
            sampler.state.fade_out_samples - 1
        );
    }
}
