use cpal::FromSample;
use cpal::Host;
use cpal::HostId;
use cpal::Sample;
use cpal::SizedSample;
use cpal::traits::{DeviceTrait, HostTrait};

use hound::{WavSpec, WavWriter};
use modular_core::PORT_MAX_CHANNELS;
use modular_core::PatchGraph;
use modular_core::dsp::get_constructors;
use modular_core::dsp::schema;
use modular_core::dsp::utils::SchmittTrigger;

use modular_core::types::ClockMessages;
use modular_core::types::Message;
use modular_core::types::ScopeMode;
use modular_core::types::WellKnownModule;
use napi::Result;
use napi::bindgen_prelude::Float32Array;
use napi_derive::napi;
use parking_lot::Mutex;
use profiling;
use ringbuf::{
  HeapRb,
  traits::{Consumer, Producer, Split},
};
use rtrb::{Consumer as RtrbConsumer, Producer as RtrbProducer, RingBuffer};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};

use modular_core::patch::Patch;
use modular_core::types::{ROOT_OUTPUT_PORT, ScopeBufferKey};
use std::time::Instant;

use crate::commands::{
  AudioError, COMMAND_QUEUE_CAPACITY, ERROR_QUEUE_CAPACITY, GARBAGE_QUEUE_CAPACITY, GarbageItem,
  GraphCommand, PatchUpdate, QueuedTrigger,
};
use crate::midi::MidiInputManager;

// ============================================================================
// Audio Host Information
// ============================================================================

/// Information about an audio host
#[derive(Debug, Clone)]
#[napi(object)]
pub struct HostInfo {
  /// Host identifier (e.g., "CoreAudio", "WASAPI", "ALSA")
  pub id: String,
  /// Human-readable host name
  pub name: String,
}

// ============================================================================
// Audio Device Information
// ============================================================================

/// Buffer size range for an audio device
#[derive(Debug, Clone)]
#[napi(object)]
pub struct BufferSizeRange {
  pub min: u32,
  pub max: u32,
}

/// Information about an audio device
#[derive(Debug, Clone)]
#[napi(object)]
pub struct AudioDeviceInfo {
  /// Stable Device ID
  pub id: String,
  /// Host ID this device belongs to
  pub host_id: String,
  /// Device name
  pub name: String,
  /// Number of input channels (0 if output-only)
  pub input_channels: u16,
  /// Number of output channels (0 if input-only)
  pub output_channels: u16,
  /// Whether this is the default device for this host
  pub is_default: bool,
  /// Default sample rate in Hz
  pub sample_rate: u32,
  /// Supported sample rates (common rates that the device supports)
  pub supported_sample_rates: Vec<u32>,
  /// Buffer size range (min/max), or None if unknown
  pub buffer_size_range: Option<BufferSizeRange>,
}

/// Common sample rates to check for support
const COMMON_SAMPLE_RATES: &[u32] = &[44100, 48000, 88200, 96000, 176400, 192000];

/// Maximum sample rate to use as a default for new users / missing config.
/// If the OS/device reports a default above this, we pick the highest
/// supported rate at or below this cap instead.
const PREFERRED_MAX_DEFAULT_SAMPLE_RATE: u32 = 48_000;

/// Choose a sensible default sample rate for the given device.
///
/// Uses the device's cpal-reported default (`device_default`) when it is
/// at or below `PREFERRED_MAX_DEFAULT_SAMPLE_RATE`.  When the device default
/// is higher (common on macOS when Audio MIDI Setup is set to 96 kHz+),
/// we pick the highest rate from `supported_rates` that is still ≤ the cap.
/// If no supported rate is ≤ the cap (very unlikely), we fall back to the
/// device default so audio still works.
pub fn preferred_default_sample_rate(device_default: u32, supported_rates: &[u32]) -> u32 {
  if device_default <= PREFERRED_MAX_DEFAULT_SAMPLE_RATE {
    return device_default;
  }

  // Device default is too high — pick the best rate at or below the cap.
  supported_rates
    .iter()
    .copied()
    .filter(|&r| r <= PREFERRED_MAX_DEFAULT_SAMPLE_RATE)
    .max()
    .unwrap_or(device_default)
}

// ============================================================================
// Device Cache
// ============================================================================

/// Cached information about a device (includes cpal Device handle)
#[derive(Clone)]
pub struct CachedDevice {
  pub info: AudioDeviceInfo,
  // Note: cpal::Device doesn't implement Clone, so we store just the info
  // and look up the device by ID when needed
}

/// Cache of all available audio hosts and devices
#[derive(Default)]
pub struct AudioDeviceCache {
  /// All available hosts
  pub hosts: Vec<HostInfo>,
  /// Output devices keyed by host_id
  pub output_devices: HashMap<String, Vec<AudioDeviceInfo>>,
  /// Input devices keyed by host_id
  pub input_devices: HashMap<String, Vec<AudioDeviceInfo>>,
}

impl AudioDeviceCache {
  pub fn new() -> Self {
    let mut cache = Self::default();
    cache.refresh();
    cache
  }

  /// Refresh the cache by enumerating all hosts and their devices
  pub fn refresh(&mut self) {
    self.hosts.clear();
    self.output_devices.clear();
    self.input_devices.clear();

    for host_id in cpal::available_hosts() {
      let host_id_str = format!("{:?}", host_id);

      self.hosts.push(HostInfo {
        id: host_id_str.clone(),
        name: host_id_str.clone(),
      });

      if let Ok(host) = cpal::host_from_id(host_id) {
        // Get output devices for this host
        let output_devices = enumerate_output_devices(&host, &host_id_str);
        self
          .output_devices
          .insert(host_id_str.clone(), output_devices);

        // Get input devices for this host
        let input_devices = enumerate_input_devices(&host, &host_id_str);
        self.input_devices.insert(host_id_str, input_devices);
      }
    }
  }

  /// Get all output devices across all hosts
  pub fn all_output_devices(&self) -> Vec<AudioDeviceInfo> {
    self.output_devices.values().flatten().cloned().collect()
  }

  /// Get all input devices across all hosts
  pub fn all_input_devices(&self) -> Vec<AudioDeviceInfo> {
    self.input_devices.values().flatten().cloned().collect()
  }

  /// Find an output device by ID
  pub fn find_output_device(&self, device_id: &str) -> Option<&AudioDeviceInfo> {
    self
      .output_devices
      .values()
      .flatten()
      .find(|d| d.id == device_id)
  }

  /// Find an input device by ID
  pub fn find_input_device(&self, device_id: &str) -> Option<&AudioDeviceInfo> {
    self
      .input_devices
      .values()
      .flatten()
      .find(|d| d.id == device_id)
  }

  /// Get output devices for a specific host
  pub fn output_devices_for_host(&self, host_id: &str) -> Vec<AudioDeviceInfo> {
    self
      .output_devices
      .get(host_id)
      .cloned()
      .unwrap_or_default()
  }

  /// Get input devices for a specific host
  pub fn input_devices_for_host(&self, host_id: &str) -> Vec<AudioDeviceInfo> {
    self.input_devices.get(host_id).cloned().unwrap_or_default()
  }

  /// Get all host IDs
  pub fn host_ids(&self) -> Vec<String> {
    self.hosts.iter().map(|h| h.id.clone()).collect()
  }
}

/// Per-host device info for the cache snapshot
#[derive(Debug, Clone)]
#[napi(object)]
pub struct HostDeviceInfo {
  pub host_id: String,
  pub host_name: String,
  pub output_devices: Vec<AudioDeviceInfo>,
  pub input_devices: Vec<AudioDeviceInfo>,
}

/// N-API compatible structure for the full device cache
#[derive(Debug, Clone)]
#[napi(object)]
pub struct DeviceCacheSnapshot {
  /// All hosts with their devices grouped together
  pub hosts: Vec<HostDeviceInfo>,
}

/// Current audio state information
#[derive(Debug, Clone)]
#[napi(object)]
pub struct CurrentAudioState {
  pub host_id: String,
  pub output_device_id: Option<String>,
  pub output_device_name: Option<String>,
  pub input_device_id: Option<String>,
  pub input_device_name: Option<String>,
  pub sample_rate: u32,
  pub buffer_size: Option<u32>,
  pub output_channels: u16,
  pub input_channels: u16,
  pub fallback_warning: Option<String>,
}

/// Extract supported sample rates and buffer size range from device configs
fn get_device_capabilities(
  configs: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
) -> (Vec<u32>, Option<BufferSizeRange>) {
  let mut supported_rates = std::collections::HashSet::new();
  let mut min_buffer = u32::MAX;
  let mut max_buffer = 0u32;

  for config in configs {
    // Check which common sample rates are supported
    let min_rate = config.min_sample_rate();
    let max_rate = config.max_sample_rate();
    for &rate in COMMON_SAMPLE_RATES {
      if rate >= min_rate && rate <= max_rate {
        supported_rates.insert(rate);
      }
    }

    // Extract buffer size range
    match config.buffer_size() {
      cpal::SupportedBufferSize::Range { min, max } => {
        min_buffer = min_buffer.min(*min);
        max_buffer = max_buffer.max(*max);
      }
      cpal::SupportedBufferSize::Unknown => {}
    }
  }

  let mut rates: Vec<u32> = supported_rates.into_iter().collect();
  rates.sort();

  let buffer_range = if min_buffer <= max_buffer && max_buffer > 0 {
    Some(BufferSizeRange {
      min: min_buffer,
      max: max_buffer,
    })
  } else {
    None
  };

  (rates, buffer_range)
}

/// Enumerate output devices for a specific host
fn enumerate_output_devices(host: &Host, host_id: &str) -> Vec<AudioDeviceInfo> {
  let default_device_id = host.default_output_device().and_then(|d| d.id().ok());

  host
    .devices()
    .map(|devices| {
      devices
        .filter_map(|device| {
          let id = device.id().ok()?;
          let config = device.default_output_config().ok()?;

          // Get supported configurations
          let (supported_sample_rates, buffer_size_range) = device
            .supported_output_configs()
            .map(get_device_capabilities)
            .unwrap_or_default();

          Some(AudioDeviceInfo {
            is_default: default_device_id.as_ref() == Some(&id),
            id: id.to_string(),
            host_id: host_id.to_string(),
            name: device.description().ok()?.name().to_owned(),
            input_channels: 0,
            output_channels: config.channels(),
            sample_rate: config.sample_rate(),
            supported_sample_rates,
            buffer_size_range,
          })
        })
        .collect()
    })
    .unwrap_or_default()
}

/// Enumerate input devices for a specific host
fn enumerate_input_devices(host: &Host, host_id: &str) -> Vec<AudioDeviceInfo> {
  let default_device_id = host.default_input_device().and_then(|d| d.id().ok());

  host
    .input_devices()
    .map(|devices| {
      devices
        .filter_map(|device| {
          let id = device.id().ok()?;
          let config = device.default_input_config().ok()?;

          // Get supported configurations
          let (supported_sample_rates, buffer_size_range) = device
            .supported_input_configs()
            .map(get_device_capabilities)
            .unwrap_or_default();

          Some(AudioDeviceInfo {
            is_default: default_device_id.as_ref() == Some(&id),
            id: id.to_string(),
            host_id: host_id.to_string(),
            name: device.description().ok()?.name().to_owned(),
            input_channels: config.channels(),
            output_channels: 0,
            sample_rate: config.sample_rate(),
            supported_sample_rates,
            buffer_size_range,
          })
        })
        .collect()
    })
    .unwrap_or_default()
}

// Legacy functions for backward compatibility (now use cache internally)

