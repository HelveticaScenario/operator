//! Audio driver.
//!
//! Two paths:
//!
//! 1. **Hardcoded sine** (default): plays a 440 Hz sine through the default
//!    output. This is the original Step-3 validation that the cpal stream
//!    works inside the gpui binary.
//!
//! 2. **Patch-driven** (`OPERATOR_ZED_PATCH_TEST=1`): builds a tiny
//!    hand-crafted `PatchGraph` (single `$sine` -> `$signal`/ROOT_OUTPUT),
//!    materializes it via `modular_core::patch::Patch::from_graph`, and
//!    drives it from the cpal callback. Validates the audio loop integration
//!    (next-session shopping-list item 4) end-to-end without needing the JS
//!    runtime. Once deno_core is wired and emits real graphs, this branch
//!    becomes the production path and the sine fallback can be deleted.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use modular_core::patch::Patch;
use modular_core::types::{ModuleState, PatchGraph, ROOT_ID, ROOT_OUTPUT_PORT};
use parking_lot::Mutex;

pub struct AudioEngine {
    _stream: cpal::Stream,
    pub state: Arc<Mutex<EngineState>>,
}

#[derive(Default)]
pub struct EngineState {
    pub frequency: f32,
    pub amplitude: f32,
    pub muted: bool,
}

impl AudioEngine {
    pub fn start() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default output device".to_string())?;
        let config = device
            .default_output_config()
            .map_err(|e| format!("default output config: {e}"))?;

        let sample_rate = config.sample_rate() as f32;
        let channels = config.channels() as usize;

        let muted_by_default = std::env::var("OPERATOR_ZED_MUTE")
            .map(|v| v != "0")
            .unwrap_or(true);
        let state = Arc::new(Mutex::new(EngineState {
            frequency: 440.0,
            amplitude: 0.15,
            muted: muted_by_default,
        }));

        let use_patch = std::env::var("OPERATOR_ZED_PATCH_TEST")
            .map(|v| v != "0")
            .unwrap_or(false);

        let mut driver: Driver = if use_patch {
            match build_test_patch(sample_rate) {
                Ok(patch) => {
                    eprintln!("[modz] audio: driving hand-crafted Patch (sine -> ROOT_OUTPUT)");
                    Driver::Patch(Box::new(patch))
                }
                Err(err) => {
                    eprintln!("[modz] audio: patch build failed ({err}); falling back to sine");
                    Driver::Sine { phase: 0.0 }
                }
            }
        } else {
            Driver::Sine { phase: 0.0 }
        };

        let stream_state = state.clone();
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_output_stream(
                    &config.into(),
                    move |buf: &mut [f32], _| {
                        driver.fill(buf, channels, sample_rate, &stream_state);
                    },
                    move |err| eprintln!("audio stream error: {err}"),
                    None,
                )
                .map_err(|e| format!("build_output_stream: {e}"))?,
            other => return Err(format!("unsupported sample format {other:?}")),
        };

        stream
            .play()
            .map_err(|e| format!("stream.play: {e}"))?;

        Ok(Self {
            _stream: stream,
            state,
        })
    }

    pub fn set_muted(&self, muted: bool) {
        self.state.lock().muted = muted;
    }
}

enum Driver {
    Sine { phase: f32 },
    Patch(Box<Patch>),
}

impl Driver {
    fn fill(
        &mut self,
        buf: &mut [f32],
        channels: usize,
        sample_rate: f32,
        state: &Arc<Mutex<EngineState>>,
    ) {
        let s = state.lock();
        if s.muted {
            for sample in buf.iter_mut() {
                *sample = 0.0;
            }
            return;
        }
        let freq = s.frequency;
        let amp = s.amplitude;
        drop(s);

        match self {
            Driver::Sine { phase } => {
                let phase_increment = freq * std::f32::consts::TAU / sample_rate;
                for frame in buf.chunks_mut(channels) {
                    let value = phase.sin() * amp;
                    for sample in frame.iter_mut() {
                        *sample = value;
                    }
                    *phase += phase_increment;
                    if *phase > std::f32::consts::TAU {
                        *phase -= std::f32::consts::TAU;
                    }
                }
            }
            Driver::Patch(patch) => {
                for frame in buf.chunks_mut(channels) {
                    for module in patch.sampleables.values() {
                        module.update();
                    }
                    let value = patch.get_output() * amp;
                    for sample in frame.iter_mut() {
                        *sample = value;
                    }
                }
            }
        }
    }
}

/// Hand-crafted PatchGraph used by `OPERATOR_ZED_PATCH_TEST=1`.
///
/// One `$sine` oscillator at C4 (0 V/Oct -> ~261.6 Hz) wired into the
/// well-known `ROOT_OUTPUT` `$signal` module. Mirrors the shape produced by
/// `factories.ts` -> `signalFactory(..., { id: 'ROOT_OUTPUT' })`.
fn build_test_patch(sample_rate: f32) -> Result<Patch, String> {
    let graph = PatchGraph {
        modules: vec![
            ModuleState {
                id: "osc1".to_string(),
                module_type: "$sine".to_string(),
                id_is_explicit: Some(true),
                params: serde_json::json!({ "freq": 0.0 }),
            },
            ModuleState {
                id: ROOT_ID.clone(),
                module_type: "$signal".to_string(),
                id_is_explicit: Some(true),
                params: serde_json::json!({
                    "source": {
                        "type": "cable",
                        "module": "osc1",
                        "port": *ROOT_OUTPUT_PORT,
                        "channel": 0,
                    }
                }),
            },
        ],
        module_id_remaps: None,
        scopes: Vec::new(),
    };
    Patch::from_graph(&graph, sample_rate)
}
