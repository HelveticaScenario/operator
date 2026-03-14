#![deny(clippy::all)]

mod audio;
mod commands;
mod midi;
mod params_cache;
mod validation;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use napi::bindgen_prelude::Float32Array;
use std::{collections::HashMap, sync::Arc};

use modular_core::{
  PatchGraph,
  dsp::schema,
  types::{ROOT_CLOCK_ID, ScopeBufferKey, ScopeStats},
};
use napi::Result;
use napi_derive::napi;

use crate::audio::{
  ApplyPatchError, AudioBudgetSnapshot, AudioDeviceCache, AudioDeviceInfo, AudioSharedState,
  AudioState, CurrentAudioState, DeviceCacheSnapshot, GarbageProducer, HostDeviceInfo, HostInfo,
  InputBufferReader, InputBufferWriter, TransportSnapshot, create_audio_channels,
  create_input_ring_buffer, find_input_device_in_host, find_output_device_in_host,
  get_host_by_preference, make_input_stream, make_stream, preferred_default_sample_rate,
};
use crate::commands::{GraphCommand, QueuedTrigger};
use crate::midi::MidiInputManager;

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../reserved_output_names.rs"));

/// Returns the list of reserved output names that cannot be used as module output port names.
/// These are names that conflict with built-in properties/methods on ModuleOutput, Collection, etc.
#[napi]
pub fn get_reserved_output_names() -> Vec<String> {
    RESERVED_OUTPUT_NAMES.iter().map(|s| s.to_string()).collect()
}

/// Information about a MIDI input port (for N-API)
#[napi(object)]
pub struct MidiInputInfo {
  pub name: String,
  pub index: u32,
}

/// Result of a patch update, including any validation errors and the assigned update ID.
#[napi(object)]
pub struct PatchUpdateResult {
  pub errors: Vec<ApplyPatchError>,
  /// Unique ID for this update (as f64 since napi(object) doesn't support u64).
  pub update_id: f64,
}

/// Audio configuration for synthesizer initialization
#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AudioConfigOptions {
  pub host_id: Option<String>,
  pub output_device_id: Option<String>,
  pub input_device_id: Option<String>,
  pub sample_rate: Option<u32>,
  pub buffer_size: Option<u32>,
}

#[napi(js_name = "Synthesizer")]
pub struct Synthesizer {
  state: Arc<AudioState>,
  _output_stream: cpal::Stream,
  _input_stream: Option<cpal::Stream>,
  midi_manager: Arc<MidiInputManager>,
  sample_rate: f32,
  buffer_size: Option<u32>,
  channels: u16,
  input_channels: u16,
  is_recording: bool,
  host_id: String,
  output_device_id: Option<String>,
  input_device_id: Option<String>,
  device_cache: AudioDeviceCache,
  fallback_warning: Option<String>,
  next_update_id: u64,
}

/// Resolve devices from config, falling back to defaults if not found
fn resolve_devices(
  config: &AudioConfigOptions,
  fallback_warning: &mut Option<String>,
) -> (
  cpal::Host,
  String,
  cpal::Device,
  String,
  Option<cpal::Device>,
  Option<String>,
) {
  // Try to get requested host, fall back to preference
  let (host, host_id) = if let Some(ref requested_host_id) = config.host_id {
    // Try to parse host ID
    match parse_host_id(requested_host_id) {
      Some(host_id_enum) => match cpal::host_from_id(host_id_enum) {
        Ok(host) => (host, requested_host_id.clone()),
        Err(_) => {
          *fallback_warning = Some(format!(
            "Requested host '{}' not available, using default. ",
            requested_host_id
          ));
          let host = get_host_by_preference();
          let id = format!("{:?}", host.id());
          (host, id)
        }
      },
      None => {
        *fallback_warning = Some(format!(
          "Unknown host '{}', using default. ",
          requested_host_id
        ));
        let host = get_host_by_preference();
        let id = format!("{:?}", host.id());
        (host, id)
      }
    }
  } else {
    let host = get_host_by_preference();
    let id = format!("{:?}", host.id());
    (host, id)
  };

  // Try to get requested output device, fall back to default
  let (output_device, output_device_id) =
    if let Some(ref requested_device_id) = config.output_device_id {
      match find_output_device_in_host(&host, requested_device_id) {
        Some(device) => {
          let id = device
            .id()
            .ok()
            .map(|id| id.to_string())
            .unwrap_or_else(|| requested_device_id.clone());
          (device, id)
        }
        None => {
          *fallback_warning = Some(format!(
            "{}Output device '{}' not found, using default. ",
            fallback_warning.clone().unwrap_or_default(),
            requested_device_id
          ));
          let device = host
            .default_output_device()
            .expect("No default output device available");
          let id = device
            .id()
            .ok()
            .map(|id| id.to_string())
            .unwrap_or_default();
          (device, id)
        }
      }
    } else {
      let device = host
        .default_output_device()
        .expect("No default output device available");
      let id = device
        .id()
        .ok()
        .map(|id| id.to_string())
        .unwrap_or_default();
      (device, id)
    };

  // Try to get requested input device (None is valid = no input)
  let (input_device, input_device_id) = match &config.input_device_id {
    None => {
      // No input requested, try default
      match host.default_input_device() {
        Some(device) => {
          let id = device.id().ok().map(|id| id.to_string());
          (Some(device), id)
        }
        None => (None, None),
      }
    }
    Some(requested_device_id)
      if requested_device_id == "none" || requested_device_id.is_empty() =>
    {
      // Explicitly no input
      (None, None)
    }
    Some(requested_device_id) => match find_input_device_in_host(&host, requested_device_id) {
      Some(device) => {
        let id = device.id().ok().map(|id| id.to_string());
        (Some(device), id)
      }
      None => {
        *fallback_warning = Some(format!(
          "{}Input device '{}' not found, using default. ",
          fallback_warning.clone().unwrap_or_default(),
          requested_device_id
        ));
        match host.default_input_device() {
          Some(device) => {
            let id = device.id().ok().map(|id| id.to_string());
            (Some(device), id)
          }
          None => (None, None),
        }
      }
    },
  };

  (
    host,
    host_id,
    output_device,
    output_device_id,
    input_device,
    input_device_id,
  )
}