/// List all available audio hosts
pub fn list_available_hosts() -> Vec<HostInfo> {
  cpal::available_hosts()
    .into_iter()
    .map(|host_id| {
      let name = format!("{:?}", host_id);
      HostInfo {
        id: format!("{:?}", host_id),
        name,
      }
    })
    .collect()
}

/// List all available audio output devices (legacy - enumerates fresh)
pub fn list_output_devices() -> Vec<AudioDeviceInfo> {
  let host = get_host_by_preference();
  let host_id = format!("{:?}", host.id());
  enumerate_output_devices(&host, &host_id)
}

/// List all available audio input devices (legacy - enumerates fresh)
pub fn list_input_devices() -> Vec<AudioDeviceInfo> {
  let host = get_host_by_preference();
  let host_id = format!("{:?}", host.id());
  enumerate_input_devices(&host, &host_id)
}

/// Find an output device by id
pub fn find_output_device(id: &str) -> Option<cpal::Device> {
  let host = get_host_by_preference();
  host
    .output_devices()
    .ok()?
    .find(|d| d.id().ok() == cpal::DeviceId::from_str(id).ok())
}

/// Find an input device by id
pub fn find_input_device(id: &str) -> Option<cpal::Device> {
  let host = get_host_by_preference();
  host
    .input_devices()
    .ok()?
    .find(|d| d.id().ok() == cpal::DeviceId::from_str(id).ok())
}

/// Find an output device by id in a specific host
pub fn find_output_device_in_host(host: &Host, id: &str) -> Option<cpal::Device> {
  host
    .output_devices()
    .ok()?
    .find(|d| d.id().ok() == cpal::DeviceId::from_str(id).ok())
}

/// Find an input device by id in a specific host
pub fn find_input_device_in_host(host: &Host, id: &str) -> Option<cpal::Device> {
  host
    .input_devices()
    .ok()?
    .find(|d| d.id().ok() == cpal::DeviceId::from_str(id).ok())
}

// ============================================================================
// Audio Input Ring Buffer (using ringbuf crate)
// ============================================================================

/// Ring buffer size for audio input (in frames, where each frame has PORT_MAX_CHANNELS samples)
const INPUT_RING_BUFFER_FRAMES: usize = 4096;

/// Total size of the ring buffer in samples
const INPUT_RING_BUFFER_SIZE: usize = INPUT_RING_BUFFER_FRAMES * PORT_MAX_CHANNELS;

/// Producer half of the input ring buffer (used by input stream callback)
pub type InputBufferProducer = ringbuf::HeapProd<f32>;

/// Consumer half of the input ring buffer (used by output stream callback)
pub type InputBufferConsumer = ringbuf::HeapCons<f32>;

/// Writer for input audio - owns the producer, moved into input stream closure
pub struct InputBufferWriter {
  producer: InputBufferProducer,
}

impl InputBufferWriter {
  /// Write interleaved samples to the ring buffer
  pub fn write(&mut self, data: &[f32]) {
    for &sample in data {
      // Drop samples if buffer is full (better than blocking)
      let _ = self.producer.try_push(sample);
    }
  }
}

/// Reader for input audio - owns the consumer + channel count, moved into output stream closure
pub struct InputBufferReader {
  consumer: InputBufferConsumer,
  channels: usize,
}

impl InputBufferReader {
  /// Read one frame of input audio (up to PORT_MAX_CHANNELS samples)
  pub fn read_frame(&mut self) -> [f32; PORT_MAX_CHANNELS] {
    let mut result = [0.0f32; PORT_MAX_CHANNELS];

    if self.channels == 0 {
      return result;
    }

    let samples_to_read = self.channels.min(PORT_MAX_CHANNELS);

    for i in 0..samples_to_read {
      if let Some(sample) = self.consumer.try_pop() {
        result[i] = sample;
      }
    }

    // Skip extra channels if input has more than PORT_MAX_CHANNELS
    for _ in samples_to_read..self.channels {
      let _ = self.consumer.try_pop();
    }

    result
  }
}

/// Create input ring buffer writer and reader
/// Pass writer to input stream, reader to output stream
pub fn create_input_ring_buffer(channels: usize) -> (InputBufferWriter, InputBufferReader) {
  let rb = HeapRb::<f32>::new(INPUT_RING_BUFFER_SIZE);
  let (producer, consumer) = rb.split();
  (
    InputBufferWriter { producer },
    InputBufferReader { consumer, channels },
  )
}

// ============================================================================
// Multi-Channel Output Buffer
// ============================================================================

/// Output buffer for multi-channel audio
/// Each DSP module can write to specific channels
pub struct OutputBuffer {
  /// Sample values per channel for current frame
  samples: [f32; PORT_MAX_CHANNELS],
  /// Number of active channels
  channels: u16,
}

impl OutputBuffer {
  pub fn new(channels: u16) -> Self {
    Self {
      samples: [0.0; PORT_MAX_CHANNELS],
      channels,
    }
  }

  /// Clear all samples to zero
  pub fn clear(&mut self) {
    for s in &mut self.samples[..self.channels as usize] {
      *s = 0.0;
    }
  }

  /// Add a sample to a specific channel (mixing)
  pub fn add(&mut self, channel: usize, value: f32) {
    if channel < self.channels as usize {
      self.samples[channel] += value;
    }
  }

  /// Set a sample for a specific channel (replacing)
  pub fn set(&mut self, channel: usize, value: f32) {
    if channel < self.channels as usize {
      self.samples[channel] = value;
    }
  }

  /// Get sample for a channel
  pub fn get(&self, channel: usize) -> f32 {
    if channel < self.channels as usize {
      self.samples[channel]
    } else {
      0.0
    }
  }

  pub fn channels(&self) -> u16 {
    self.channels
  }
}

fn apply_patch_debug_enabled() -> bool {
  static ENABLED: OnceLock<bool> = OnceLock::new();
  *ENABLED.get_or_init(|| match std::env::var("MODULAR_DEBUG_LOG") {
    Ok(v) => {
      let v = v.trim().to_ascii_lowercase();
      v == "1" || v == "true" || v == "yes" || v == "on"
    }
    Err(_) => false,
  })
}

fn format_id_set_sample(set: &HashSet<String>, max: usize) -> String {
  if set.is_empty() {
    return "(empty)".to_string();
  }

  let mut ids: Vec<&String> = set.iter().collect();
  ids.sort();

  let shown: Vec<&str> = ids.iter().take(max).map(|s| s.as_str()).collect();

  if set.len() <= max {
    shown.join(", ").to_string()
  } else {
    format!("{} …(+{})", shown.join(", "), set.len().saturating_sub(max))
  }
}

macro_rules! patch_dbg {
  ($($arg:tt)*) => {
    if apply_patch_debug_enabled() {
      eprintln!($($arg)*);
    }
  };
}

#[napi(object)]
pub struct ApplyPatchError {
  pub message: String,
  pub errors: Option<Vec<ValidationError>>,
}

use crate::validation::ValidationError;
use crate::validation::validate_patch;

/// Attenuation factor applied to audio output to prevent clipping.
/// DSP modules output signals in the range [-5, 5] volts (modular synth convention).
/// This factor brings the output into a reasonable range for audio output.
const AUDIO_OUTPUT_ATTENUATION: f32 = 0.2;

/// Gain factor applied to audio input.
/// Audio input from cpal is in the range [-1, 1]. This factor brings it into
/// the [-5, 5] volt range used by DSP modules (inverse of AUDIO_OUTPUT_ATTENUATION).
const AUDIO_INPUT_GAIN: f32 = 1.0 / AUDIO_OUTPUT_ATTENUATION;

/// Safety soft clipper: linear below the knee, tanh saturation above.
/// Prevents output from ever reaching ±1.0 to protect speakers and hearing.
const SAFETY_CLIP_KNEE: f32 = 0.9;
const SAFETY_CLIP_HEADROOM: f32 = 1.0 - SAFETY_CLIP_KNEE;

#[inline(always)]
fn safety_soft_clip(x: f32) -> f32 {
  if !x.is_finite() {
    return 0.0;
  }
  if x.abs() <= SAFETY_CLIP_KNEE {
    x
  } else {
    let sign = x.signum();
    let excess = x.abs() - SAFETY_CLIP_KNEE;
    let clipped = SAFETY_CLIP_KNEE + SAFETY_CLIP_HEADROOM * (excess / SAFETY_CLIP_HEADROOM).tanh();
    // tanh asymptotically approaches 1.0 but f32 can round to exactly 1.0 for large inputs
    sign * clipped.min(SAFETY_CLIP_KNEE + SAFETY_CLIP_HEADROOM * 0.9999)
  }
}

const SCOPE_CAPACITY: u32 = 1024;

use modular_core::types::ScopeStats;

// Adapted from https://github.com/VCVRack/Fundamental/blob/e819498fd388755efcb876b37d1e33fddf4a29ac/src/Scope.cpp
pub struct ScopeBuffer {
  sample_counter: u32,
  skip_rate: u32,
  trigger_threshold: Option<(f32, ScopeMode)>,
  trigger: SchmittTrigger,
  buffer: [[f32; SCOPE_CAPACITY as usize]; 2],
  buffer_select: bool,
  recording: bool,
  buffer_idx: usize,
  read_idx: usize,
}

fn ms_to_samples(ms: u32, sample_rate: f32) -> u32 {
  ((ms as f32 / 1000.0) * sample_rate) as u32
}

// A function that calculates the skip rate needed to capture target samples over total samples
fn calculate_skip_rate(total_samples: u32) -> u32 {
  total_samples / SCOPE_CAPACITY
}

impl ScopeBuffer {
  pub fn new(
    ms_per_frame: u32,
    trigger_threshold: Option<(i32, ScopeMode)>,
    sample_rate: f32,
  ) -> Self {
    let trigger_f = trigger_threshold.map(|(t, mode)| ((t as f32) / 1000.0, mode));
    let thresh_val = trigger_f.map(|(t, _)| t).unwrap_or(0.0);
    Self {
      buffer: [[0.0; SCOPE_CAPACITY as usize]; 2],
      sample_counter: 0,
      skip_rate: calculate_skip_rate(ms_to_samples(ms_per_frame, sample_rate)),
      trigger_threshold: trigger_f,
      trigger: SchmittTrigger::new(thresh_val, thresh_val + 0.001),
      buffer_select: false,
      recording: false,
      buffer_idx: 0,
      read_idx: 0,
    }
  }

  pub fn push(&mut self, value: f32) {
    if self.trigger_threshold.is_none() {
      self.trigger.reset();
      self.recording = true;
      self.read_idx = self.buffer_idx;
    } else if self.trigger.process(value) && !self.recording {
      self.trigger.reset();
      self.recording = true;
      self.buffer_idx = 0;
      self.read_idx = 0;
      self.sample_counter = 0;
    }

    self.buffer_idx %= SCOPE_CAPACITY as usize;
    self.read_idx %= SCOPE_CAPACITY as usize;

    let write_buf = if self.buffer_select { 1 } else { 0 };

    if self.recording {
      if self.sample_counter == 0 {
        self.buffer[write_buf][self.buffer_idx] = value;
        self.buffer_idx += 1;
        if self.buffer_idx >= SCOPE_CAPACITY as usize {
          match self.trigger_threshold {
            Some((_, ScopeMode::Wait)) => {
              self.recording = false;
              self.buffer_select = !self.buffer_select;
            }
            Some((_, ScopeMode::Roll)) => {
              self.recording = false;
            }
            None => { /* keep recording continuously */ }
          }
        }
      }
      self.sample_counter += 1;
      if self.sample_counter > self.skip_rate {
        self.sample_counter = 0;
      }
    }
  }

