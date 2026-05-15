#![deny(clippy::all)]

mod audio;
mod commands;
mod graph_analysis;
mod link;
mod midi;
mod panic_log;
mod params_cache;
mod validation;
mod wav_bpm;
mod wav_metadata;

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

use std::path::{Path, PathBuf};
use std::time::SystemTime;

include!(concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/../reserved_output_names.rs"
));

/// Returns the list of reserved output names that cannot be used as module output port names.
/// These are names that conflict with built-in properties/methods on ModuleOutput, Collection, etc.
#[napi]
pub fn get_reserved_output_names() -> Vec<String> {
  RESERVED_OUTPUT_NAMES
    .iter()
    .map(|s| s.to_string())
    .collect()
}

// ============================================================================
// WAV Cache
// ============================================================================

/// Parse raw RIFF chunks to detect wavetable frame boundaries before hound decodes
/// the audio. Looks for vendor-specific chunks:
/// - `clm ` (Serum): ASCII payload starting with `<!>` then frame-size digits.
/// - `uhWT` (u-he Hive): presence implies frame size of 2048 (format undocumented).
/// - `srge` (Surge): `[i32 version][i32 frame_size]`, version must be 1.
///
/// Returns `None` for non-WAV files, missing/unknown metadata, or any I/O errors.
/// Never panics. Runs on the main thread.
fn detect_wavetable_frame_size(file_path: &Path) -> Option<usize> {
  use std::io::{Read, Seek, SeekFrom};

  let mut file = std::fs::File::open(file_path).ok()?;

  // RIFF header: "RIFF" <u32 size> "WAVE"
  let mut header = [0u8; 12];
  file.read_exact(&mut header).ok()?;
  if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
    return None;
  }

  // Walk chunks: 4-byte id + 4-byte LE size + payload (padded to even length).
  loop {
    let mut chunk_header = [0u8; 8];
    match file.read_exact(&mut chunk_header) {
      Ok(()) => {}
      Err(_) => return None, // EOF or truncated — no metadata found
    }

    let id = [
      chunk_header[0],
      chunk_header[1],
      chunk_header[2],
      chunk_header[3],
    ];
    let size = u32::from_le_bytes([
      chunk_header[4],
      chunk_header[5],
      chunk_header[6],
      chunk_header[7],
    ]) as u64;

    match &id {
      b"clm " => {
        // Serum: ASCII like "<!>2048 10000000 wavetable Foo"
        let read_len = size.min(256) as usize;
        let mut buf = vec![0u8; read_len];
        if file.read_exact(&mut buf).is_err() {
          return None;
        }
        // Skip remaining payload + padding to stay aligned in case we continue
        let remaining = size - read_len as u64;
        let padding = if size % 2 == 1 { 1 } else { 0 };
        if remaining + padding > 0
          && file
            .seek(SeekFrom::Current((remaining + padding) as i64))
            .is_err()
        {
          // best-effort: return what we found below
        }
        if let Some(stripped) = buf.strip_prefix(b"<!>") {
          let digits: Vec<u8> = stripped
            .iter()
            .copied()
            .take_while(|b| b.is_ascii_digit())
            .collect();
          // Only parse if we saw a non-digit terminator within the buffer;
          // otherwise the number may have been truncated by the 256-byte read cap.
          if !digits.is_empty() && digits.len() < stripped.len() {
            if let Ok(s) = std::str::from_utf8(&digits) {
              if let Ok(n) = s.parse::<usize>() {
                if n > 0 {
                  return Some(n);
                }
              }
            }
          }
        }
        // No parseable frame size — keep scanning
      }
      b"uhWT" => {
        // u-he Hive: format undocumented; presence implies 2048.
        return Some(2048);
      }
      b"srge" => {
        // Surge: [i32 version][i32 frame_size], version must be 1.
        if size >= 8 {
          let mut payload = [0u8; 8];
          if file.read_exact(&mut payload).is_err() {
            return None;
          }
          let version = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
          let frame_size = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
          // Skip rest of payload + padding
          let remaining = size - 8;
          let padding = if size % 2 == 1 { 1 } else { 0 };
          if remaining + padding > 0
            && file
              .seek(SeekFrom::Current((remaining + padding) as i64))
              .is_err()
          {
            return None;
          }
          if version == 1 && frame_size > 0 {
            return Some(frame_size as usize);
          }
          // Version mismatch — keep scanning (another chunk might match)
        } else {
          // Malformed srge — skip it
          let padding = if size % 2 == 1 { 1 } else { 0 };
          if file
            .seek(SeekFrom::Current((size + padding) as i64))
            .is_err()
          {
            return None;
          }
        }
      }
      _ => {
        // Skip unknown chunk payload + padding byte if size is odd.
        let padding = if size % 2 == 1 { 1 } else { 0 };
        if file
          .seek(SeekFrom::Current((size + padding) as i64))
          .is_err()
        {
          return None;
        }
      }
    }
  }
}