/// Parse a host ID string to cpal::HostId
fn parse_host_id(id: &str) -> Option<cpal::HostId> {
  match id {
    #[cfg(target_os = "macos")]
    "CoreAudio" => Some(cpal::HostId::CoreAudio),
    #[cfg(target_os = "windows")]
    "Wasapi" | "WASAPI" => Some(cpal::HostId::Wasapi),
    // #[cfg(target_os = "windows")]
    // "Asio" | "ASIO" => Some(cpal::HostId::Asio),
    #[cfg(target_os = "linux")]
    "Alsa" | "ALSA" => Some(cpal::HostId::Alsa),
    #[cfg(target_os = "linux")]
    "Jack" | "JACK" => Some(cpal::HostId::Jack),
    _ => None,
  }
}

/// Build output stream with the appropriate sample format
fn build_output_stream(
  device: &cpal::Device,
  config: &cpal::StreamConfig,
  sample_format: cpal::SampleFormat,
  command_rx: crate::audio::CommandConsumer,
  error_tx: crate::audio::ErrorProducer,
  garbage_tx: GarbageProducer,
  shared: AudioSharedState,
  input_reader: InputBufferReader,
) -> Result<cpal::Stream> {
  match sample_format {
    cpal::SampleFormat::I8 => make_stream::<i8>(
      device,
      config,
      command_rx,
      error_tx,
      garbage_tx,
      shared,
      input_reader,
    ),
    cpal::SampleFormat::I16 => make_stream::<i16>(
      device,
      config,
      command_rx,
      error_tx,
      garbage_tx,
      shared,
      input_reader,
    ),
    cpal::SampleFormat::I32 => make_stream::<i32>(
      device,
      config,
      command_rx,
      error_tx,
      garbage_tx,
      shared,
      input_reader,
    ),
    cpal::SampleFormat::F32 => make_stream::<f32>(
      device,
      config,
      command_rx,
      error_tx,
      garbage_tx,
      shared,
      input_reader,
    ),
    _ => Err(napi::Error::from_reason(format!(
      "Unsupported output sample format: {:?}",
      sample_format
    ))),
  }
}

/// Build input stream with the appropriate sample format
fn build_input_stream(
  device: &cpal::Device,
  config: &cpal::StreamConfig,
  sample_format: cpal::SampleFormat,
  input_writer: InputBufferWriter,
) -> Result<cpal::Stream> {
  match sample_format {
    cpal::SampleFormat::I8 => make_input_stream::<i8>(device, config, input_writer),
    cpal::SampleFormat::I16 => make_input_stream::<i16>(device, config, input_writer),
    cpal::SampleFormat::I32 => make_input_stream::<i32>(device, config, input_writer),
    cpal::SampleFormat::F32 => make_input_stream::<f32>(device, config, input_writer),
    _ => Err(napi::Error::from_reason(format!(
      "Unsupported input sample format: {:?}",
      sample_format
    ))),
  }
}