  fn read_buffer_idx(&self) -> usize {
    let write_buf = if self.buffer_select { 1 } else { 0 };
    let other_buf = if write_buf == 0 { 1 } else { 0 };
    match self.trigger_threshold {
      Some((_, ScopeMode::Wait)) => other_buf,
      Some((_, ScopeMode::Roll)) => write_buf,
      None => write_buf,
    }
  }

  pub fn get_buffer(&self) -> Float32Array {
    Float32Array::new(self.buffer[self.read_buffer_idx()].to_vec())
  }

  pub fn compute_stats(&self) -> ScopeStats {
    let buf = &self.buffer[self.read_buffer_idx()];
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for &val in buf.iter() {
      if val < min {
        min = val;
      }
      if val > max {
        max = val;
      }
    }
    if min == f32::MAX {
      min = 0.0;
    }
    if max == f32::MIN {
      max = 0.0;
    }
    ScopeStats {
      min: min as f64,
      max: max as f64,
      peak_to_peak: (max - min) as f64,
      read_offset: self.read_idx as u32,
    }
  }
}

// ============================================================================
// Command Queue Types
// ============================================================================

/// Producer end of the command queue (main thread → audio thread)
pub type CommandProducer = RtrbProducer<GraphCommand>;
/// Consumer end of the command queue (audio thread ← main thread)
pub type CommandConsumer = RtrbConsumer<GraphCommand>;

/// Producer end of the error queue (audio thread → main thread)
pub type ErrorProducer = RtrbProducer<AudioError>;
/// Consumer end of the error queue (main thread ← audio thread)
pub type ErrorConsumer = RtrbConsumer<AudioError>;

/// Producer end of the garbage queue (audio thread → main thread)
pub type GarbageProducer = RtrbProducer<GarbageItem>;
/// Consumer end of the garbage queue (main thread ← audio thread)
pub type GarbageConsumer = RtrbConsumer<GarbageItem>;

/// Create the command, error, and garbage queues for audio thread communication
pub fn create_audio_channels() -> (
  CommandProducer,
  CommandConsumer,
  ErrorProducer,
  ErrorConsumer,
  GarbageProducer,
  GarbageConsumer,
) {
  let (cmd_prod, cmd_cons) = RingBuffer::new(COMMAND_QUEUE_CAPACITY);
  let (err_prod, err_cons) = RingBuffer::new(ERROR_QUEUE_CAPACITY);
  let (garbage_prod, garbage_cons) = RingBuffer::new(GARBAGE_QUEUE_CAPACITY);
  (
    cmd_prod,
    cmd_cons,
    err_prod,
    err_cons,
    garbage_prod,
    garbage_cons,
  )
}

// ============================================================================
// AudioStateHandle - Main thread side
// ============================================================================

/// Main thread handle for audio state. Sends commands to audio thread.
pub struct AudioState {
  /// Command queue producer (main thread → audio thread)
  command_tx: Mutex<CommandProducer>,
  /// Error queue consumer (main thread ← audio thread)
  error_rx: Mutex<ErrorConsumer>,
  /// Garbage queue consumer - drains deferred deallocations from audio thread
  garbage_rx: Mutex<GarbageConsumer>,
  /// Stopped flag - shared with audio thread for quick reads
  stopped: Arc<AtomicBool>,
  /// Scope collection - shared with audio thread for UI reads
  scope_collection: Arc<Mutex<HashMap<ScopeBufferKey, ScopeBuffer>>>,
  /// Recording writer - shared with audio thread
  recording_writer: Arc<Mutex<Option<WavWriter<BufWriter<File>>>>>,
  /// Recording path
  recording_path: Arc<Mutex<Option<PathBuf>>>,
  /// Sample rate
  sample_rate: f32,
  /// Output channels
  channels: u16,
  /// Audio budget meter - written by audio thread, read by main thread
  audio_budget_meter: Arc<AudioBudgetMeter>,
  /// Module states (e.g., seq current step) - written by audio thread, read by main thread
  module_states: Arc<Mutex<HashMap<String, serde_json::Value>>>,
  /// MIDI input manager - shared with audio thread for polling
  midi_manager: Arc<MidiInputManager>,
  /// Transport state meter - written by audio thread, read by main thread
  pub transport_meter: Arc<TransportMeter>,
}

#[derive(Default)]
struct AudioThreadHealth {
  estimated_frame_budget_usage_max: AtomicU64,
}

#[derive(Debug, Clone, Copy)]
#[napi(object)]
pub struct AudioThreadHealthSnapshot {
  pub estimated_frame_budget_usage_max: f64,
}

impl AudioState {
  /// Create a new AudioState with command queue channels
  pub fn new_with_channels(
    command_tx: CommandProducer,
    error_rx: ErrorConsumer,
    garbage_rx: GarbageConsumer,
    sample_rate: f32,
    channels: u16,
    midi_manager: Arc<MidiInputManager>,
  ) -> Self {
    Self {
      command_tx: Mutex::new(command_tx),
      error_rx: Mutex::new(error_rx),
      garbage_rx: Mutex::new(garbage_rx),
      stopped: Arc::new(AtomicBool::new(true)),
      scope_collection: Arc::new(Mutex::new(HashMap::new())),
      recording_writer: Arc::new(Mutex::new(None)),
      recording_path: Arc::new(Mutex::new(None)),
      sample_rate,
      channels,
      audio_budget_meter: Arc::new(AudioBudgetMeter::default()),
      module_states: Arc::new(Mutex::new(HashMap::new())),
      midi_manager,
      transport_meter: Arc::new(TransportMeter::default()),
    }
  }

  /// Send a command to the audio thread
  pub(crate) fn send_command(&self, cmd: GraphCommand) -> Result<()> {
    let mut tx = self.command_tx.lock();
    tx.push(cmd).map_err(|_| {
      napi::Error::from_reason("Command queue full - audio thread may be overloaded".to_string())
    })
  }

  /// Drain any errors accumulated on the audio thread
  pub fn drain_errors(&self) -> Vec<AudioError> {
    let mut rx = self.error_rx.lock();
    let mut errors = Vec::new();
    while let Ok(err) = rx.pop() {
      errors.push(err);
    }
    errors
  }

  /// Drain deferred deallocations from the audio thread.
  /// Items are simply dropped on the main thread where allocation/deallocation is safe.
  pub fn drain_garbage(&self) {
    let mut rx = self.garbage_rx.lock();
    while let Ok(_item) = rx.pop() {
      // Item is dropped here on the main thread - this is the whole point
    }
  }

  pub fn take_audio_thread_budget_snapshot_and_reset(&self) -> AudioBudgetSnapshot {
    self
      .audio_budget_meter
      .take_snapshot(self.sample_rate as f64, self.channels as f64)
  }

  /// Stop transport. Flips the atomic immediately (stop is synchronous by
  /// design — no quantize) and notifies the audio thread so it can tear down
  /// pending_start and propagate to Link.
  pub fn request_stop(&self) {
    self.stopped.store(true, Ordering::SeqCst);
    let _ = self.send_command(GraphCommand::Stop);
  }

  /// Request transport start. Does NOT flip the atomic — the audio thread's
  /// Start handler owns that, immediately in free-run and at the next bar
  /// boundary when Link is enabled. This avoids a one-buffer leak window
  /// where the main thread's flip could be observed before the Start handler
  /// re-armed the quantize.
  pub fn request_start(&self) {
    let _ = self.send_command(GraphCommand::Start);
  }

  pub fn is_stopped(&self) -> bool {
    self.stopped.load(Ordering::SeqCst)
  }

  /// Read the current transport state snapshot (lock-free)
  pub fn get_transport_state(&self) -> TransportSnapshot {
    self.transport_meter.snapshot()
  }

  /// Get shared references for audio processor creation
  pub fn get_shared_state(&self) -> AudioSharedState {
    AudioSharedState {
      stopped: self.stopped.clone(),
      scope_collection: self.scope_collection.clone(),
      recording_writer: self.recording_writer.clone(),
      audio_budget_meter: self.audio_budget_meter.clone(),
      module_states: self.module_states.clone(),
      midi_manager: self.midi_manager.clone(),
      transport_meter: self.transport_meter.clone(),
    }
  }

  pub fn start_recording(&self, filename: Option<String>) -> Result<String> {
    let filename =
      filename.unwrap_or_else(|| format!("recording_{}.wav", chrono_simple_timestamp()));
    let path = PathBuf::from(&filename);

    let spec = WavSpec {
      channels: 1,
      sample_rate: self.sample_rate as u32,
      bits_per_sample: 32,
      sample_format: hound::SampleFormat::Float,
    };

    let writer = WavWriter::create(&path, spec)
      .map_err(|e| napi::Error::from_reason(format!("Failed to start file write: {}", e)))?;
    *self.recording_writer.lock() = Some(writer);
    *self.recording_path.lock() = Some(path);

    Ok(filename)
  }

  pub fn stop_recording(&self) -> Result<Option<String>> {
    let writer = self.recording_writer.lock().take();
    let path = self.recording_path.lock().take();

    if let Some(w) = writer {
      w.finalize()
        .map_err(|e| napi::Error::from_reason(format!("Failed to finalize file writer: {}", e)))?;
    }

    Ok(path.map(|p| p.to_string_lossy().to_string()))
  }

  pub fn get_audio_buffers(&self) -> Vec<(ScopeBufferKey, Float32Array, ScopeStats)> {
    // Skip emitting audio buffers entirely when stopped
    if self.is_stopped() {
      return Vec::new();
    }

    let scope_collection = match self.scope_collection.try_lock() {
      Some(sc) => sc,
      None => return Vec::new(),
    };
    scope_collection
      .iter()
      .map(|(key, buffer)| {
        let data = buffer.get_buffer();
        let stats = buffer.compute_stats();
        (key.clone(), data, stats)
      })
      .collect()
  }

  pub fn get_module_states(&self) -> HashMap<String, serde_json::Value> {
    // Read module states from shared buffer (written by audio thread)
    match self.module_states.try_lock() {
      Some(states) => states.clone(),
      None => HashMap::new(), // Skip if locked by audio thread
    }
  }

