//! Looping playback of a media file's decoded audio track, for `$v.video(...).audio`.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    Wav,
    param_errors::ModuleParamErrors,
    poly::{PORT_MAX_CHANNELS, PolyOutput},
    types::hermite_clamped,
};

/// Output width follows the audio track, capped at the port-wide channel limit.
fn media_audio_derive_channel_count(params: &MediaAudioParams) -> usize {
    params.wav.channel_count().clamp(1, PORT_MAX_CHANNELS)
}

fn default_speed() -> f64 {
    1.0
}

/// Fastest playback rate; matches the video element's.
const MAX_SPEED: f64 = 16.0;

/// Fade at each end of the loop, in seconds, that keeps the loop point from clicking.
const EDGE_FADE_SECONDS: f64 = 0.001;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields, validate = media_audio_validate_params -> ModuleParamErrors)]
struct MediaAudioParams {
    wav: Wav,
    /// Playback rate: 1 is normal speed, 0 is silent (default 1).
    #[serde(default = "default_speed")]
    #[deserr(default = default_speed())]
    speed: f64,
    /// Loop start in seconds (default 0).
    #[serde(default)]
    #[deserr(default)]
    loop_start: f64,
    /// Loop end in seconds; the end of the file by default.
    #[serde(default)]
    #[deserr(default)]
    loop_end: Option<f64>,
}