/// Result of setting up audio streams
struct StreamSetupResult {
  output_stream: cpal::Stream,
  input_stream: Option<cpal::Stream>,
  state: Arc<AudioState>,
  sample_rate: f32,
  channels: u16,
  input_channels: u16,
  input_device_id: Option<String>,
}

/// Common parameters for stream setup
struct StreamSetupParams<'a> {
  output_device: &'a cpal::Device,
  output_config: &'a cpal::SupportedStreamConfig,
  sample_rate: u32,
  buffer_size: Option<u32>,
  /// Input device with its config, or None for no input
  input: Option<(&'a cpal::Device, cpal::SupportedStreamConfig, String)>,
  /// MIDI input manager (shared with audio thread)
  midi_manager: Arc<MidiInputManager>,
}

/// Set up output and input streams with the given parameters.
/// This is the shared core logic between `new` and `recreate_streams`.
fn setup_streams(params: StreamSetupParams) -> Result<StreamSetupResult> {
  let channels = params.output_config.channels();

  // Build stream config
  let stream_buffer_size = params
    .buffer_size
    .map(cpal::BufferSize::Fixed)
    .unwrap_or(cpal::BufferSize::Default);

  let stream_config = cpal::StreamConfig {
    channels,
    sample_rate: params.sample_rate,
    buffer_size: stream_buffer_size,
  };

  // Create command, error, and garbage queues for audio thread communication
  let (command_tx, command_rx, error_tx, error_rx, garbage_tx, garbage_rx) =
    create_audio_channels();

  // Create audio state handle (main thread side)
  let state = Arc::new(AudioState::new_with_channels(
    command_tx,
    error_rx,
    garbage_rx,
    params.sample_rate as f32,
    channels,
    params.midi_manager.clone(),
  ));

  // Get shared state for audio processor
  let shared = state.get_shared_state();

  // Get input channel count before creating ring buffer
  let input_channels = params
    .input
    .as_ref()
    .map(|(_, config, _)| config.channels() as usize)
    .unwrap_or(0);

  // Create ring buffer
  let (input_writer, input_reader) = create_input_ring_buffer(input_channels);

  // Create and start output stream
  let output_stream = build_output_stream(
    params.output_device,
    &stream_config,
    params.output_config.sample_format(),
    command_rx,
    error_tx,
    garbage_tx,
    shared,
    input_reader,
  )?;

  output_stream
    .play()
    .map_err(|e| napi::Error::from_reason(format!("Failed to start output stream: {}", e)))?;

  // Create and start input stream if configured
  let (input_stream, actual_input_device_id, final_input_channels) =
    if let Some((input_device, input_config, input_id)) = params.input {
      let input_stream_config = cpal::StreamConfig {
        channels: input_config.channels(),
        sample_rate: params.sample_rate,
        buffer_size: stream_buffer_size,
      };

      match build_input_stream(
        input_device,
        &input_stream_config,
        input_config.sample_format(),
        input_writer,
      ) {
        Ok(stream) => match stream.play() {
          Ok(()) => {
            println!(
              "Audio input: {} ({} channels, {} Hz)",
              input_id, input_channels, params.sample_rate
            );
            (Some(stream), Some(input_id), input_channels as u16)
          }
          Err(e) => {
            eprintln!("Failed to start input stream: {}", e);
            (None, None, 0)
          }
        },
        Err(e) => {
          eprintln!("Failed to create input stream: {}", e);
          (None, None, 0)
        }
      }
    } else {
      (None, None, 0)
    };

  Ok(StreamSetupResult {
    output_stream,
    input_stream,
    state,
    sample_rate: params.sample_rate as f32,
    channels,
    input_channels: final_input_channels,
    input_device_id: actual_input_device_id,
  })
}

