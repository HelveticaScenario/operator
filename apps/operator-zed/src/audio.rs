//! Audio driver.
//!
//! Build a single cpal output stream up front. Inside the audio callback,
//! poll a `crossbeam_channel::Receiver<Patch>` and hot-swap to whichever
//! `Patch` arrives last. Until the first `Patch` shows up the callback
//! falls back to a hardcoded 440 Hz sine so the binary is never silent.
//!
//! The sender is exposed back to `main.rs` so the cmd-S handler (and the
//! one-shot startup run if a file was passed on argv) can push freshly
//! built `Patch::from_graph(...)` values into the audio thread.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, TryRecvError, bounded};
use modular_core::patch::Patch;
use parking_lot::Mutex;

use crate::dsl_state::{SCOPE_RING_CAPACITY, ScopeTarget};

pub struct AudioEngine {
    _stream: cpal::Stream,
    pub state: Arc<Mutex<EngineState>>,
    pub patch_tx: Sender<Patch>,
    pub sample_rate: f32,
    /// Scope targets shared between the cpal callback (push samples) and
    /// the UI thread (read samples). Updated by `DslState::update_after_exec`.
    pub scope_targets: Arc<Mutex<Vec<ScopeTarget>>>,
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

        let (patch_tx, patch_rx) = bounded::<Patch>(4);
        let scope_targets: Arc<Mutex<Vec<ScopeTarget>>> = Arc::new(Mutex::new(Vec::new()));
        let mut driver = Driver::new(patch_rx, scope_targets.clone());

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
            patch_tx,
            sample_rate,
            scope_targets,
        })
    }

    pub fn set_muted(&self, muted: bool) {
        self.state.lock().muted = muted;
    }
}

struct Driver {
    patch_rx: Receiver<Patch>,
    patch: Option<Patch>,
    sine_phase: f32,
    scope_targets: Arc<Mutex<Vec<ScopeTarget>>>,
}

impl Driver {
    fn new(
        patch_rx: Receiver<Patch>,
        scope_targets: Arc<Mutex<Vec<ScopeTarget>>>,
    ) -> Self {
        Self {
            patch_rx,
            patch: None,
            sine_phase: 0.0,
            scope_targets,
        }
    }

    fn fill(
        &mut self,
        buf: &mut [f32],
        channels: usize,
        sample_rate: f32,
        state: &Arc<Mutex<EngineState>>,
    ) {
        // Drain pending patch updates; keep the most recent one.
        loop {
            match self.patch_rx.try_recv() {
                Ok(patch) => self.patch = Some(patch),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }

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

        match &self.patch {
            Some(patch) => {
                // Snapshot current scope targets so we don't hold the lock
                // across the entire fill (UI thread might be reading too).
                let scopes = self.scope_targets.lock().clone();
                for frame in buf.chunks_mut(channels) {
                    for module in patch.sampleables.values() {
                        module.update();
                    }
                    let value = patch.get_output() * amp;
                    for sample in frame.iter_mut() {
                        *sample = value;
                    }
                    // Sample each configured scope channel for this audio
                    // frame and push into its ring buffer.
                    for target in &scopes {
                        let Some(module) = patch.sampleables.get(&target.module_id) else {
                            continue;
                        };
                        let Ok(poly) = module.get_poly_sample(&target.port_name) else {
                            continue;
                        };
                        let v = poly.get(target.channel as usize);
                        let mut ring = target.samples.lock();
                        if ring.len() == SCOPE_RING_CAPACITY {
                            ring.pop_front();
                        }
                        ring.push_back(v);
                    }
                }
            }
            None => {
                let phase_increment = freq * std::f32::consts::TAU / sample_rate;
                for frame in buf.chunks_mut(channels) {
                    let value = self.sine_phase.sin() * amp;
                    for sample in frame.iter_mut() {
                        *sample = value;
                    }
                    self.sine_phase += phase_increment;
                    if self.sine_phase > std::f32::consts::TAU {
                        self.sine_phase -= std::f32::consts::TAU;
                    }
                }
            }
        }
    }
}