fn media_audio_validate_params(
    params: MediaAudioParams,
    _location: deserr::ValuePointerRef,
) -> Result<MediaAudioParams, ModuleParamErrors> {
    let mut errors = ModuleParamErrors::default();
    if !params.speed.is_finite() || !(0.0..=MAX_SPEED).contains(&params.speed) {
        errors.add(
            "speed".to_string(),
            format!("speed must be between 0 and {MAX_SPEED}"),
        );
    }
    if !params.loop_start.is_finite() || params.loop_start < 0.0 {
        errors.add(
            "loopStart".to_string(),
            "loopStart must be 0 or more seconds".to_string(),
        );
    }
    if let Some(end) = params.loop_end
        && (!end.is_finite() || end <= params.loop_start)
    {
        errors.add(
            "loopEnd".to_string(),
            "loopEnd must be later than loopStart".to_string(),
        );
    }
    if errors.is_empty() {
        Ok(params)
    } else {
        Err(errors)
    }
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MediaAudioOutputs {
    #[output("output", "audio of the media file", default)]
    sample: PolyOutput,
}

#[derive(Default)]
struct MediaAudioState {
    /// Playback position in frames of the audio track.
    position: f64,
    /// Whether `position` has been placed at the loop start. Carried across
    /// patch updates so playback continues where it was.
    started: bool,
    /// Engine sample rate, captured in init.
    sample_rate: f32,
    /// Track frames per engine sample at normal speed. The track's rate only
    /// resolves after connect(), so this is set in on_patch_update.
    rate_ratio: f64,
    /// Loop window in frames; `win_hi <= win_lo` means there is nothing to play.
    win_lo: f64,
    win_hi: f64,
    /// Fade length at each loop end, in frames.
    fade_frames: f64,
}

/// Plays the audio track of a media file in a loop, at a fixed speed, from
/// the loop start when the patch starts. The DSL creates one for each
/// `$v.video(...).audio`.
#[module(name = "_mediaAudio", channels_derive = media_audio_derive_channel_count, args(wav), has_init, patch_update)]
pub struct MediaAudio {
    params: MediaAudioParams,
    outputs: MediaAudioOutputs,
    state: MediaAudioState,
}

impl MediaAudio {
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let state = &mut self.state;
        if !self.params.wav.is_loaded() || state.win_hi <= state.win_lo || self.params.speed == 0.0
        {
            for ch in 0..channels {
                self.outputs.sample.set(ch, 0.0);
            }
            return;
        }

        let span = state.win_hi - state.win_lo;
        if state.position >= state.win_hi {
            state.position = state.win_lo + (state.position - state.win_lo) % span;
        } else if state.position < state.win_lo {
            state.position = state.win_lo;
        }

        let position = state.position;
        let fade = state.fade_frames;
        let gain = if fade > 0.0 {
            ((position - state.win_lo).min(state.win_hi - position) / fade).clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        // Read around the integer position so the fraction keeps full
        // precision however far into the track playback is.
        let left = position.floor() as usize;
        let start = left.saturating_sub(1);
        let local = (position - start as f64) as f32;
        for ch in 0..channels {
            let samples = self.params.wav.channel(ch);
            let window = &samples[start.min(samples.len())..(left + 3).min(samples.len())];
            // A wav's samples are -1 to 1; the engine's audio is -5 to 5V.
            self.outputs
                .sample
                .set(ch, hermite_clamped(window, local) * 5.0 * gain);
        }

        state.position += self.params.speed * state.rate_ratio;
    }
}

impl crate::types::PatchUpdateHandler for MediaAudio {
    fn on_patch_update(&mut self) {
        let wav_rate = self.params.wav.sample_rate() as f64;
        let engine_rate = self.state.sample_rate as f64;
        let frames = self.params.wav.frame_count();
        let state = &mut self.state;
        if wav_rate <= 0.0 || engine_rate <= 0.0 || frames == 0 {
            state.win_lo = 0.0;
            state.win_hi = 0.0;
            state.rate_ratio = 1.0;
            return;
        }
        let last = (frames - 1) as f64;
        state.rate_ratio = wav_rate / engine_rate;
        state.win_lo = (self.params.loop_start * wav_rate).min(last);
        state.win_hi = self
            .params
            .loop_end
            .map_or(last, |end| (end * wav_rate).min(last));
        state.fade_frames = EDGE_FADE_SECONDS * wav_rate;
        if !state.started {
            state.position = state.win_lo;
            state.started = true;
        }
    }
}

message_handlers!(impl MediaAudio {});

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::dsp::{get_constructors, get_params_deserializers};
    use crate::params::DeserializedParams;
    use crate::patch::Patch;
    use crate::types::{
        Connect, OutputStruct, PatchUpdateHandler, SampleBuffer, Sampleable, WavData,
    };

    const SAMPLE_RATE: f32 = 48000.0;

    fn module_with(params: serde_json::Value) -> Box<dyn Sampleable> {
        let deserializers = get_params_deserializers();
        let cached = deserializers["_mediaAudio"](params)
            .unwrap_or_else(|e| panic!("params deserialization failed: {e}"));
        get_constructors()["_mediaAudio"](
            &"m".to_string(),
            SAMPLE_RATE,
            DeserializedParams {
                params: cached.params,
                channel_count: cached.channel_count,
            },
            1,
            crate::types::ProcessingMode::Block,
        )
        .unwrap_or_else(|e| panic!("constructor failed: {e}"))
    }

    fn params(extra: serde_json::Value) -> serde_json::Value {
        let mut v = json!({ "wav": { "type": "wav_ref", "path": "t", "channels": 1 } });
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    }

    /// A mono track whose sample `i` is `i / 10000`, so the output names the frame.
    fn ramp_wav(frames: usize) -> Arc<WavData> {
        let samples = (0..frames).map(|i| i as f32 / 10_000.0).collect();
        Arc::new(WavData::new(
            SampleBuffer::from_samples(vec![samples], SAMPLE_RATE),
            None,
        ))
    }

    fn connected(extra: serde_json::Value, frames: usize) -> Box<dyn Sampleable> {
        let module = module_with(params(extra));
        let mut patch = Patch::new();
        patch.wav_data.insert("t".to_string(), ramp_wav(frames));
        module.connect(&patch);
        module.on_patch_update();
        module
    }

    fn tick(module: &dyn Sampleable) -> f32 {
        module.start_block();
        module.ensure_processed();
        module.get_value_at("output", 0, 0)
    }

    /// The frame a ramp-track output names: the output is `frame / 10000 * 5`.
    fn frame_of(output: f32) -> f32 {
        output / 5.0 * 10_000.0
    }

    #[test]
    fn plays_from_the_loop_start() {
        let module = connected(json!({ "loopStart": 0.01 }), 4800);
        let outputs: Vec<f32> = (0..200).map(|_| tick(module.as_ref())).collect();
        // Past the fade at the loop start, sample n is frame 480 + n.
        assert!((frame_of(outputs[100]) - 580.0).abs() < 0.01);
        assert!((frame_of(outputs[199]) - 679.0).abs() < 0.01);
    }

    #[test]
    fn fades_in_from_silence_at_the_loop_start() {
        let module = connected(json!({ "loopStart": 0.01 }), 4800);
        assert_eq!(tick(module.as_ref()), 0.0);
        // Ten frames into a 48-frame fade the gain is about a fifth.
        let early = (0..10).map(|_| tick(module.as_ref())).last().unwrap();
        let full = 490.0 / 10_000.0 * 5.0;
        assert!(early > 0.0 && early < full * 0.5, "early = {early}");
    }

    #[test]
    fn loops_back_to_the_start_at_the_loop_end() {
        let module = connected(json!({ "loopEnd": 0.01 }), 4800);
        // The loop is 480 frames long; after one pass the frames repeat.
        let first: Vec<f32> = (0..480).map(|_| tick(module.as_ref())).collect();
        let second: Vec<f32> = (0..480).map(|_| tick(module.as_ref())).collect();
        assert!((first[100] - second[100]).abs() < 1e-4);
        assert!((first[300] - second[300]).abs() < 1e-4);
    }

    #[test]
    fn speed_scales_how_far_each_sample_advances() {
        let module = connected(json!({ "speed": 2.0 }), 4800);
        let outputs: Vec<f32> = (0..200).map(|_| tick(module.as_ref())).collect();
        assert!((frame_of(outputs[150]) - 300.0).abs() < 0.01);
    }

    #[test]
    fn speed_zero_is_silent() {
        let module = connected(json!({ "speed": 0.0 }), 4800);
        for _ in 0..100 {
            assert_eq!(tick(module.as_ref()), 0.0);
        }
    }

    #[test]
    fn a_patch_update_keeps_playing_from_where_it_was() {
        let module = connected(json!({}), 48_000);
        for _ in 0..1000 {
            tick(module.as_ref());
        }
        module.on_patch_update();
        let next = tick(module.as_ref());
        assert!((frame_of(next) - 1000.0).abs() < 0.01);
    }

    #[test]
    fn is_silent_until_the_audio_is_connected() {
        let module = module_with(params(json!({})));
        module.on_patch_update();
        assert_eq!(tick(module.as_ref()), 0.0);
    }

    #[test]
    fn rejects_unplayable_settings() {
        let deserializers = get_params_deserializers();
        let try_params = |extra: serde_json::Value| deserializers["_mediaAudio"](params(extra));
        assert!(try_params(json!({})).is_ok());
        assert!(try_params(json!({ "speed": 16.0 })).is_ok());
        assert!(try_params(json!({ "speed": 17.0 })).is_err());
        assert!(try_params(json!({ "speed": -1.0 })).is_err());
        assert!(try_params(json!({ "loopStart": -1.0 })).is_err());
        assert!(try_params(json!({ "loopStart": 1.0, "loopEnd": 1.0 })).is_err());
        assert!(try_params(json!({ "loopStart": 1.0, "loopEnd": 2.0 })).is_ok());
    }
}