#[napi]
impl Synthesizer {
  /// Create a new Synthesizer with optional audio configuration.
  /// If config is provided but invalid (device not found, sample rate unsupported),
  /// falls back to OS-preferred host and default devices.
  #[napi(constructor)]
  pub fn new(config: Option<AudioConfigOptions>) -> Result<Self> {
    let config = config.unwrap_or_default();

    // Build device cache first
    let device_cache = AudioDeviceCache::new();

    // Track any fallback warnings
    let mut fallback_warning: Option<String> = None;

    // Try to use requested config, fall back to defaults if invalid
    let (_host, host_id, output_device, output_device_id, input_device, input_device_id) =
      resolve_devices(&config, &mut fallback_warning);

    // Get output config
    let output_config = output_device.default_output_config().map_err(|err| {
      napi::Error::from_reason(format!("Failed to get default output config: {}", err))
    })?;

    // Determine sample rate (use requested if valid, else a sensible default)
    let sample_rate = if let Some(requested_rate) = config.sample_rate {
      // Check if the requested rate is supported
      if let Some(device_info) = device_cache.find_output_device(&output_device_id) {
        if device_info.supported_sample_rates.contains(&requested_rate) {
          requested_rate
        } else {
          let preferred = preferred_default_sample_rate(
            output_config.sample_rate(),
            &device_info.supported_sample_rates,
          );
          fallback_warning = Some(format!(
            "{}Requested sample rate {}Hz not supported, using {}Hz. ",
            fallback_warning.unwrap_or_default(),
            requested_rate,
            preferred,
          ));
          preferred
        }
      } else {
        output_config.sample_rate()
      }
    } else {
      // No sample rate in config (new user / empty config) — pick a
      // sensible default: use the device's native rate but cap it so
      // new users don't accidentally start at 96 kHz+.
      if let Some(device_info) = device_cache.find_output_device(&output_device_id) {
        preferred_default_sample_rate(
          output_config.sample_rate(),
          &device_info.supported_sample_rates,
        )
      } else {
        output_config.sample_rate()
      }
    };

    // Prepare input device info if available
    let input_setup = if let Some(ref input_dev) = input_device {
      match input_dev.default_input_config() {
        Ok(input_config) => {
          let id = input_device_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
          Some((input_dev, input_config, id))
        }
        Err(e) => {
          eprintln!("Failed to get input config: {}", e);
          None
        }
      }
    } else {
      None
    };

    println!(
      "Audio output: {} Hz, {} channels, host: {}",
      sample_rate,
      output_config.channels(),
      host_id
    );

    // Create MIDI manager first so it can be shared with audio thread
    let midi_manager = Arc::new(MidiInputManager::new());

    // Set up streams using shared helper
    let setup_result = setup_streams(StreamSetupParams {
      output_device: &output_device,
      output_config: &output_config,
      sample_rate,
      buffer_size: config.buffer_size,
      input: input_setup,
      midi_manager: midi_manager.clone(),
    })?;

    println!("Audio output stream started.");

    Ok(Self {
      state: setup_result.state,
      _output_stream: setup_result.output_stream,
      _input_stream: setup_result.input_stream,
      midi_manager,
      sample_rate: setup_result.sample_rate,
      buffer_size: config.buffer_size,
      channels: setup_result.channels,
      input_channels: setup_result.input_channels,
      is_recording: false,
      host_id,
      output_device_id: Some(output_device_id),
      input_device_id: setup_result.input_device_id,
      device_cache,
      fallback_warning,
      next_update_id: 0,
    })
  }

  #[napi]
  pub fn stop(&mut self) {
    self.state.set_stopped(true);
  }

  #[napi]
  pub fn is_stopped(&self) -> bool {
    self.state.is_stopped()
  }

  #[napi]
  pub fn sample_rate(&self) -> f32 {
    self.sample_rate
  }

  #[napi]
  pub fn channels(&self) -> u16 {
    self.channels
  }

  #[napi]
  pub fn input_channels(&self) -> u16 {
    self.input_channels
  }

  #[napi]
  pub fn get_scopes(&self) -> Vec<(ScopeBufferKey, Float32Array, ScopeStats)> {
    self.state.get_audio_buffers()
  }

  #[napi]
  pub fn update_patch(
    &mut self,
    patch: PatchGraph,
    trigger: Option<QueuedTrigger>,
  ) -> PatchUpdateResult {
    // Extract MIDI device names from MIDI modules and sync connections
    self.sync_midi_devices_from_patch(&patch);

    // Drain deferred deallocations from the audio thread
    self.state.drain_garbage();

    let trigger = trigger.unwrap_or(QueuedTrigger::Immediate);

    // Assign a unique update ID
    self.next_update_id += 1;
    let update_id = self.next_update_id;

    // Update transport meter with tempo/time signature from ROOT_CLOCK params
    if let Some(root_clock) = patch.modules.iter().find(|m| m.id == *ROOT_CLOCK_ID) {
      let bpm = root_clock
        .params
        .get("tempo")
        .and_then(|v| v.as_f64())
        .unwrap_or(120.0);
      let numerator = root_clock
        .params
        .get("numerator")
        .and_then(|v| v.as_u64())
        .unwrap_or(4) as u32;
      let denominator = root_clock
        .params
        .get("denominator")
        .and_then(|v| v.as_u64())
        .unwrap_or(4) as u32;
      self
        .state
        .transport_meter
        .write_tempo(bpm, numerator, denominator);
    }

    let errors = self
      .state
      .handle_set_patch(patch, self.sample_rate, trigger, update_id);

    PatchUpdateResult {
      errors,
      update_id: update_id as f64,
    }
  }