struct WavCacheEntry {
  data: Arc<modular_core::types::WavData>,
  mtime: SystemTime,
  metadata: wav_metadata::WavMetadata,
}

struct WavCache {
  entries: HashMap<String, WavCacheEntry>,
  workspace_path: PathBuf,
}

fn build_wav_load_info(
  rel_path: &str,
  num_channels: u32,
  frame_count: u32,
  meta: &wav_metadata::WavMetadata,
  mtime: f64,
) -> WavLoadInfo {
  WavLoadInfo {
    channels: num_channels,
    frame_count,
    path: rel_path.to_string(),
    sample_rate: meta.sample_rate,
    duration: meta.frame_count as f64 / meta.sample_rate as f64,
    bit_depth: meta.bit_depth as u32,
    pitch: meta.pitch,
    playback: meta.playback.as_ref().map(|p| match p {
      wav_metadata::PlaybackMode::OneShot => "one-shot".to_string(),
      wav_metadata::PlaybackMode::Loop => "loop".to_string(),
    }),
    bpm: meta.bpm,
    beats: meta.beats,
    time_signature: meta.time_signature.map(|(num, den)| WavTimeSignature {
      num: num as u32,
      den: den as u32,
    }),
    bar_count: meta.bar_count,
    loops: meta
      .loops
      .iter()
      .map(|l| WavLoopInfo {
        loop_type: match l.loop_type {
          wav_metadata::LoopType::Forward => "forward".to_string(),
          wav_metadata::LoopType::PingPong => "pingpong".to_string(),
          wav_metadata::LoopType::Backward => "backward".to_string(),
        },
        start: l.start_seconds,
        end: l.end_seconds,
      })
      .collect(),
    cue_points: meta
      .cue_points
      .iter()
      .map(|c| WavCuePointInfo {
        position: c.position_seconds,
        label: c.label.clone(),
      })
      .collect(),
    mtime,
  }
}

/// Convert a `SystemTime` to epoch milliseconds as f64. Returns 0.0 if the time
/// predates the UNIX epoch or the conversion fails — this is a cache-key hint,
/// not a correctness constraint.
fn mtime_to_epoch_millis(mtime: SystemTime) -> f64 {
  mtime
    .duration_since(SystemTime::UNIX_EPOCH)
    .ok()
    .map(|d| d.as_millis() as f64)
    .unwrap_or(0.0)
}

impl WavCache {
  fn new(workspace_path: PathBuf) -> Self {
    Self {
      entries: HashMap::new(),
      workspace_path,
    }
  }

  fn set_workspace_path(&mut self, path: PathBuf) {
    if self.workspace_path != path {
      self.entries.clear();
      self.workspace_path = path;
    }
  }

  /// Snapshot all cached WAV data as Arc clones (cheap) for PatchUpdate.
  fn snapshot(&self) -> HashMap<String, Arc<modular_core::types::WavData>> {
    self
      .entries
      .iter()
      .map(|(k, v)| (k.clone(), Arc::clone(&v.data)))
      .collect()
  }

