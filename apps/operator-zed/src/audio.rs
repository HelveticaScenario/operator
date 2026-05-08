//! Audio driver. Stage-1: hardcoded 440 Hz sine through default cpal output.
//! Validates the cpal -> stream path inside the gpui binary before wiring
//! deno_core / modular_core in the next iteration.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
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

        let mut phase: f32 = 0.0;
        let stream_state = state.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_output_stream(
                    &config.into(),
                    move |buf: &mut [f32], _| {
                        fill_sine(buf, channels, sample_rate, &stream_state, &mut phase);
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

fn fill_sine(
    buf: &mut [f32],
    channels: usize,
    sample_rate: f32,
    state: &Arc<Mutex<EngineState>>,
    phase: &mut f32,
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