  /// Lightweight single-module param update. Bypasses full patch rebuild —
  /// only for modules already in the patch.
  #[napi]
  pub fn set_module_param(
    &self,
    module_id: String,
    module_type: String,
    params: serde_json::Value,
  ) -> Result<()> {
    // Deserialize on main thread (no cache — slider values would pollute it)
    let deserialized = deserialize_params(&module_type, params, false)?;
    self.state.send_command(GraphCommand::SingleParamUpdate {
      module_id,
      params: deserialized,
    })
  }

  /// Extract MIDI device names from patch modules and sync connections
  fn sync_midi_devices_from_patch(&self, patch: &PatchGraph) {
    use std::collections::HashSet;

    let mut devices: HashSet<String> = HashSet::new();

    for module in &patch.modules {
      // Check if this is a MIDI module type
      match module.module_type.as_str() {
        "midi.cv" | "midi.cc" | "midi.gate" => {
          // Extract device param from params JSON
          if let Some(device) = module.params.get("device").and_then(|v| v.as_str()) {
            if !device.is_empty() {
              devices.insert(device.to_string());
            }
          }
        }
        _ => {}
      }
    }

    // Sync MIDI manager connections
    self.midi_manager.sync_devices(&devices);
  }

  #[napi]
  pub fn start_recording(&mut self, path: Option<String>) -> Result<String> {
    if self.is_recording {
      return Err(napi::Error::from_reason(
        "Recording is already in progress".to_string(),
      ));
    }
    match self.state.start_recording(path) {
      Ok(p) => {
        self.is_recording = true;
        Ok(p)
      }
      Err(e) => Err(e),
    }
  }

  #[napi]
  pub fn stop_recording(&mut self) -> Result<Option<String>> {
    if !self.is_recording {
      return Err(napi::Error::from_reason(
        "No recording is in progress".to_string(),
      ));
    }
    match self.state.stop_recording() {
      Ok(p) => {
        self.is_recording = false;
        Ok(p)
      }
      Err(e) => Err(e),
    }
  }

  #[napi]
  pub fn is_recording(&self) -> bool {
    self.is_recording
  }

  #[napi]
  pub fn get_health(&self) -> AudioBudgetSnapshot {
    // Drain deferred deallocations from the audio thread (called periodically from JS)
    self.state.drain_garbage();
    self.state.take_audio_thread_budget_snapshot_and_reset()
  }

  #[napi]
  pub fn get_module_states(&self) -> HashMap<String, serde_json::Value> {
    self.state.get_module_states()
  }

  #[napi]
  pub fn get_transport_state(&self) -> TransportSnapshot {
    self.state.get_transport_state()
  }

  // =========================================================================
  // Audio Device Management
  // =========================================================================

  /// Refresh the device cache (re-enumerates all hosts and devices)
  #[napi]
  pub fn refresh_device_cache(&mut self) {
    self.device_cache.refresh();
  }

  /// Get the full device cache snapshot
  #[napi]
  pub fn get_device_cache(&self) -> DeviceCacheSnapshot {
    let hosts = self
      .device_cache
      .hosts
      .iter()
      .map(|h| HostDeviceInfo {
        host_id: h.id.clone(),
        host_name: h.name.clone(),
        output_devices: self.device_cache.output_devices_for_host(&h.id),
        input_devices: self.device_cache.input_devices_for_host(&h.id),
      })
      .collect();

    DeviceCacheSnapshot { hosts }
  }

  /// Get the current audio state (host, devices, sample rate, etc.)
  #[napi]
  pub fn get_current_audio_state(&self) -> CurrentAudioState {
    let output_device_name = self
      .output_device_id
      .as_ref()
      .and_then(|id| self.device_cache.find_output_device(id))
      .map(|d| d.name.clone());

    let input_device_name = self
      .input_device_id
      .as_ref()
      .and_then(|id| self.device_cache.find_input_device(id))
      .map(|d| d.name.clone());

    CurrentAudioState {
      host_id: self.host_id.clone(),
      output_device_id: self.output_device_id.clone(),
      output_device_name,
      input_device_id: self.input_device_id.clone(),
      input_device_name,
      sample_rate: self.sample_rate as u32,
      buffer_size: self.buffer_size,
      output_channels: self.channels,
      input_channels: self.input_channels,
      fallback_warning: self.fallback_warning.clone(),
    }
  }