  /// Load a WAV file, returning cached data if mtime hasn't changed.
  fn load(&mut self, rel_path: &str, engine_sample_rate: f32) -> Result<WavLoadInfo> {
    let full_path = self
      .workspace_path
      .join("wavs")
      .join(format!("{}.wav", rel_path));

    let metadata = std::fs::metadata(&full_path).map_err(|e| {
      napi::Error::from_reason(format!(
        "WAV file not found: {} ({})",
        full_path.display(),
        e
      ))
    })?;
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

    // Cache hit — mtime matches
    if let Some(entry) = self.entries.get(rel_path) {
      if entry.mtime == mtime {
        return Ok(build_wav_load_info(
          rel_path,
          entry.data.channel_count() as u32,
          entry.data.frame_count() as u32,
          &entry.metadata,
          mtime_to_epoch_millis(mtime),
        ));
      }
    }

    // Cache miss or stale — decode
    let reader = hound::WavReader::open(&full_path).map_err(|e| {
      napi::Error::from_reason(format!(
        "Failed to read WAV file {}: {}",
        full_path.display(),
        e
      ))
    })?;

    let spec = reader.spec();
    let file_sample_rate = spec.sample_rate as f32;
    let num_channels = spec.channels as usize;

    // Decode all samples into interleaved f32
    let raw_samples: Vec<f32> = match spec.sample_format {
      hound::SampleFormat::Int => {
        let bits = spec.bits_per_sample;
        let max_val = (1u32 << (bits - 1)) as f32;
        reader
          .into_samples::<i32>()
          .map(|s| s.unwrap_or(0) as f32 / max_val)
          .collect()
      }
      hound::SampleFormat::Float => reader
        .into_samples::<f32>()
        .map(|s| s.unwrap_or(0.0))
        .collect(),
    };

    // Deinterleave into per-channel vectors
    let total_frames = raw_samples.len() / num_channels.max(1);
    let mut channels: Vec<Vec<f32>> = vec![Vec::with_capacity(total_frames); num_channels];
    for (i, sample) in raw_samples.iter().enumerate() {
      let ch = i % num_channels;
      channels[ch].push(*sample);
    }

    let frame_count = channels.first().map_or(0, Vec::len);

    // Build a mono mixdown for BPM analysis (cheap — runs once, only on cache miss).
    let mono_for_analysis: Vec<f32> = if num_channels <= 1 {
      channels.first().cloned().unwrap_or_default()
    } else {
      let scale = 1.0 / num_channels as f32;
      (0..frame_count)
        .map(|i| channels.iter().map(|c| c[i]).sum::<f32>() * scale)
        .collect()
    };

    // Detect wavetable frame size from vendor-specific RIFF chunks (pre-decode probe).
    let detected_frame_size = detect_wavetable_frame_size(&full_path);

    let wav_data = Arc::new(modular_core::types::WavData::new(
      modular_core::types::SampleBuffer::from_samples(channels, file_sample_rate),
      detected_frame_size,
    ));

    // Extract RIFF metadata (re-open file for a clean read)
    let mut riff_file = std::fs::File::open(&full_path).map_err(|e| {
      napi::Error::from_reason(format!(
        "Failed to open WAV for metadata: {} ({})",
        full_path.display(),
        e
      ))
    })?;
    let metadata = wav_metadata::extract(
      &mut riff_file,
      total_frames as u64,
      Some(&full_path),
      Some(&mono_for_analysis),
    )
    .map_err(|e| {
      napi::Error::from_reason(format!(
        "Failed to extract WAV metadata from {}: {}",
        full_path.display(),
        e
      ))
    })?;

    let info = build_wav_load_info(
      rel_path,
      num_channels as u32,
      frame_count as u32,
      &metadata,
      mtime_to_epoch_millis(mtime),
    );

    self.entries.insert(
      rel_path.to_string(),
      WavCacheEntry {
        data: wav_data,
        mtime,
        metadata,
      },
    );

    Ok(info)
  }
}

#[napi(object)]
pub struct WavLoopInfo {
  pub loop_type: String,
  pub start: f64,
  pub end: f64,
}

#[napi(object)]
pub struct WavCuePointInfo {
  pub position: f64,
  pub label: String,
}

#[napi(object)]
pub struct WavTimeSignature {
  pub num: u32,
  pub den: u32,
}