  /// Build a PatchUpdate from desired graph and send to audio thread.
  /// This computes the diff using the shadow state and constructs new modules on the main thread.
  pub fn apply_patch(
    &self,
    desired_graph: PatchGraph,
    sample_rate: f32,
    trigger: QueuedTrigger,
    update_id: u64,
    wav_data: HashMap<String, Arc<modular_core::types::WavData>>,
    tempo_override: Option<f64>,
  ) -> Result<()> {
    let PatchGraph {
      modules,
      module_id_remaps,
      scopes,
      ..
    } = desired_graph;

    // Build PatchUpdate with all the info needed
    let mut update = PatchUpdate::new(sample_rate);
    update.update_id = update_id;

    // Add remaps
    update.remaps = module_id_remaps.unwrap_or_default();

    // Build maps for efficient lookup
    let desired_modules: HashMap<String, _> =
      modules.into_iter().map(|m| (m.id.clone(), m)).collect();

    // Compute scopes to add/remove (no updates — key includes config)
    {
      let scope_collection = self.scope_collection.lock();
      let current_keys: HashSet<ScopeBufferKey> = scope_collection.keys().cloned().collect();

      // Expand desired scopes into per-channel buffer keys
      let desired_keys: HashSet<ScopeBufferKey> = scopes
        .iter()
        .flat_map(|scope| {
          scope.channels.iter().map(move |ch| ScopeBufferKey {
            module_id: ch.module_id.clone(),
            port_name: ch.port_name.clone(),
            channel: ch.channel,
            ms_per_frame: scope.ms_per_frame,
            trigger_threshold: scope.trigger_threshold,
          })
        })
        .collect();

      // Scopes to remove
      update.scope_removes = current_keys.difference(&desired_keys).cloned().collect();

      // Scopes to add (pre-build ScopeBuffers on main thread)
      update.scope_adds = desired_keys
        .difference(&current_keys)
        .map(|key| {
          let buffer = ScopeBuffer::new(key.ms_per_frame, key.trigger_threshold, sample_rate);
          (key.clone(), buffer)
        })
        .collect();
    }

    // Construct all modules that appear in desired graph on main thread.
    // The audio thread will always replace existing modules and transfer state.
    let constructors = get_constructors();
    for (id, module_state) in desired_modules {
      // Deserialize params FIRST (before construction)
      let deserialized =
        crate::deserialize_params(&module_state.module_type, module_state.params, true).map_err(
          |e| napi::Error::from_reason(format!("Failed to deserialize params for {}: {}", id, e)),
        )?;

      if let Some(constructor) = constructors.get(&module_state.module_type) {
        match constructor(&id, sample_rate, deserialized) {
          Ok(module) => {
            update.inserts.push((id.clone(), module));
          }
          Err(err) => {
            return Err(napi::Error::from_reason(format!(
              "Failed to create module {}: {}",
              id, err
            )));
          }
        }
      } else {
        return Err(napi::Error::from_reason(format!(
          "{} is not a valid module type",
          module_state.module_type
        )));
      }
    }

    // Pre-compute desired IDs on main thread to avoid HashSet allocation on audio thread
    update.desired_ids = update.inserts.iter().map(|(id, _)| id.clone()).collect();

    // Run main-thread resource preparation (e.g. FFT-based mipmap generation for
    // wavetable oscillators). Called here because allocation and file-backed
    // data access must not happen on the audio thread.
    for (_id, module) in update.inserts.iter() {
      module.prepare_resources(&wav_data);
    }

    // Populate wav_data from the cache snapshot
    update.wav_data = wav_data;

    // Pass through tempo override for Link integration
    update.tempo_override = tempo_override;

    // Send the update to audio thread
    self.send_command(GraphCommand::QueuedPatchUpdate { update, trigger })
  }

  pub fn handle_set_patch(
    &self,
    patch_graph: PatchGraph,
    sample_rate: f32,
    trigger: QueuedTrigger,
    update_id: u64,
    wav_data: HashMap<String, Arc<modular_core::types::WavData>>,
    tempo_override: Option<f64>,
  ) -> Vec<ApplyPatchError> {
    // Validate patch
    let schemas = schema();
    if let Err(errors) = validate_patch(&patch_graph, &schemas) {
      return vec![ApplyPatchError {
        message: "Validation failed".to_string(),
        errors: Some(errors),
      }];
    }

    // If stopped, clear audio-thread state and request a start. The audio
    // thread's Start handler flips `stopped`: immediately when free-running,
    // at the next bar boundary when Link is enabled (the quantize also
    // surfaces in the UI via `link_pending_start`).
    if self.is_stopped() {
      let _ = self.send_command(GraphCommand::ClearPatch);
      self.scope_collection.lock().clear();
      self.request_start();
    }

    // Apply patch
    if let Err(e) = self.apply_patch(
      patch_graph,
      sample_rate,
      trigger,
      update_id,
      wav_data,
      tempo_override,
    ) {
      return vec![ApplyPatchError {
        message: format!("Failed to apply patch: {}", e),
        errors: None,
      }];
    }

    // No errors
    vec![]
  }
}

/// Shared state that both AudioState (main thread) and AudioProcessor (audio thread) can access
pub struct AudioSharedState {
  pub stopped: Arc<AtomicBool>,
  pub scope_collection: Arc<Mutex<HashMap<ScopeBufferKey, ScopeBuffer>>>,
  pub recording_writer: Arc<Mutex<Option<WavWriter<BufWriter<File>>>>>,
  pub audio_budget_meter: Arc<AudioBudgetMeter>,
  /// Module states (e.g., seq current step) - written by audio thread, read by main thread
  pub module_states: Arc<Mutex<HashMap<String, serde_json::Value>>>,
  /// MIDI input manager for polling MIDI messages
  pub midi_manager: Arc<MidiInputManager>,
  /// Transport state meter - written by audio thread, read by main thread
  pub transport_meter: Arc<TransportMeter>,
}

fn chrono_simple_timestamp() -> String {
  use std::time::{SystemTime, UNIX_EPOCH};
  let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
  format!("{}", duration.as_secs())
}

// ============================================================================
// AudioProcessor - Audio thread side
// ============================================================================
//
// Ableton Link integration lives in `crate::link`. The audio thread holds a
// `LinkState` that owns the live `rusty_link` resources and exposes only
// realtime-safe operations.

/// Audio processor that runs on the audio thread.
/// Owns the Patch directly and processes commands from the main thread.
struct AudioProcessor {
  /// The DSP patch graph - owned directly, no mutex needed
  patch: Patch,
  /// Command queue consumer
  command_rx: CommandConsumer,
  /// Error queue producer
  error_tx: ErrorProducer,
  /// Garbage queue producer (audio thread → main thread)
  garbage_tx: GarbageProducer,
  /// Shared stopped flag
  stopped: Arc<AtomicBool>,
  /// Shared scope collection
  scope_collection: Arc<Mutex<HashMap<ScopeBufferKey, ScopeBuffer>>>,
  /// Shared module states (e.g., seq current step)
  module_states: Arc<Mutex<HashMap<String, serde_json::Value>>>,
  /// MIDI input manager for polling
  midi_manager: Arc<MidiInputManager>,
  /// Queued patch update waiting for its trigger condition
  queued_update: Option<(PatchUpdate, QueuedTrigger)>,
  /// Transport state meter - written each frame, read by main thread
  transport_meter: Arc<TransportMeter>,
  /// Sample rate of the audio stream
  sample_rate: f32,
  /// Ableton Link integration (audio-thread side). Owns the live
  /// `rusty_link` resources when active and exposes only RT-safe operations.
  link: crate::link::LinkState,
}

impl AudioProcessor {
  fn new(
    command_rx: CommandConsumer,
    error_tx: ErrorProducer,
    garbage_tx: GarbageProducer,
    shared: AudioSharedState,
    sample_rate: f32,
  ) -> Self {
    Self {
      patch: Patch::new(),
      command_rx,
      error_tx,
      garbage_tx,
      stopped: shared.stopped,
      scope_collection: shared.scope_collection,
      module_states: shared.module_states,
      midi_manager: shared.midi_manager,
      queued_update: None,
      transport_meter: shared.transport_meter,
      sample_rate,
      link: crate::link::LinkState::new(),
    }
  }

  /// Process all pending commands from the main thread and poll MIDI.
  /// Called at the start of each audio callback before processing frames.
  fn process_commands(&mut self) {
    // Poll MIDI messages and dispatch directly to the patch
    // This happens in the audio thread for low-latency MIDI response
    for msg in self.midi_manager.take_messages() {
      if let Err(e) = self.patch.dispatch_message(&msg) {
        let _ = self.error_tx.push(AudioError::MessageDispatchFailed {
          message: e.to_string(),
        });
      }
    }

    // Process commands from the main thread
    while let Ok(cmd) = self.command_rx.pop() {
      match cmd {
        GraphCommand::QueuedPatchUpdate { update, trigger } => {
          // Push tempo to Link if explicitly set via $setTempo. The
          // capture/commit call inside `set_tempo_now` is the realtime-safe
          // way to mutate session state per Ableton's docs.
          if let Some(tempo) = update.tempo_override {
            self.link.set_tempo_now(tempo);
          }

          // If there's already a queued update, discard it and apply the new one
          // immediately. This is intentional: when the user triggers a second
          // update before the first one fires (e.g. pressing Ctrl+Enter twice),
          // we treat it as "apply now" rather than re-queuing for the next
          // bar/beat.
          if let Some((old_update, _)) = self.queued_update.take() {
            if let Err(err) = self.garbage_tx.push(GarbageItem::PatchUpdate(old_update)) {
              println!(
                "Failed to push old patch update to garbage queue: ${:?}",
                err
              );
            } // If queue is full, old update will be dropped here as fallback (not ideal but safe)
            self.queued_update = Some((update, QueuedTrigger::Immediate));
          } else {
            self.queued_update = Some((update, trigger));
          }
        }
        GraphCommand::SingleModuleUpdate {
          module_id,
          module: new_module,
        } => {
          // State transfer + replace, then reconnect
          if let Some(old_module) = self.patch.sampleables.remove(&module_id) {
            new_module.transfer_state_from(old_module.as_ref());
            self.patch.remove_message_listeners_for_module(&module_id);
            if self
              .garbage_tx
              .push(GarbageItem::Module(old_module))
              .is_err()
            {}
          }
          self.patch.sampleables.insert(module_id.clone(), new_module);
          // Re-register message listeners for the replaced module
          self.patch.add_message_listeners_for_module(&module_id);
          // Reconnect all modules so the new module picks up its connections
          for module in self.patch.sampleables.values() {
            module.connect(&self.patch);
          }
          // Refresh any module-local caches rebuilt from params after reconnect.
          for module in self.patch.sampleables.values() {
            module.on_patch_update();
          }
        }
        GraphCommand::DispatchMessage(msg) => {
          if let Err(e) = self.patch.dispatch_message(&msg) {
            let _ = self.error_tx.push(AudioError::MessageDispatchFailed {
              message: e.to_string(),
            });
          }
        }
        GraphCommand::Start => {
          if self.link.is_active() {
            // With Link: keep `stopped=true` and arm a pending start.
            // The buffer-level phase check releases `stopped` when the
            // shared Link timeline reaches a bar boundary.
            self
              .stopped
              .store(true, std::sync::atomic::Ordering::SeqCst);
            self.link.request_quantized_start();
          } else {
            // Free-run: flip immediately and reset the clock.
            self
              .stopped
              .store(false, std::sync::atomic::Ordering::SeqCst);
            let _ = self
              .patch
              .dispatch_message(&Message::Clock(ClockMessages::Start));
          }
        }
        GraphCommand::Stop => {
          // Stop is handled via the stopped flag.
          // Also clear any pending start so a later peer-start does not
          // resurrect a cancelled patch, and propagate stop to peers.
          self.link.clear_pending_start();
          self.link.signal_stop();
        }
        GraphCommand::ClearPatch => {
          // Discard any pending queued update
          if let Some((old_update, _)) = self.queued_update.take() {
            if let Err(err) = self.garbage_tx.push(GarbageItem::PatchUpdate(old_update)) {
              println!(
                "Failed to push old patch update to garbage queue: ${:?}",
                err
              );
            } // If queue is full, old update will be dropped here as fallback (not ideal but safe)
          }
          // Defer deallocation of all non-reserved modules to main thread
          let ids_to_remove: Vec<String> = self.patch.sampleables.keys().cloned().collect();
          for id in ids_to_remove {
            if let Some(module) = self.patch.sampleables.remove(&id) {
              let _ = self.garbage_tx.push(GarbageItem::Module(module));
            }
          }
          self.patch.insert_audio_in();
          self.patch.rebuild_message_listeners();
        }
        GraphCommand::SetLink(new_resources) => {
          // Construction/destruction of `AblLink` (and `enable()`) are
          // realtime-unsafe per Ableton's documentation. They run on the
          // main thread; the audio thread only swaps the prepared resources
          // in/out and ships any old set off to the garbage queue for
          // main-thread drop.
          let was_active = self.link.is_active();
          if let Some(old) = self.link.install(new_resources) {
            // If the queue is full, the Box drops here on the audio thread
            // as a fallback — not ideal RT-wise but bounded and strictly
            // better than leaking the live networking resources.
            let _ = self.garbage_tx.push(GarbageItem::Link(old));
          }

          // When the live instance changes, clear any external clock sync
          // on ROOT_CLOCK so the patch's own clock takes over.
          if was_active {
            use modular_core::types::ROOT_CLOCK_ID;
            if let Some(root_clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) {
              root_clock.clear_external_sync();
            }
          }
        }
      }
    }
  }