  /// Recreate both input and output streams with new device/config
  /// This is the primary way to change audio devices after initialization
  #[napi]
  pub fn recreate_streams(
    &mut self,
    output_device_id: String,
    sample_rate: u32,
    buffer_size: Option<u32>,
    input_device_id: Option<String>,
  ) -> Result<()> {
    // Find the host for the output device
    let output_device_info = self
      .device_cache
      .find_output_device(&output_device_id)
      .ok_or_else(|| {
        napi::Error::from_reason(format!(
          "Output device '{}' not found in cache. Try refreshing the device cache.",
          output_device_id
        ))
      })?;

    let host_id = output_device_info.host_id.clone();
    let host = parse_host_id(&host_id)
      .and_then(|id| cpal::host_from_id(id).ok())
      .ok_or_else(|| napi::Error::from_reason(format!("Failed to get host '{}'", host_id)))?;

    // Find output device
    let output_device = find_output_device_in_host(&host, &output_device_id).ok_or_else(|| {
      napi::Error::from_reason(format!(
        "Output device '{}' not found in host '{}'",
        output_device_id, host_id
      ))
    })?;

    // Validate sample rate is supported
    if !output_device_info
      .supported_sample_rates
      .contains(&sample_rate)
    {
      return Err(napi::Error::from_reason(format!(
        "Sample rate {}Hz not supported by output device",
        sample_rate
      )));
    }

    // Get output config
    let output_config = output_device
      .default_output_config()
      .map_err(|e| napi::Error::from_reason(format!("Failed to get output config: {}", e)))?;

    // Prepare input device info if requested
    let input_setup = if let Some(ref input_id) = input_device_id {
      if input_id == "none" || input_id.is_empty() {
        None
      } else {
        // Validate input device exists
        let input_device_info = self
          .device_cache
          .find_input_device(input_id)
          .ok_or_else(|| {
            napi::Error::from_reason(format!("Input device '{}' not found in cache", input_id))
          })?;

        // Validate same host
        if input_device_info.host_id != host_id {
          return Err(napi::Error::from_reason(format!(
            "Input device '{}' is on host '{}' but output device is on host '{}'. Both must be on the same host.",
            input_id, input_device_info.host_id, host_id
          )));
        }

        // Validate sample rate
        if !input_device_info
          .supported_sample_rates
          .contains(&sample_rate)
        {
          return Err(napi::Error::from_reason(format!(
            "Sample rate {}Hz not supported by input device",
            sample_rate
          )));
        }

        let input_device = find_input_device_in_host(&host, input_id).ok_or_else(|| {
          napi::Error::from_reason(format!("Input device '{}' not found in host", input_id))
        })?;

        let input_config = input_device
          .default_input_config()
          .map_err(|e| napi::Error::from_reason(format!("Failed to get input config: {}", e)))?;

        Some((input_device, input_config, input_id.clone()))
      }
    } else {
      None
    };

    // Set up streams using shared helper (reuse existing midi_manager)
    let setup_result = setup_streams(StreamSetupParams {
      output_device: &output_device,
      output_config: &output_config,
      sample_rate,
      buffer_size,
      input: input_setup
        .as_ref()
        .map(|(d, c, id)| (d, c.clone(), id.clone())),
      midi_manager: self.midi_manager.clone(),
    })?;

    // Update self with new streams and state
    self._output_stream = setup_result.output_stream;
    self._input_stream = setup_result.input_stream;
    self.state = setup_result.state;
    self.sample_rate = setup_result.sample_rate;
    self.buffer_size = buffer_size;
    self.channels = setup_result.channels;
    self.input_channels = setup_result.input_channels;
    self.host_id = host_id.clone();
    self.output_device_id = Some(output_device_id.clone());
    self.input_device_id = setup_result.input_device_id;
    self.fallback_warning = None; // Clear any previous fallback warning

    println!(
      "Audio streams recreated: output={} input={:?} {}Hz {}ch host={}",
      output_device_id, self.input_device_id, sample_rate, setup_result.channels, host_id
    );

    Ok(())
  }

  // Legacy API compatibility methods (now use cache)

  /// Force refresh the device cache (legacy - same as refresh_device_cache)
  #[napi]
  pub fn refresh_device_list(&mut self) {
    self.refresh_device_cache();
  }

  /// List all available audio hosts (from cache)
  #[napi]
  pub fn list_audio_hosts(&self) -> Vec<HostInfo> {
    self.device_cache.hosts.clone()
  }

  /// List all available audio output devices (from cache)
  #[napi]
  pub fn list_audio_output_devices(&self) -> Vec<AudioDeviceInfo> {
    self.device_cache.all_output_devices()
  }