#[napi(object)]
pub struct WavLoadInfo {
  pub channels: u32,
  pub frame_count: u32,
  pub path: String,
  pub sample_rate: u32,
  pub duration: f64,
  pub bit_depth: u32,
  pub pitch: Option<f64>,
  pub playback: Option<String>,
  pub bpm: Option<f64>,
  pub beats: Option<u32>,
  pub time_signature: Option<WavTimeSignature>,
  /// Number of bars the sample spans, computed from BPM and time signature.
  /// E.g. an exact 2-bar loop is `2.0`; a 2.64-bar buffer is `2.64`. `None`
  /// when no BPM could be derived from any source.
  pub bar_count: Option<f64>,
  pub loops: Vec<WavLoopInfo>,
  pub cue_points: Vec<WavCuePointInfo>,
  /// File modification time, epoch milliseconds. Used as a cache-key hint so the
  /// DSL executor can invalidate params caches when the underlying WAV changes
  /// on disk. Falls back to 0.0 when the mtime can't be read.
  pub mtime: f64,
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
  wav_cache: WavCache,
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
    panic_log::install_panic_hook();
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
      wav_cache: WavCache::new(PathBuf::new()),
    })
  }

  #[napi]
  pub fn stop(&mut self) {
    self.state.request_stop();
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

  /// True if the audio callback caught a panic and is now emitting silence.
  /// Recovery requires destroying this Synthesizer and creating a new one,
  /// or calling `restart_audio()`. The panic payload + backtrace is written
  /// to the path returned by `panic_log_dir()`.
  #[napi]
  pub fn is_audio_thread_panicked(&self) -> bool {
    self.state.is_audio_thread_panicked()
  }

  /// Absolute path to the directory where panic logs are written. Matches
  /// Electron's `app.getPath('logs')` convention per OS:
  /// - macOS:   `~/Library/Logs/Operator/`
  /// - Linux:   `~/.config/Operator/logs/`
  /// - Windows: `%APPDATA%\Operator\logs\`
  #[napi]
  pub fn panic_log_dir(&self) -> String {
    panic_log::panic_log_dir()
      .to_string_lossy()
      .into_owned()
  }

  #[napi]
  pub fn get_scopes(&self) -> Vec<(ScopeBufferKey, Float32Array, ScopeStats)> {
    self.state.get_audio_buffers()
  }

  #[napi]
  pub fn update_patch(
    &mut self,
    mut patch: PatchGraph,
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
    // Also extract and strip `tempoSet` — it's a DSL-only flag, not a real Clock param
    let tempo_override =
      if let Some(root_clock) = patch.modules.iter_mut().find(|m| m.id == *ROOT_CLOCK_ID) {
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

        // Check if DSL explicitly called $setTempo, then strip the flag
        // so Rust serde doesn't reject the unknown field on ClockParams
        let tempo_set = root_clock
          .params
          .as_object_mut()
          .and_then(|obj| obj.remove("tempoSet"))
          .and_then(|v| v.as_bool())
          .unwrap_or(false);
        if tempo_set { Some(bpm) } else { None }
      } else {
        None
      };

    let wav_data_snapshot = self.wav_cache.snapshot();
    let errors = self.state.handle_set_patch(
      patch,
      self.sample_rate,
      trigger,
      update_id,
      wav_data_snapshot,
      tempo_override,
    );

    PatchUpdateResult {
      errors,
      update_id: update_id as f64,
    }
  }

  /// Load a WAV file into the cache, returning metadata about the loaded sample.
  #[napi]
  pub fn load_wav(&mut self, path: String) -> Result<WavLoadInfo> {
    self.wav_cache.load(&path, self.sample_rate)
  }

  /// Set the workspace root directory for WAV file loading.
  #[napi]
  pub fn set_wav_workspace(&mut self, workspace_path: String) {
    self
      .wav_cache
      .set_workspace_path(PathBuf::from(workspace_path));
  }

  /// Get the list of currently cached WAV file paths.
  #[napi]
  pub fn get_wav_cache_snapshot(&self) -> Vec<String> {
    self.wav_cache.entries.keys().cloned().collect()
  }

  /// Lightweight single-module param update. Constructs a new module on the main
  /// thread and sends it to the audio thread for state-transfer + replacement.
  #[napi]
  pub fn set_module_param(
    &self,
    module_id: String,
    module_type: String,
    params: serde_json::Value,
  ) -> Result<()> {
    // Deserialize on main thread (no cache — slider values would pollute it)
    let deserialized = deserialize_params(&module_type, params, false).map_err(|e| {
      napi::Error::from_reason(format!(
        "Failed to deserialize params for {}: {}",
        module_type, e
      ))
    })?;

    // Construct the module on the main thread
    let constructors = modular_core::dsp::get_constructors();
    let constructor = constructors
      .get(&module_type)
      .ok_or_else(|| napi::Error::from_reason(format!("Unknown module type: {}", module_type)))?;
    let module = constructor(
      &module_id,
      self.sample_rate,
      deserialized,
      1,
      modular_core::types::ProcessingMode::Block,
    )
    .map_err(|e| {
      napi::Error::from_reason(format!("Failed to create module {}: {}", module_id, e))
    })?;

    self
      .state
      .send_command(GraphCommand::SingleModuleUpdate { module_id, module })
  }

  /// Extract MIDI device names from patch modules and sync connections
  fn sync_midi_devices_from_patch(&self, patch: &PatchGraph) {
    use std::collections::HashSet;

    let mut devices: HashSet<String> = HashSet::new();

    for module in &patch.modules {
      // Check if this is a MIDI module type
      match module.module_type.as_str() {
        "$midiCV" | "$midiCC" => {
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
    self.state.take_audio_thread_budget_snapshot_and_reset()
  }

  /// Drain deferred deallocations from the audio thread. The RT audio thread
  /// cannot free memory itself, so it pushes old resources onto a lock-free
  /// garbage queue. Call this periodically from the main thread to drop them.
  #[napi]
  pub fn drain_garbage(&self) {
    self.state.drain_garbage();
  }

  #[napi]
  pub fn get_module_states(&self) -> HashMap<String, serde_json::Value> {
    self.state.get_module_states()
  }

  #[napi]
  pub fn get_transport_state(&self) -> TransportSnapshot {
    self.state.get_transport_state()
  }

  #[napi]
  pub fn enable_link(&self, enabled: bool) -> Result<()> {
    // Idempotency: if Link is already in the requested state, do nothing.
    // This both avoids constructing a redundant `AblLink` (heap allocation,
    // socket open, networking thread spawn — all expensive) and prevents
    // tearing down an existing live session whose peers would then have to
    // re-discover us.
    if self.state.transport_meter.read_link_enabled() == enabled {
      return Ok(());
    }

    let resources = if enabled {
      // Construct + enable on the main thread. Per Ableton's documentation
      // (`Link.hpp`: "Realtime-safe: no" on `enable()`), construction and
      // enable cannot run on the audio thread without risking glitches.
      // Initialise at the user's last known tempo so toggling Link doesn't
      // reset the session BPM.
      let bpm = self.state.transport_meter.read_bpm();
      let link = rusty_link::AblLink::new(bpm);
      link.enable(true);
      link.enable_start_stop_sync(true);
      Some(Box::new(crate::link::LinkResources {
        link,
        host_time_filter: rusty_link::HostTimeFilter::new(),
        session_state: rusty_link::SessionState::new(),
      }))
    } else {
      None
    };

    self.state.send_command(GraphCommand::SetLink(resources))?;

    // Update the meter immediately so the UI flips state without waiting
    // for the next audio callback. Peer count starts at 0 in both cases:
    // on disable there are no peers; on a fresh enable the live peer count
    // is 0 until the audio thread overwrites it from `link.num_peers()`.
    self.state.transport_meter.write_link_state(enabled, 0);
    Ok(())
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

  /// Restart the audio streams using the current device/sample-rate/buffer-size
  /// settings. `recreate_streams` replaces `self.state` with a fresh
  /// `AudioState`, so the new stream's callback captures a fresh
  /// `audio_thread_panicked` flag (initially false). The old (poisoned) flag
  /// stays set on the old `AudioState` so the old callback continues to emit
  /// silence until cpal drops the old stream. Call this after
  /// `is_audio_thread_panicked()` returns true to recover.
  #[napi]
  pub fn restart_audio(&mut self) -> Result<()> {
    let output_device_id = self
      .output_device_id
      .clone()
      .ok_or_else(|| napi::Error::from_reason("No output device configured"))?;
    self.recreate_streams(
      output_device_id,
      self.sample_rate as u32,
      self.buffer_size,
      self.input_device_id.clone(),
    )
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
pub(crate) use params_cache::deserialize_params;

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
      let param_errors = e.into_errors();
      DeriveChannelCountResult {
        channel_count: None,
        errors: Some(
          param_errors
            .into_iter()
            .map(|err| DeriveChannelCountError {
              message: err.message,
              params: if err.field.is_empty() {
                vec![]
              } else {
                vec![err.field]
              },
            })
            .collect(),
        ),
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod detect_wavetable_tests {
  use super::detect_wavetable_frame_size;
  use std::io::Write;
  use std::path::PathBuf;

  fn write_u32_le(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_le_bytes());
  }

  fn write_i32_le(buf: &mut Vec<u8>, val: i32) {
    buf.extend_from_slice(&val.to_le_bytes());
  }

  /// Build a minimal WAV-like RIFF header. Chunks are appended by callers.
  fn riff_header() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"RIFF");
    write_u32_le(&mut buf, 0); // placeholder size (detector doesn't use it)
    buf.extend_from_slice(b"WAVE");
    buf
  }

  /// Append a chunk with the given 4-byte id and payload. Pads to even length.
  fn append_chunk(buf: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    buf.extend_from_slice(id);
    write_u32_le(buf, payload.len() as u32);
    buf.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
      buf.push(0);
    }
  }

  /// Fill in the RIFF size field with the actual byte count.
  fn fix_riff_size(buf: &mut [u8]) {
    let riff_size = (buf.len() - 8) as u32;
    buf[4..8].copy_from_slice(&riff_size.to_le_bytes());
  }

  /// Write bytes to a unique temp file and return the path (test helper).
  fn write_temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_nanos())
      .unwrap_or(0);
    path.push(format!(
      "modular_detect_wavetable_{}_{}_{}.wav",
      name,
      std::process::id(),
      nanos
    ));
    let mut f = std::fs::File::create(&path).expect("create temp file");
    f.write_all(bytes).expect("write temp file");
    f.flush().expect("flush temp file");
    path
  }

  /// Append a minimal fmt chunk so a hypothetical decoder could still open the file.
  fn append_fmt(buf: &mut Vec<u8>) {
    let mut payload = Vec::new();
    // PCM format, 1 channel, 44100 Hz, byte_rate, block_align, 16 bits
    payload.extend_from_slice(&1u16.to_le_bytes()); // audio format
    payload.extend_from_slice(&1u16.to_le_bytes()); // channels
    payload.extend_from_slice(&44100u32.to_le_bytes()); // sample rate
    payload.extend_from_slice(&(44100u32 * 2).to_le_bytes()); // byte rate
    payload.extend_from_slice(&2u16.to_le_bytes()); // block align
    payload.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    append_chunk(buf, b"fmt ", &payload);
  }

  #[test]
  fn clm_chunk_detected() {
    let mut buf = riff_header();
    append_fmt(&mut buf);
    append_chunk(&mut buf, b"clm ", b"<!>2048 10000000 wavetable Test");
    fix_riff_size(&mut buf);

    let path = write_temp_file("clm", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, Some(2048));
  }

  #[test]
  fn clm_chunk_odd_size_padded() {
    // Payload length is odd (31 bytes) to exercise the padding branch.
    let payload = b"<!>512 foo bar baz wavetable!!"; // 30 bytes
    assert_eq!(payload.len(), 30);
    let mut odd_payload = payload.to_vec();
    odd_payload.push(b'x'); // now 31 bytes
    let mut buf = riff_header();
    append_chunk(&mut buf, b"clm ", &odd_payload);
    fix_riff_size(&mut buf);

    let path = write_temp_file("clm_odd", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, Some(512));
  }

  #[test]
  fn uhwt_chunk_detected() {
    let mut buf = riff_header();
    append_fmt(&mut buf);
    append_chunk(&mut buf, b"uhWT", &[0xDE, 0xAD, 0xBE, 0xEF]);
    fix_riff_size(&mut buf);

    let path = write_temp_file("uhwt", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, Some(2048));
  }

  #[test]
  fn srge_chunk_version1_detected() {
    let mut payload = Vec::new();
    write_i32_le(&mut payload, 1); // version
    write_i32_le(&mut payload, 1024); // frame_size
    let mut buf = riff_header();
    append_fmt(&mut buf);
    append_chunk(&mut buf, b"srge", &payload);
    fix_riff_size(&mut buf);

    let path = write_temp_file("srge_v1", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, Some(1024));
  }

  #[test]
  fn srge_chunk_version2_keeps_scanning_and_returns_none_if_no_other_match() {
    let mut payload = Vec::new();
    write_i32_le(&mut payload, 2); // version != 1 → skip
    write_i32_le(&mut payload, 1024);
    let mut buf = riff_header();
    append_fmt(&mut buf);
    append_chunk(&mut buf, b"srge", &payload);
    fix_riff_size(&mut buf);

    let path = write_temp_file("srge_v2", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, None);
  }

  #[test]
  fn srge_v2_followed_by_clm_keeps_scanning_and_finds_clm() {
    let mut payload = Vec::new();
    write_i32_le(&mut payload, 2); // version != 1 → skip
    write_i32_le(&mut payload, 999);
    let mut buf = riff_header();
    append_fmt(&mut buf);
    append_chunk(&mut buf, b"srge", &payload);
    append_chunk(&mut buf, b"clm ", b"<!>4096 extra stuff");
    fix_riff_size(&mut buf);

    let path = write_temp_file("srge_then_clm", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, Some(4096));
  }

  #[test]
  fn no_known_chunks_returns_none() {
    let mut buf = riff_header();
    append_fmt(&mut buf);
    append_chunk(&mut buf, b"data", &[0u8; 16]);
    fix_riff_size(&mut buf);

    let path = write_temp_file("no_match", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, None);
  }

  #[test]
  fn non_wav_file_returns_none() {
    let path = write_temp_file("not_wav", b"This is not a WAV file at all.");
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, None);
  }

  #[test]
  fn empty_file_returns_none() {
    let path = write_temp_file("empty", &[]);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, None);
  }

  #[test]
  fn missing_file_returns_none() {
    let mut path = std::env::temp_dir();
    path.push("modular_detect_wavetable_definitely_does_not_exist.wav");
    assert_eq!(detect_wavetable_frame_size(&path), None);
  }

  #[test]
  fn truncated_chunk_header_returns_none() {
    // RIFF header + partial chunk header (only 4 bytes instead of 8) → EOF mid-walk.
    let mut buf = riff_header();
    buf.extend_from_slice(b"clm ");
    fix_riff_size(&mut buf);

    let path = write_temp_file("truncated", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, None);
  }

  #[test]
  fn clm_without_prefix_returns_none() {
    // Payload doesn't start with "<!>" → no parseable frame size, keep scanning,
    // no other known chunks → None.
    let mut buf = riff_header();
    append_chunk(&mut buf, b"clm ", b"no prefix here");
    fix_riff_size(&mut buf);

    let path = write_temp_file("clm_no_prefix", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, None);
  }

  #[test]
  fn clm_zero_frame_size_keeps_scanning() {
    // A `clm ` payload with frame_size 0 should be rejected and scanning continue.
    let mut buf = riff_header();
    append_fmt(&mut buf);
    append_chunk(&mut buf, b"clm ", b"<!>0 10000000 wavetable Zero");
    fix_riff_size(&mut buf);

    let path = write_temp_file("clm_zero", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, None);

    // Same zero-valued `clm ` followed by a valid srge v1 → scanning should continue
    // and return the srge frame size.
    let mut srge_payload = Vec::new();
    write_i32_le(&mut srge_payload, 1);
    write_i32_le(&mut srge_payload, 1024);
    let mut buf = riff_header();
    append_fmt(&mut buf);
    append_chunk(&mut buf, b"clm ", b"<!>0 10000000 wavetable Zero");
    append_chunk(&mut buf, b"srge", &srge_payload);
    fix_riff_size(&mut buf);

    let path = write_temp_file("clm_zero_then_srge", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, Some(1024));
  }

  #[test]
  fn clm_truncated_number_keeps_scanning() {
    // The detector reads at most 256 bytes of the `clm ` payload. If `<!>` is
    // followed by a digit run that fills the entire 256-byte read (no non-digit
    // terminator seen), the number may be truncated — the detector must not return
    // a potentially-wrong value and must keep scanning.

    // Build a payload: "<!>" + 300 ASCII digits. After stripping "<!>", the
    // first 253 bytes of the 256-byte read window are all digits, so digits.len()
    // == stripped.len() → possibly-truncated → fall through.
    let mut long_digits_payload = Vec::new();
    long_digits_payload.extend_from_slice(b"<!>");
    long_digits_payload.extend(std::iter::repeat(b'1').take(300));

    let mut buf = riff_header();
    append_fmt(&mut buf);
    append_chunk(&mut buf, b"clm ", &long_digits_payload);
    fix_riff_size(&mut buf);

    let path = write_temp_file("clm_truncated_num", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, None);

    // Same truncated `clm ` followed by a valid srge v1 frame_size=512 → must
    // keep scanning past the truncated clm and return 512.
    let mut srge_payload = Vec::new();
    write_i32_le(&mut srge_payload, 1);
    write_i32_le(&mut srge_payload, 512);
    let mut buf = riff_header();
    append_fmt(&mut buf);
    append_chunk(&mut buf, b"clm ", &long_digits_payload);
    append_chunk(&mut buf, b"srge", &srge_payload);
    fix_riff_size(&mut buf);

    let path = write_temp_file("clm_truncated_num_then_srge", &buf);
    let result = detect_wavetable_frame_size(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(result, Some(512));
  }
}