  /// Apply a patch update command
  fn apply_patch_update(&mut self, update: PatchUpdate) {
    let PatchUpdate {
      remaps,
      inserts,
      desired_ids,
      scope_adds,
      scope_removes,
      wav_data,
      ..
    } = update;

    // Apply remaps in two phases to avoid chain/swap collisions.
    // Phase 1: Extract all remap sources into a temporary map.
    // Phase 2: Insert them at their new IDs.
    // This prevents chains (A→B, B→C) from overwriting modules that are
    // themselves being remapped, and also handles swaps (A→B, B→A).
    let mut remapped_ids: Vec<String> = Vec::new();
    let mut remap_staging = Vec::new();
    let filtered_remaps: Vec<_> = remaps
      .into_iter()
      .filter(|r| {
        !is_reserved_module_id(&r.from) && !is_reserved_module_id(&r.to) && r.from != r.to
      })
      .collect();

    // Phase 1: Remove all sources
    for remap in &filtered_remaps {
      if let Some(module) = self.patch.sampleables.remove(&remap.from) {
        self.patch.remove_message_listeners_for_module(&remap.from);
        remap_staging.push((remap.to.clone(), module));
      }
    }

    // Phase 2: Insert at destinations
    for (to_id, module) in remap_staging {
      self.patch.sampleables.insert(to_id.clone(), module);
      remapped_ids.push(to_id);
    }

    // Always-replace: insert new modules, transferring state from old ones.
    // Every module is reconstructed on the main thread with fresh params.
    // State continuity is preserved by transfer_state_from().
    let mut newly_inserted_ids: Vec<String> = Vec::new();
    for (id, new_module) in inserts {
      if let Some(old_module) = self.patch.sampleables.remove(&id) {
        new_module.transfer_state_from(old_module.as_ref());
        self.patch.remove_message_listeners_for_module(&id);
        if self
          .garbage_tx
          .push(GarbageItem::Module(old_module))
          .is_err()
        {}
      }
      newly_inserted_ids.push(id.clone());
      self.patch.sampleables.insert(id, new_module);
    }

    // Remove modules that are no longer in the desired graph.
    // Stale modules are sent to the garbage queue for deallocation on the main thread,
    // avoiding Drop running in the audio callback.
    let stale_ids: Vec<String> = self
      .patch
      .sampleables
      .keys()
      .filter(|id| !is_reserved_module_id(id) && !desired_ids.contains(*id))
      .cloned()
      .collect();
    for id in stale_ids {
      if let Some(module) = self.patch.sampleables.remove(&id) {
        self.patch.remove_message_listeners_for_module(&id);
        if self.garbage_tx.push(GarbageItem::Module(module)).is_err() {}
      }
    }

    // Incrementally update message listeners for changed modules only.
    // Stale entries were already removed above; now add entries for new and remapped modules.
    for id in newly_inserted_ids.iter().chain(remapped_ids.iter()) {
      self.patch.add_message_listeners_for_module(id);
    }

    // Swap wav_data into the patch (cheap — just moving Arc clones).
    // Old Arc<WavData> refs just decrement refcount — no audio data freed
    // because the main-thread WavCache still holds references.
    if !wav_data.is_empty() || !self.patch.wav_data.is_empty() {
      let old_wav_data = std::mem::replace(&mut self.patch.wav_data, wav_data);
      drop(old_wav_data);
    }

    // Connect all modules
    for module in self.patch.sampleables.values() {
      module.connect(&self.patch);
    }

    // Call on_patch_update for all modules
    for module in self.patch.sampleables.values() {
      module.on_patch_update();
    }

    // Update scopes
    {
      let mut scope_collection = self.scope_collection.lock();

      // Remove scopes
      for key in &scope_removes {
        if let Some(buffer) = scope_collection.remove(key) {
          let _ = self.garbage_tx.push(GarbageItem::Scope(buffer));
        }
      }

      // Add new scopes
      for (key, buffer) in scope_adds {
        scope_collection.insert(key, buffer);
      }
    }
  }

  /// Process a single frame, returning multi-channel output
  fn process_frame(&mut self, num_channels: usize) -> [f32; PORT_MAX_CHANNELS] {
    use modular_core::types::{ROOT_CLOCK_ID, ROOT_ID};
    profiling::scope!("process_frame");

    let mut output = [0.0f32; PORT_MAX_CHANNELS];

    if self.is_stopped() {
      return output; // Skip processing when stopped
    }

    // 1. Sync Link state to ROOT_CLOCK, then update ROOT_CLOCK
    let root_clock = self.patch.sampleables.get(&*ROOT_CLOCK_ID).cloned();
    self.link.sync_frame(|bar_phase, tempo| {
      if let Some(ref clock) = root_clock {
        clock.sync_external_clock(bar_phase, tempo);
      }
    });

    // 2. Update ROOT_CLOCK so its trigger outputs are available this frame
    if let Some(root_clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) {
      root_clock.update();
    }

    // 2. Check queued update trigger against ROOT_CLOCK outputs
    let should_apply = if let Some((_, trigger)) = self.queued_update.as_ref() {
      match trigger {
        QueuedTrigger::Immediate => true,
        QueuedTrigger::NextBar => {
          if let Some(clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) {
            clock
              .get_poly_sample("barTrigger")
              .map(|p| p.get(0) >= 1.0)
              .unwrap_or(true)
          } else {
            true // No clock module = apply immediately
          }
        }
        QueuedTrigger::NextBeat => {
          if let Some(clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) {
            clock
              .get_poly_sample("beatTrigger")
              .map(|p| p.get(0) >= 1.0)
              .unwrap_or(true)
          } else {
            true
          }
        }
      }
    } else {
      false
    };

    // 3. If triggered, apply the patch update
    if should_apply {
      let (update, _) = self.queued_update.take().unwrap();
      let applied_id = update.update_id;
      self.apply_patch_update(update);
      self.transport_meter.write_applied_update_id(applied_id);
    }

    // 4. Update all modules (ROOT_CLOCK won't re-run due to CAS guard;
    //    newly inserted modules participate on this same frame)
    {
      profiling::scope!("update_modules");
      for module in self.patch.sampleables.values() {
        module.update();
      }
    }

    // 4.5 Write transport state from ROOT_CLOCK outputs (CAS guard prevents re-execution)
    {
      let has_queued = self.queued_update.is_some();
      if let Some(clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) {
        let bar_phase = clock
          .get_poly_sample("playhead")
          .map(|p| p.get(0) as f64)
          .unwrap_or(0.0);
        let bar_count = clock
          .get_poly_sample("playhead")
          .map(|p| p.get(1) as u64)
          .unwrap_or(0);
        let beat_in_bar = clock
          .get_poly_sample("beatInBar")
          .map(|p| p.get(0) as u32)
          .unwrap_or(0);
        let is_playing = !self.is_stopped();
        self.transport_meter.write_from_audio(
          bar_phase,
          bar_count,
          beat_in_bar,
          is_playing,
          has_queued,
        );
      } else {
        self
          .transport_meter
          .write_from_audio(0.0, 0, 0, false, has_queued);
      }
    }

    // 5. Tick all modules (reset processed flags)
    {
      profiling::scope!("tick_modules");
      for module in self.patch.sampleables.values() {
        module.tick();
      }
    }

    // Capture audio for scopes
    {
      profiling::scope!("capture_scopes");
      let mut scope_lock = self.scope_collection.lock();
      for (key, scope_buffer) in scope_lock.iter_mut() {
        if let Some(module) = self.patch.sampleables.get(&key.module_id)
          && let Ok(poly) = module.get_poly_sample(&key.port_name)
        {
          let sample = if (key.channel as usize) < poly.channels() {
            poly.get(key.channel as usize)
          } else {
            0.0
          };
          scope_buffer.push(sample);
        }
      }
    }

    // Get output from root module
    if let Some(root) = self.patch.sampleables.get(&*ROOT_ID) {
      if let Ok(poly) = root.get_poly_sample(&ROOT_OUTPUT_PORT) {
        for ch in 0..num_channels.min(PORT_MAX_CHANNELS) {
          output[ch] = poly.get(ch) * AUDIO_OUTPUT_ATTENUATION;
        }
      }
    }

    output
  }

  /// Collect states from modules that implement StatefulModule (e.g., Seq).
  /// Uses try_lock to avoid blocking the audio thread if the main thread is reading.
  /// Reuses HashMap entries to avoid repeated String allocation on the audio thread.
  fn collect_module_states(&self) {
    // Use try_lock to avoid blocking audio if main thread is reading
    if let Some(mut states) = self.module_states.try_lock() {
      // Remove entries for modules that no longer exist
      states.retain(|id, _| self.patch.sampleables.contains_key(id));

      // Update existing entries and add new ones
      for (id, module) in &self.patch.sampleables {
        if let Some(state) = module.get_state() {
          if let Some(existing) = states.get_mut(id.as_str()) {
            *existing = state;
          } else {
            states.insert(id.clone(), state);
          }
        }
      }
    }
  }

  fn is_stopped(&self) -> bool {
    self.stopped.load(Ordering::SeqCst)
  }
}

/// Check if a module ID is reserved (well-known system module)
fn is_reserved_module_id(id: &str) -> bool {
  id == WellKnownModule::RootOutput.id()
    || id == WellKnownModule::RootClock.id()
    || id == WellKnownModule::RootInput.id()
    || id == WellKnownModule::HiddenAudioIn.id()
}