  /// List all available audio input devices (from cache)
  #[napi]
  pub fn list_audio_input_devices(&self) -> Vec<AudioDeviceInfo> {
    self.device_cache.all_input_devices()
  }

  /// Get the current output device ID
  #[napi]
  pub fn get_output_device_id(&self) -> Option<String> {
    self.output_device_id.clone()
  }

  /// Get the current input device ID
  #[napi]
  pub fn get_input_device_id(&self) -> Option<String> {
    self.input_device_id.clone()
  }

  /// Set the audio output device (legacy - use recreate_streams instead)
  /// This uses device default sample rate and buffer size
  #[napi]
  pub fn set_audio_output_device(&mut self, device_id: String) -> Result<()> {
    // Get device info to find its default sample rate
    let device_info = self
      .device_cache
      .find_output_device(&device_id)
      .ok_or_else(|| {
        napi::Error::from_reason(format!("Output device '{}' not found", device_id))
      })?;

    let sample_rate = device_info.sample_rate;

    self.recreate_streams(device_id, sample_rate, None, self.input_device_id.clone())
  }

  /// Set the audio input device (legacy - use recreate_streams instead)
  #[napi]
  pub fn set_audio_input_device(&mut self, device_id: Option<String>) -> Result<()> {
    let output_device_id = self
      .output_device_id
      .clone()
      .ok_or_else(|| napi::Error::from_reason("No output device configured"))?;

    self.recreate_streams(
      output_device_id,
      self.sample_rate as u32,
      self.buffer_size,
      device_id,
    )
  }

  // =========================================================================
  // MIDI Device Management
  // =========================================================================

  /// List all available MIDI input ports
  #[napi]
  pub fn list_midi_inputs(&self) -> Vec<MidiInputInfo> {
    MidiInputManager::list_ports()
      .into_iter()
      .map(|p| MidiInputInfo {
        name: p.name,
        index: p.index as u32,
      })
      .collect()
  }

  /// Get the currently connected MIDI input port name (first port for backward compatibility)
  #[napi]
  pub fn get_midi_input_name(&self) -> Option<String> {
    self.midi_manager.connected_port()
  }

  /// Get all currently connected MIDI input port names
  #[napi]
  pub fn get_midi_input_names(&self) -> Vec<String> {
    self.midi_manager.connected_ports()
  }

  /// Connect to a MIDI input port by name (for manual/backward-compatible use)
  #[napi]
  pub fn set_midi_input(&self, port_name: Option<String>) -> Result<()> {
    match port_name {
      None => {
        self.midi_manager.disconnect_all();
        println!("[MIDI] All inputs disconnected");
        Ok(())
      }
      Some(name) => {
        self
          .midi_manager
          .connect(&name)
          .map_err(napi::Error::from_reason)?;
        Ok(())
      }
    }
  }

  /// Attempt to reconnect to any MIDI devices that were configured but disconnected.
  /// Call this periodically if you want hot-plug support.
  /// Note: MIDI messages are polled automatically in the audio thread.
  #[napi]
  pub fn try_reconnect_midi(&self) {
    self.midi_manager.try_reconnect();
  }
}

#[napi]
pub fn get_schemas() -> Result<Vec<modular_core::types::ModuleSchema>> {
  Ok(schema())
}

/// Validate a PatchGraph against the module schemas.
///
/// Returns an array of validation errors (empty = valid).
/// This is a pure function: no audio hardware or Synthesizer instance needed.
#[napi]
pub fn validate_patch_graph(
  patch: modular_core::types::PatchGraph,
) -> Vec<crate::validation::ValidationError> {
  let schemas = schema();
  match crate::validation::validate_patch(&patch, &schemas) {
    Ok(()) => vec![],
    Err(errors) => errors,
  }
}

// Re-export for use in audio.rs
pub(crate) use params_cache::{deserialize_params};

#[napi(object)]
pub struct DeriveChannelCountResult {
  pub channel_count: Option<u32>,
  pub errors: Option<Vec<DeriveChannelCountError>>,
}

#[napi(object)]
pub struct DeriveChannelCountError {
  pub message: String,
  pub params: Vec<String>,
}

/// Parse a serde deserialization error to extract missing field names.
/// serde_json produces errors like: "missing field `input` at line 1 column 42"
fn parse_deserialization_error(error: &str) -> (String, Vec<String>) {
  let mut missing_fields = Vec::new();

  // serde_json only reports the FIRST missing field per deserialization attempt.
  // For now, report the single missing field from the error message.
  if let Some(start) = error.find("missing field `") {
    let after_prefix = &error[start + "missing field `".len()..];
    if let Some(end) = after_prefix.find('`') {
      missing_fields.push(after_prefix[..end].to_string());
    }
  }

  (error.to_string(), missing_fields)
}