pub fn make_stream<T>(
  device: &cpal::Device,
  config: &cpal::StreamConfig,
  command_rx: CommandConsumer,
  error_tx: ErrorProducer,
  garbage_tx: GarbageProducer,
  shared: AudioSharedState,
  mut input_reader: InputBufferReader,
) -> Result<cpal::Stream>
where
  T: SizedSample + FromSample<f32> + hound::Sample,
{
  let num_channels = config.channels as usize;

  let err_fn = |err| eprintln!("Error building output sound stream: {err}");

  let time_at_start = std::time::Instant::now();
  println!("Time at start: {time_at_start:?}");

  // Clone shared state for the closure
  let recording_writer = shared.recording_writer.clone();
  let audio_budget_meter = shared.audio_budget_meter.clone();

  // Create the audio processor that owns the patch
  let sample_rate = config.sample_rate as f32;
  let mut audio_processor =
    AudioProcessor::new(command_rx, error_tx, garbage_tx, shared, sample_rate);

  let mut final_state_processor = FinalStateProcessor::new(num_channels);

  let stream = device
    .build_output_stream(
      config,
      move |output: &mut [T], _info: &cpal::OutputCallbackInfo| {
        profiling::scope!("audio_callback");

        let callback_start = Instant::now();

        // Process any pending commands from the main thread
        {
          profiling::scope!("process_commands");
          audio_processor.process_commands();
        }

        // Capture Link session state once per buffer (before frame loop).
        audio_processor
          .link
          .capture_buffer_state(audio_processor.sample_rate);

        // Operator transport is decoupled from Link peer transport:
        // peers never start or stop Operator. The only Link-driven transport
        // action is releasing a locally-initiated quantized start when the
        // shared timeline reaches a bar boundary.
        if audio_processor.link.check_pending_start_release() {
          audio_processor
            .stopped
            .store(false, std::sync::atomic::Ordering::SeqCst);
          let msg = modular_core::types::Message::Clock(ClockMessages::Start);
          let _ = audio_processor.patch.dispatch_message(&msg);
        }

        // Write Link state to transport meter for UI visibility. Done
        // BEFORE the is_stopped() guard so the UI always shows the
        // free-running Link phase, even when Operator is stopped.
        audio_processor.link.write_meter(&audio_processor.transport_meter);

        let num_frames = output.len() / num_channels;

        {
          profiling::scope!("process_frames");
          for frame in output.chunks_mut(num_channels) {
            // Read from the input buffer and update audio_in
            {
              let mut audio_in = audio_processor.patch.audio_in.lock();
              let input_samples = input_reader.read_frame();

              // Set channel count so that get() returns values instead of 0.0
              audio_in.set_channels(PORT_MAX_CHANNELS);
              for i in 0..PORT_MAX_CHANNELS {
                // Apply gain to bring input from [-1, 1] to [-5, 5] volt range
                audio_in.set(i, input_samples[i] * AUDIO_INPUT_GAIN);
              }
            }

            // Process frame and get multi-channel output
            let samples = final_state_processor
              .process_frame_with_processor(&mut audio_processor, num_channels);

            for (ch, s) in frame.iter_mut().enumerate() {
              if ch < samples.len() {
                *s = T::from_sample(samples[ch]);
              } else {
                *s = T::from_sample(0.0);
              }
            }

            // Record if enabled (use try_lock to avoid blocking audio)
            // For multi-channel, record first channel (mono mix could be added later)
            if let Some(mut writer_guard) = recording_writer.try_lock()
              && let Some(ref mut writer) = *writer_guard
            {
              let _ = writer.write_sample(T::from_sample(samples[0]));
            }
          }
        }

        // Increment Link sample count for HostTimeFilter
        audio_processor.link.add_samples(num_frames as u64);

        // Collect module states for UI (e.g., seq step highlighting)
        // Done once per buffer, not per frame, to minimize overhead
        {
          profiling::scope!("collect_module_states");
          audio_processor.collect_module_states();
        }

        let elapsed_ns = callback_start.elapsed().as_nanos() as u64;

        audio_budget_meter.record_chunk(output.len() as u64, elapsed_ns);
      },
      err_fn,
      None,
    )
    .map_err(|e| napi::Error::from_reason(format!("Failed to build output stream: {}", e)))?;

  Ok(stream)
}

/// Build an input stream that writes to the input buffer
pub fn make_input_stream<T>(
  device: &cpal::Device,
  config: &cpal::StreamConfig,
  mut input_writer: InputBufferWriter,
) -> Result<cpal::Stream>
where
  T: SizedSample + cpal::Sample,
  f32: FromSample<T>,
{
  let err_fn = |err| eprintln!("Error building input sound stream: {err}");

  let mut f32_buffer: Vec<f32> = Vec::new();
  let stream = device
    .build_input_stream(
      config,
      move |data: &[T], _info: &cpal::InputCallbackInfo| {
        // Convert to f32 and write to ring buffer (reuse allocation)
        f32_buffer.clear();
        f32_buffer.extend(data.iter().map(|&s| f32::from_sample(s)));
        input_writer.write(&f32_buffer);
      },
      err_fn,
      None,
    )
    .map_err(|e| napi::Error::from_reason(format!("Failed to build input stream: {}", e)))?;

  Ok(stream)
}

pub fn get_host_by_preference() -> Host {
  #[cfg(target_os = "windows")]
  {
    // if let Ok(asio_host) = cpal::host_from_id(HostId::Asio) {
    //   println!("Using ASIO");
    //   return asio_host;
    // }

    // Fall back to WASAPI
    if let Ok(wasapi) = cpal::host_from_id(HostId::Wasapi) {
      println!("Using WASAPI");
      return wasapi;
    }
  }

  #[cfg(target_os = "macos")]
  {
    // Try CoreAudio on macOS
    if let Ok(coreaudio_host) = cpal::host_from_id(HostId::CoreAudio) {
      println!("Using CoreAudio");
      return coreaudio_host;
    }
  }

  #[cfg(target_os = "linux")]
  {
    if let Ok(jack_host) = cpal::host_from_id(HostId::Jack) {
      println!("Using JACK");
      return jack_host;
    }

    // Try ALSA on Linux
    if let Ok(alsa_host) = cpal::host_from_id(HostId::Alsa) {
      println!("Using ALSA");
      return alsa_host;
    }
  }

  // Fallback to the default host
  let default_host = cpal::default_host();
  println!("Using default host: {:?}", default_host.id());
  default_host
}

/// Get the sample rate from the default audio device
pub fn get_sample_rate() -> Result<f32> {
  let host = get_host_by_preference();
  let device = host
    .default_output_device()
    .ok_or_else(|| napi::Error::from_reason("No audio output device found".to_string()))?;
  let config = device
    .default_output_config()
    .map_err(|e| napi::Error::from_reason(format!("Failed to get default output config: {}", e)))?;
  Ok(config.sample_rate() as f32)
}

enum VolumeChange {
  Decrease,
  None,
}
struct FinalStateProcessor {
  attenuation_factor: f32,
  volume_change: VolumeChange,
  prev_is_stopped: bool,
  num_channels: usize,
}

impl FinalStateProcessor {
  fn new(num_channels: usize) -> Self {
    Self {
      attenuation_factor: 0.0,
      volume_change: VolumeChange::None,
      prev_is_stopped: true,
      num_channels,
    }
  }

  /// Process frame using AudioProcessor and return multi-channel output
  fn process_frame_with_processor(
    &mut self,
    processor: &mut AudioProcessor,
    num_channels: usize,
  ) -> [f32; PORT_MAX_CHANNELS] {
    let is_stopped = processor.is_stopped();
    match (self.prev_is_stopped, is_stopped) {
      (true, false) => {
        self.volume_change = VolumeChange::None;
        self.attenuation_factor = 1.0;
      }
      (false, true) => {
        self.volume_change = VolumeChange::Decrease;
      }
      _ => {}
    }
    self.prev_is_stopped = is_stopped;

    match self.volume_change {
      VolumeChange::Decrease => {
        self.attenuation_factor *= 0.999;
        if self.attenuation_factor < 0.0001 {
          self.attenuation_factor = 0.0;
          self.volume_change = VolumeChange::None;
        }
      }
      VolumeChange::None => {}
    }

    let mut output = [0.0f32; PORT_MAX_CHANNELS];

    if self.attenuation_factor < f32::EPSILON {
      return output;
    }

    let raw_output = processor.process_frame(num_channels);

    // Apply attenuation and soft clipping to all channels
    let mut any_audible = false;
    for ch in 0..num_channels.min(PORT_MAX_CHANNELS) {
      let sample = raw_output[ch] * self.attenuation_factor;
      output[ch] = safety_soft_clip(sample);
      if sample.abs() >= 0.0005 {
        any_audible = true;
      }
    }

    // When stopped and all channels are silent, fully mute
    if is_stopped && !any_audible {
      self.attenuation_factor = 0.0;
      self.volume_change = VolumeChange::None;
      return [0.0f32; PORT_MAX_CHANNELS];
    }

    output
  }
}

#[derive(Debug, Clone)]
#[napi(object)]
pub struct AudioBudgetSnapshot {
  pub total_samples: napi::bindgen_prelude::BigInt,
  pub total_time_ns: napi::bindgen_prelude::BigInt,

  /// Average nanoseconds per sample over snapshot window
  pub avg_ns_per_sample: f64,

  /// Average real-time usage (1.0 == real-time)
  pub avg_usage: f64,

  /// Worst-case nanoseconds per sample (peak density)
  pub peak_ns_per_sample: f64,

  /// Worst-case real-time usage (1.0 == real-time)
  pub peak_usage: f64,
}

#[derive(Debug, Default)]
pub struct AudioBudgetMeter {
  total_samples: AtomicU64,
  total_time_ns: AtomicU64,

  /// Q32 fixed-point: (ns / sample)
  max_ns_per_sample_q32: AtomicU64,
}

impl AudioBudgetMeter {
  pub const fn new() -> Self {
    Self {
      total_samples: AtomicU64::new(0),
      total_time_ns: AtomicU64::new(0),
      max_ns_per_sample_q32: AtomicU64::new(0),
    }
  }

  /// Call from audio callback
  #[inline(always)]
  pub fn record_chunk(&self, samples: u64, time_ns: u64) {
    if samples == 0 {
      return;
    }

    self.total_samples.fetch_add(samples, Ordering::Relaxed);
    self.total_time_ns.fetch_add(time_ns, Ordering::Relaxed);

    let ns_per_sample_q32 = (time_ns << 32) / samples;

    let mut prev = self.max_ns_per_sample_q32.load(Ordering::Relaxed);

    while ns_per_sample_q32 > prev {
      match self.max_ns_per_sample_q32.compare_exchange_weak(
        prev,
        ns_per_sample_q32,
        Ordering::Relaxed,
        Ordering::Relaxed,
      ) {
        Ok(_) => break,
        Err(v) => prev = v,
      }
    }
  }

  /// Call from non-audio thread
  pub fn take_snapshot(&self, sample_rate: f64, channels: f64) -> AudioBudgetSnapshot {
    let total_samples = self.total_samples.swap(0, Ordering::Relaxed);
    let total_time_ns = self.total_time_ns.swap(0, Ordering::Relaxed);
    let max_q32 = self.max_ns_per_sample_q32.swap(0, Ordering::Relaxed);

    let budget_ns_per_sample = 1e9 / (sample_rate * channels);

    let avg_ns_per_sample = if total_samples > 0 {
      total_time_ns as f64 / total_samples as f64
    } else {
      0.0
    };

    let peak_ns_per_sample = (max_q32 as f64) / (1u64 << 32) as f64;

    AudioBudgetSnapshot {
      total_samples: napi::bindgen_prelude::BigInt::from(total_samples),
      total_time_ns: napi::bindgen_prelude::BigInt::from(total_time_ns),

      avg_ns_per_sample,
      avg_usage: avg_ns_per_sample / budget_ns_per_sample,

      peak_ns_per_sample,
      peak_usage: peak_ns_per_sample / budget_ns_per_sample,
    }
  }
}

// ============================================================================
// TransportMeter - Lock-free transport state shared between threads
// ============================================================================

/// Lock-free transport state shared between audio thread and main thread.
/// Audio thread writes each frame, main thread reads for UI display.
#[derive(Debug)]
pub struct TransportMeter {
  /// Current bar phase (0..1), stored as f64 bits
  bar_phase_bits: AtomicU64,
  /// Completed bar count (0-indexed)
  bar_count: AtomicU64,
  /// Current beat within the bar (0-indexed)
  beat_in_bar: AtomicU64,
  /// Tempo in BPM, stored as f64 bits
  bpm_bits: AtomicU64,
  /// Time signature numerator (beats per bar)
  time_sig_numerator: AtomicU64,
  /// Time signature denominator (beat value)
  time_sig_denominator: AtomicU64,
  /// Whether the clock is running
  is_playing: AtomicBool,
  /// Whether a queued patch update is pending
  has_queued_update: AtomicBool,
  /// The update_id of the most recently applied patch update
  last_applied_update_id: AtomicU64,
  /// Whether Ableton Link is currently enabled
  link_enabled: AtomicBool,
  /// Number of Link peers in the session
  link_peers: AtomicU32,
  /// Free-running Link bar phase (0..1), always updated when Link is enabled
  link_phase_bits: AtomicU64,
  /// Armed for a quantized start — waiting for a Link bar boundary before
  /// playback actually begins. While true, `is_playing` is still false.
  link_pending_start: AtomicBool,
}