/// Derive the output channel count for a module from its params JSON.
///
/// Returns a structured result with either the derived channel count or
/// error information when deserialization fails.
/// This uses the cache, so it also warms the cache for subsequent apply_patch calls.
#[napi]
pub fn derive_channel_count(
  module_type: String,
  params: serde_json::Value,
) -> DeriveChannelCountResult {
  match deserialize_params(&module_type, params, true) {
    Ok(d) => DeriveChannelCountResult {
      channel_count: Some(d.channel_count as u32),
      errors: None,
    },
    Err(e) => {
      let error_message = e.to_string();
      let (message, missing_params) = parse_deserialization_error(&error_message);
      DeriveChannelCountResult {
        channel_count: None,
        errors: Some(vec![DeriveChannelCountError {
          message,
          params: missing_params,
        }]),
      }
    }
  }
}

/// Parse a mini notation pattern and return all leaf spans.
///
/// This is used by the Monaco editor to create tracked decorations
/// that move with text edits.
#[napi]
pub fn get_mini_leaf_spans(source: String) -> Result<Vec<Vec<u32>>> {
  use modular_core::pattern_system::mini::{collect_leaf_spans, parse_ast};

  let ast = parse_ast(&source).map_err(|e| napi::Error::from_reason(e.to_string()))?;

  let spans = collect_leaf_spans(&ast);

  // Convert to Vec<Vec<u32>> for N-API (since tuples aren't directly supported)
  Ok(
    spans
      .into_iter()
      .map(|(start, end)| vec![start as u32, end as u32])
      .collect(),
  )
}

/// Analyze a mini notation pattern and return the maximum polyphony needed.
///
/// Queries 90 cycles (3 min at 120 BPM) and counts the maximum number of simultaneous haps,
/// capping at 16 (the poly voice limit). Logs timing for profiling.
#[napi]
pub fn get_pattern_polyphony(source: String) -> Result<u32> {
  use modular_core::dsp::seq::SeqValue;
  use modular_core::pattern_system::{Fraction, mini::parse};
  use std::cmp::Ordering;
  use std::time::Instant;

  let start = Instant::now();

  // Parse using SeqValue - handles notes, numbers, module references, etc.
  let pattern: modular_core::pattern_system::Pattern<SeqValue> =
    parse(&source).map_err(|e| napi::Error::from_reason(e.to_string()))?;

  let parse_time = start.elapsed();
  let query_start = Instant::now();

  const NUM_CYCLES: i64 = 90;
  const MAX_POLYPHONY: u32 = 16;

  // Query all cycles at once
  let haps = pattern.query_arc(
    Fraction::from_integer(0),
    Fraction::from_integer(NUM_CYCLES),
  );

  // Sweep line algorithm: create +1 events at start, -1 events at end
  // Event: (time, delta) where delta is +1 for start, -1 for end
  let mut events: Vec<(Fraction, i32)> = Vec::with_capacity(haps.len() * 2);

  for hap in &haps {
    if hap.value.is_rest() {
      continue;
    }
    events.push((hap.part.begin.clone(), 1)); // +1 at start
    events.push((hap.part.end.clone(), -1)); // -1 at end
  }

  // Sort by time, with ends (-1) before starts (+1) at same time
  // This ensures a note ending exactly when another starts doesn't count as overlap
  events.sort_by(|a, b| {
    match a.0.cmp(&b.0) {
      Ordering::Equal => a.1.cmp(&b.1), // -1 comes before +1
      other => other,
    }
  });

  // Sweep through events tracking current and max polyphony
  let mut current: u32 = 0;
  let mut max_simultaneous: u32 = 0;

  for (_time, delta) in events {
    if delta > 0 {
      current += 1;
      max_simultaneous = max_simultaneous.max(current);
      // Early exit if we hit the cap
      if max_simultaneous >= MAX_POLYPHONY {
        let query_time = query_start.elapsed();
        println!(
          "Pattern polyphony analysis: parse={:?}, query={:?}, max_poly={} (capped)",
          parse_time, query_time, MAX_POLYPHONY
        );
        return Ok(MAX_POLYPHONY);
      }
    } else {
      current = current.saturating_sub(1);
    }
  }

  let query_time = query_start.elapsed();
  println!(
    "Pattern polyphony analysis: parse={:?}, query={:?}, haps={}, max_poly={}",
    parse_time,
    query_time,
    haps.len(),
    max_simultaneous
  );

  Ok(max_simultaneous.max(1)) // At least 1 channel
}