impl Default for TransportMeter {
  fn default() -> Self {
    Self {
      bar_phase_bits: AtomicU64::new(0f64.to_bits()),
      bar_count: AtomicU64::new(0),
      beat_in_bar: AtomicU64::new(0),
      bpm_bits: AtomicU64::new(120f64.to_bits()),
      time_sig_numerator: AtomicU64::new(4),
      time_sig_denominator: AtomicU64::new(4),
      is_playing: AtomicBool::new(false),
      has_queued_update: AtomicBool::new(false),
      last_applied_update_id: AtomicU64::new(0),
      link_enabled: AtomicBool::new(false),
      link_peers: AtomicU32::new(0),
      link_phase_bits: AtomicU64::new(0f64.to_bits()),
      link_pending_start: AtomicBool::new(false),
    }
  }
}

impl TransportMeter {
  /// Write transport state from the audio thread.
  /// Called once per frame after ROOT_CLOCK update.
  #[inline]
  pub fn write_from_audio(
    &self,
    bar_phase: f64,
    bar_count: u64,
    beat_in_bar: u32,
    is_playing: bool,
    has_queued_update: bool,
  ) {
    self
      .bar_phase_bits
      .store(bar_phase.to_bits(), Ordering::Relaxed);
    self.bar_count.store(bar_count, Ordering::Relaxed);
    self
      .beat_in_bar
      .store(beat_in_bar as u64, Ordering::Relaxed);
    self.is_playing.store(is_playing, Ordering::Relaxed);
    self
      .has_queued_update
      .store(has_queued_update, Ordering::Relaxed);
  }

  /// Write tempo and time signature. Called when params change (less frequently).
  #[inline]
  pub fn write_tempo(&self, bpm: f64, numerator: u32, denominator: u32) {
    self.bpm_bits.store(bpm.to_bits(), Ordering::Relaxed);
    self
      .time_sig_numerator
      .store(numerator as u64, Ordering::Relaxed);
    self
      .time_sig_denominator
      .store(denominator as u64, Ordering::Relaxed);
  }

  /// Record that the audio thread applied a patch update with this ID.
  #[inline]
  pub fn write_applied_update_id(&self, update_id: u64) {
    self
      .last_applied_update_id
      .store(update_id, Ordering::Relaxed);
  }

  /// Write just the BPM (e.g. when Link tempo changes externally).
  #[inline]
  pub fn write_bpm(&self, bpm: f64) {
    self.bpm_bits.store(bpm.to_bits(), Ordering::Relaxed);
  }

  /// Write Link enabled state and peer count (called from audio thread or main thread).
  #[inline]
  pub fn write_link_state(&self, enabled: bool, peers: u32) {
    self.link_enabled.store(enabled, Ordering::Relaxed);
    self.link_peers.store(peers, Ordering::Relaxed);
  }

  /// Write the free-running Link bar phase (0..1), always updated when Link is enabled.
  #[inline]
  pub fn write_link_phase(&self, phase: f64) {
    self
      .link_phase_bits
      .store(phase.to_bits(), Ordering::Relaxed);
  }

  /// Write the pending-start flag (armed, waiting for bar boundary).
  #[inline]
  pub fn write_link_pending_start(&self, armed: bool) {
    self.link_pending_start.store(armed, Ordering::Relaxed);
  }

  /// Read the current Link enabled flag (used by the main thread for
  /// idempotency checks before constructing/destroying Link resources).
  #[inline]
  pub fn read_link_enabled(&self) -> bool {
    self.link_enabled.load(Ordering::Relaxed)
  }

  /// Read the current BPM (used by the main thread when constructing a new
  /// `AblLink` so we initialise the session at the user's last known tempo
  /// rather than always 120).
  #[inline]
  pub fn read_bpm(&self) -> f64 {
    f64::from_bits(self.bpm_bits.load(Ordering::Relaxed))
  }

  /// Read transport snapshot from the main thread.
  pub fn snapshot(&self) -> TransportSnapshot {
    TransportSnapshot {
      bar_phase: f64::from_bits(self.bar_phase_bits.load(Ordering::Relaxed)),
      bar: self.bar_count.load(Ordering::Relaxed) as i64,
      beat_in_bar: self.beat_in_bar.load(Ordering::Relaxed) as u32,
      bpm: f64::from_bits(self.bpm_bits.load(Ordering::Relaxed)),
      time_sig_numerator: self.time_sig_numerator.load(Ordering::Relaxed) as u32,
      time_sig_denominator: self.time_sig_denominator.load(Ordering::Relaxed) as u32,
      is_playing: self.is_playing.load(Ordering::Relaxed),
      has_queued_update: self.has_queued_update.load(Ordering::Relaxed),
      last_applied_update_id: self.last_applied_update_id.load(Ordering::Relaxed) as f64,
      link_enabled: self.link_enabled.load(Ordering::Relaxed),
      link_peers: self.link_peers.load(Ordering::Relaxed),
      link_phase: f64::from_bits(self.link_phase_bits.load(Ordering::Relaxed)),
      link_pending_start: self.link_pending_start.load(Ordering::Relaxed),
    }
  }
}

#[napi(object)]
#[derive(Debug, Clone)]
pub struct TransportSnapshot {
  /// Current bar phase (0..1 over one bar)
  pub bar_phase: f64,
  /// Completed bar count (0-indexed; display as bar + 1)
  pub bar: i64,
  /// Current beat within the bar (0-indexed)
  pub beat_in_bar: u32,
  /// Tempo in BPM
  pub bpm: f64,
  /// Time signature numerator (beats per bar)
  pub time_sig_numerator: u32,
  /// Time signature denominator (beat value)
  pub time_sig_denominator: u32,
  /// Whether the clock is running
  pub is_playing: bool,
  /// Whether a queued patch update is pending
  pub has_queued_update: bool,
  /// The update_id of the most recently applied patch update (as f64 for N-API compatibility)
  pub last_applied_update_id: f64,
  /// Whether Ableton Link is currently enabled
  pub link_enabled: bool,
  /// Number of Link peers in the session
  pub link_peers: u32,
  /// Free-running Link bar phase (0..1), always updated when Link is enabled
  pub link_phase: f64,
  /// Armed for a quantized start — a start has been requested and the audio
  /// thread is waiting for the next Link bar boundary before actually
  /// flipping `is_playing`. Only meaningful when `link_enabled` is true.
  pub link_pending_start: bool,
}

#[cfg(test)]
mod tests {
  use super::*;
  use modular_core::Signal;
  use modular_core::poly::PolyOutput;
  use modular_core::types::ModuleIdRemap;
  use modular_core::types::{Message, MessageHandler, MessageTag, MidiNoteOn};
  use std::sync::atomic::AtomicUsize;

  // ============================================================================
  // AudioProcessor + command queue tests
  // ============================================================================

  #[test]
  fn test_stopped_state_via_shared_state() {
    // Test the shared stopped atomic directly
    let stopped = Arc::new(AtomicBool::new(true));

    // Initially stopped
    assert!(stopped.load(Ordering::Acquire));
    stopped.store(false, Ordering::Release);
    assert!(!stopped.load(Ordering::Acquire));
    stopped.store(true, Ordering::Release);
    assert!(stopped.load(Ordering::Acquire));
  }

  #[test]
  fn test_audio_processor_owns_patch() {
    // Verify AudioProcessor can be created and owns patch exclusively
    let (
      cmd_producer,
      cmd_consumer,
      err_producer,
      _err_consumer,
      garbage_producer,
      _garbage_consumer,
    ) = create_audio_channels();

    // Drop the command producer since we won't use it in this test
    drop(cmd_producer);

    let shared = AudioSharedState {
      stopped: Arc::new(AtomicBool::new(true)),
      scope_collection: Arc::new(Mutex::new(HashMap::new())),
      recording_writer: Arc::new(Mutex::new(None)),
      audio_budget_meter: Arc::new(AudioBudgetMeter::new()),
      module_states: Arc::new(Mutex::new(HashMap::new())),
      midi_manager: Arc::new(MidiInputManager::new()),
      transport_meter: Arc::new(TransportMeter::default()),
    };

    let processor = AudioProcessor::new(
      cmd_consumer,
      err_producer,
      garbage_producer,
      shared,
      44100.0,
    );

    // Processor starts with empty patch (may have hidden audio_in)
    assert!(processor.patch.sampleables.is_empty() || processor.patch.sampleables.len() == 1);
  }

  // A minimal mock Sampleable for testing remaps. Each instance encodes
  // its identity in `get_module_type()` so we can verify which concrete
  // module ends up at which map slot after remapping.
  struct MockModule {
    /// Identity label returned by `get_module_type()` for assertions.
    label: String,
    /// The "current" ID used by the Sampleable interface.
    current_id: String,
  }

  impl MockModule {
    fn new(label: &str) -> Box<dyn modular_core::types::Sampleable> {
      Box::new(Self {
        label: label.to_string(),
        current_id: label.to_string(),
      })
    }
  }

  impl modular_core::types::MessageHandler for MockModule {}

  impl modular_core::types::Sampleable for MockModule {
    fn get_id(&self) -> &str {
      &self.current_id
    }
    fn tick(&self) {}
    fn update(&self) {}
    fn get_poly_sample(&self, _port: &str) -> napi::Result<modular_core::poly::PolyOutput> {
      Ok(modular_core::poly::PolyOutput::default())
    }
    fn get_module_type(&self) -> &str {
      &self.label
    }
    fn connect(&self, _patch: &modular_core::patch::Patch) {}
    fn as_any(&self) -> &dyn std::any::Any {
      self
    }
  }

  struct CountingMessageModule {
    current_id: String,
    label: String,
    hits: Arc<AtomicUsize>,
  }

  impl CountingMessageModule {
    fn new(label: &str, hits: Arc<AtomicUsize>) -> Box<dyn modular_core::types::Sampleable> {
      Box::new(Self {
        current_id: label.to_string(),
        label: label.to_string(),
        hits,
      })
    }
  }

  impl MessageHandler for CountingMessageModule {
    fn handled_message_tags(&self) -> &'static [MessageTag] {
      &[MessageTag::MidiNoteOn]
    }

    fn handle_message(&self, _message: &Message) -> napi::Result<()> {
      self.hits.fetch_add(1, Ordering::SeqCst);
      Ok(())
    }
  }

  impl modular_core::types::Sampleable for CountingMessageModule {
    fn get_id(&self) -> &str {
      &self.current_id
    }
    fn tick(&self) {}
    fn update(&self) {}
    fn get_poly_sample(&self, _port: &str) -> napi::Result<modular_core::poly::PolyOutput> {
      Ok(modular_core::poly::PolyOutput::default())
    }
    fn get_module_type(&self) -> &str {
      &self.label
    }
    fn connect(&self, _patch: &modular_core::patch::Patch) {}
    fn as_any(&self) -> &dyn std::any::Any {
      self
    }
  }

  struct ConstantOutputModule {
    current_id: String,
    value: f32,
  }

  impl ConstantOutputModule {
    fn new(id: &str, value: f32) -> Box<dyn modular_core::types::Sampleable> {
      Box::new(Self {
        current_id: id.to_string(),
        value,
      })
    }
  }

  impl MessageHandler for ConstantOutputModule {}

  impl modular_core::types::Sampleable for ConstantOutputModule {
    fn get_id(&self) -> &str {
      &self.current_id
    }
    fn tick(&self) {}
    fn update(&self) {}
    fn get_poly_sample(&self, _port: &str) -> napi::Result<PolyOutput> {
      Ok(PolyOutput::mono(self.value))
    }
    fn get_module_type(&self) -> &str {
      "constant-output"
    }
    fn connect(&self, _patch: &modular_core::patch::Patch) {}
    fn as_any(&self) -> &dyn std::any::Any {
      self
    }
  }

  struct PatchUpdateSensitiveModule {
    current_id: String,
    params_signal: Mutex<Signal>,
    cached_signal: Mutex<Signal>,
  }

  impl PatchUpdateSensitiveModule {
    fn new(id: &str, signal: Signal) -> Box<dyn modular_core::types::Sampleable> {
      Box::new(Self {
        current_id: id.to_string(),
        params_signal: Mutex::new(signal),
        cached_signal: Mutex::new(Signal::Volts(0.0)),
      })
    }
  }

  impl MessageHandler for PatchUpdateSensitiveModule {}

  impl modular_core::types::Sampleable for PatchUpdateSensitiveModule {
    fn get_id(&self) -> &str {
      &self.current_id
    }
    fn tick(&self) {}
    fn update(&self) {}
    fn get_poly_sample(&self, _port: &str) -> napi::Result<PolyOutput> {
      Ok(PolyOutput::mono(self.cached_signal.lock().get_value()))
    }
    fn get_module_type(&self) -> &str {
      "patch-update-sensitive"
    }
    fn connect(&self, patch: &modular_core::patch::Patch) {
      modular_core::types::Connect::connect(&mut *self.params_signal.lock(), patch);
    }
    fn on_patch_update(&self) {
      let signal = self.params_signal.lock().clone();
      *self.cached_signal.lock() = signal;
    }
    fn as_any(&self) -> &dyn std::any::Any {
      self
    }
  }

  fn create_test_processor() -> (CommandProducer, AudioProcessor) {
    let (
      cmd_producer,
      cmd_consumer,
      err_producer,
      _err_consumer,
      garbage_producer,
      _garbage_consumer,
    ) = create_audio_channels();

    let shared = AudioSharedState {
      stopped: Arc::new(AtomicBool::new(true)),
      scope_collection: Arc::new(Mutex::new(HashMap::new())),
      recording_writer: Arc::new(Mutex::new(None)),
      audio_budget_meter: Arc::new(AudioBudgetMeter::new()),
      module_states: Arc::new(Mutex::new(HashMap::new())),
      midi_manager: Arc::new(MidiInputManager::new()),
      transport_meter: Arc::new(TransportMeter::default()),
    };

    let processor = AudioProcessor::new(
      cmd_consumer,
      err_producer,
      garbage_producer,
      shared,
      44100.0,
    );

    (cmd_producer, processor)
  }

  #[test]
  fn test_remap_chain_preserves_modules() {
    // Regression test: when remaps form a chain (A→B, B→C), all modules
    // must survive. Before the fix, applying A→B first would overwrite B
    // (destroying it) before B→C could move it to C.
    let (_cmd_producer, mut processor) = create_test_processor();

    // Set up initial state: cycle-2 (shift), cycle-3 (thirds)
    processor
      .patch
      .sampleables
      .insert("cycle-2".into(), MockModule::new("shift"));
    processor
      .patch
      .sampleables
      .insert("cycle-3".into(), MockModule::new("thirds"));

    // Remap chain: cycle-2→cycle-3, cycle-3→cycle-4
    let mut update = PatchUpdate::new(48000.0);
    update.remaps = vec![
      ModuleIdRemap {
        from: "cycle-2".into(),
        to: "cycle-3".into(),
      },
      ModuleIdRemap {
        from: "cycle-3".into(),
        to: "cycle-4".into(),
      },
    ];
    // Both new IDs are desired (won't be garbage-collected)
    update.desired_ids.insert("cycle-3".into());
    update.desired_ids.insert("cycle-4".into());

    processor.apply_patch_update(update);

    // Verify: "shift" module should now be at cycle-3, "thirds" at cycle-4
    assert!(
      processor.patch.sampleables.contains_key("cycle-3"),
      "cycle-3 should exist after remap"
    );
    assert!(
      processor.patch.sampleables.contains_key("cycle-4"),
      "cycle-4 should exist after remap"
    );
    assert_eq!(
      processor
        .patch
        .sampleables
        .get("cycle-3")
        .unwrap()
        .get_module_type(),
      "shift",
      "cycle-3 should contain the shift module"
    );
    assert_eq!(
      processor
        .patch
        .sampleables
        .get("cycle-4")
        .unwrap()
        .get_module_type(),
      "thirds",
      "cycle-4 should contain the thirds module"
    );
    // cycle-2 should have been removed (not in desired_ids)
    assert!(
      !processor.patch.sampleables.contains_key("cycle-2"),
      "cycle-2 should be removed (no longer desired)"
    );
  }

  #[test]
  fn test_remap_swap_preserves_both_modules() {
    // Test swap: A→B and B→A simultaneously
    let (_cmd_producer, mut processor) = create_test_processor();

    processor
      .patch
      .sampleables
      .insert("osc-1".into(), MockModule::new("alpha"));
    processor
      .patch
      .sampleables
      .insert("osc-2".into(), MockModule::new("beta"));

    let mut update = PatchUpdate::new(48000.0);
    update.remaps = vec![
      ModuleIdRemap {
        from: "osc-1".into(),
        to: "osc-2".into(),
      },
      ModuleIdRemap {
        from: "osc-2".into(),
        to: "osc-1".into(),
      },
    ];
    update.desired_ids.insert("osc-1".into());
    update.desired_ids.insert("osc-2".into());

    processor.apply_patch_update(update);

    assert_eq!(
      processor
        .patch
        .sampleables
        .get("osc-1")
        .unwrap()
        .get_module_type(),
      "beta",
      "osc-1 should now contain beta (swapped)"
    );
    assert_eq!(
      processor
        .patch
        .sampleables
        .get("osc-2")
        .unwrap()
        .get_module_type(),
      "alpha",
      "osc-2 should now contain alpha (swapped)"
    );
  }

  #[test]
  fn test_remap_simple_rename() {
    // Simple case: single remap, no chain
    let (_cmd_producer, mut processor) = create_test_processor();

    processor
      .patch
      .sampleables
      .insert("vca-1".into(), MockModule::new("my-vca"));

    let mut update = PatchUpdate::new(48000.0);
    update.remaps = vec![ModuleIdRemap {
      from: "vca-1".into(),
      to: "vca-2".into(),
    }];
    update.desired_ids.insert("vca-2".into());

    processor.apply_patch_update(update);

    assert!(
      !processor.patch.sampleables.contains_key("vca-1"),
      "old ID should be gone"
    );
    assert_eq!(
      processor
        .patch
        .sampleables
        .get("vca-2")
        .unwrap()
        .get_module_type(),
      "my-vca",
      "module should be at new ID"
    );
  }

  #[test]
  fn test_single_module_update_re_registers_message_listeners() {
    let (mut cmd_producer, mut processor) = create_test_processor();

    let old_hits = Arc::new(AtomicUsize::new(0));
    let new_hits = Arc::new(AtomicUsize::new(0));
    processor.patch.sampleables.insert(
      "m1".into(),
      CountingMessageModule::new("old", Arc::clone(&old_hits)),
    );
    processor.patch.rebuild_message_listeners();

    cmd_producer
      .push(GraphCommand::SingleModuleUpdate {
        module_id: "m1".into(),
        module: CountingMessageModule::new("new", Arc::clone(&new_hits)),
      })
      .unwrap();
    cmd_producer
      .push(GraphCommand::DispatchMessage(Message::MidiNoteOn(
        MidiNoteOn {
          device: None,
          channel: 0,
          note: 60,
          velocity: 100,
        },
      )))
      .unwrap();

    processor.process_commands();

    assert_eq!(old_hits.load(Ordering::SeqCst), 0);
    assert_eq!(new_hits.load(Ordering::SeqCst), 1);
  }

  #[test]
  fn test_single_module_update_refreshes_patch_update_caches() {
    let (mut cmd_producer, mut processor) = create_test_processor();

    processor
      .patch
      .sampleables
      .insert("src".into(), ConstantOutputModule::new("src", 3.5));
    processor.patch.sampleables.insert(
      "dep".into(),
      PatchUpdateSensitiveModule::new(
        "dep",
        Signal::Cable {
          module: "src".into(),
          resolved: None,
          port: "out".into(),
          channel: 0,
        },
      ),
    );

    for module in processor.patch.sampleables.values() {
      module.connect(&processor.patch);
    }
    for module in processor.patch.sampleables.values() {
      module.on_patch_update();
    }

    let initial = processor
      .patch
      .sampleables
      .get("dep")
      .unwrap()
      .get_poly_sample("out")
      .unwrap()
      .get(0);
    assert_eq!(initial, 3.5);

    cmd_producer
      .push(GraphCommand::SingleModuleUpdate {
        module_id: "src".into(),
        module: ConstantOutputModule::new("src", 7.25),
      })
      .unwrap();

    processor.process_commands();

    let updated = processor
      .patch
      .sampleables
      .get("dep")
      .unwrap()
      .get_poly_sample("out")
      .unwrap()
      .get(0);
    assert_eq!(updated, 7.25);
  }

  #[test]
  fn test_patch_update_remap_re_registers_message_listeners() {
    let (_cmd_producer, mut processor) = create_test_processor();

    let hits = Arc::new(AtomicUsize::new(0));
    processor.patch.sampleables.insert(
      "old-id".into(),
      CountingMessageModule::new("old-id", Arc::clone(&hits)),
    );
    processor.patch.rebuild_message_listeners();

    let mut update = PatchUpdate::new(48000.0);
    update.remaps = vec![ModuleIdRemap {
      from: "old-id".into(),
      to: "new-id".into(),
    }];
    update.desired_ids.insert("new-id".into());

    processor.apply_patch_update(update);

    let message = Message::MidiNoteOn(MidiNoteOn {
      device: None,
      channel: 0,
      note: 60,
      velocity: 100,
    });

    processor.patch.dispatch_message(&message).unwrap();

    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(!processor.patch.sampleables.contains_key("old-id"));
    assert!(processor.patch.sampleables.contains_key("new-id"));
  }

  // ============================================================================
  // Safety soft clip tests
  // ============================================================================

  #[test]
  fn test_safety_soft_clip_linear_below_knee() {
    for &val in &[0.0, 0.1, -0.1, 0.5, -0.5, 0.89, -0.89, 0.9, -0.9] {
      assert_eq!(
        safety_soft_clip(val),
        val,
        "expected linear passthrough for {val}"
      );
    }
  }

  #[test]
  fn test_safety_soft_clip_saturates_above_knee() {
    for &val in &[1.0, 2.0, 5.0, 10.0, 100.0] {
      let out = safety_soft_clip(val);
      assert!(
        out > SAFETY_CLIP_KNEE,
        "output {out} should be above knee for input {val}"
      );
      assert!(
        out < 1.0,
        "output {out} should be below 1.0 for input {val}"
      );
    }
    for &val in &[-1.0, -2.0, -5.0, -10.0, -100.0] {
      let out = safety_soft_clip(val);
      assert!(
        out < -SAFETY_CLIP_KNEE,
        "output {out} should be below -knee for input {val}"
      );
      assert!(
        out > -1.0,
        "output {out} should be above -1.0 for input {val}"
      );
    }
  }

  #[test]
  fn test_safety_soft_clip_monotonic() {
    let mut prev = safety_soft_clip(-100.0);
    let mut x = -100.0;
    while x <= 100.0 {
      let out = safety_soft_clip(x);
      assert!(out >= prev, "not monotonic at {x}: {prev} -> {out}");
      prev = out;
      x += 0.1;
    }
  }

  #[test]
  fn test_safety_soft_clip_nan_inf() {
    assert_eq!(safety_soft_clip(f32::NAN), 0.0);
    assert_eq!(safety_soft_clip(f32::INFINITY), 0.0);
    assert_eq!(safety_soft_clip(f32::NEG_INFINITY), 0.0);
  }
}
