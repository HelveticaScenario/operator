use cpal::FromSample;
use cpal::Host;
use cpal::HostId;
use cpal::Sample;
use cpal::SizedSample;
use cpal::traits::{DeviceTrait, HostTrait};

use modular_core::PORT_MAX_CHANNELS;
use modular_core::PatchGraph;
use modular_core::dsp::schema;
use modular_core::dsp::utils::SchmittTrigger;
use modular_core::profiling::{ModuleProfileAccum, ModuleProfileCollection};

use modular_core::types::ClockMessages;
use modular_core::types::Message;
use modular_core::types::ScopeMode;
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
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};

use modular_core::module_state::{ModuleLiveState, ModuleStateMeta};
use modular_core::patch::Patch;
use modular_core::types::{ROOT_OUTPUT_PORT, ScopeBufferKey, ScopeXyBufferKey, ScopeXyRanges};
use std::time::Instant;

/// Shared map of live per-module editor state, keyed by module id. Each value is
/// a module's pre-allocated, type-erased live slot (see
/// [`modular_core::module_state`]). The audio thread writes into existing slots
/// (without allocating); the main thread reads them on poll and is the only side
/// that adds or removes keys (in `apply_patch`).
type ModuleStateMap = HashMap<String, Box<dyn ModuleLiveState>>;

/// Main-thread-only cache of immutable per-module state metadata that doesn't
/// change while playing. Held behind a single lock (see
/// [`AudioState::module_state_meta`]) only because `AudioState` is shared by
/// `Arc` and so must be `Sync`; the audio thread never touches it.
#[derive(Default)]
struct ModuleStateMetaCache {
    /// Metadata for the patch currently playing on the audio thread, keyed by
    /// module id. Paired with the live slots in `get_module_states`.
    live: HashMap<String, Box<dyn ModuleStateMeta>>,
    /// Metadata for patch updates sent to the audio thread but not yet swapped
    /// in, each tagged with its `update_id`, in submission (ascending-id)
    /// order. Multiple entries coexist because a superseded update can still
    /// swap in first — its quantized trigger may fire before the superseding
    /// command is popped. Once the audio thread reports an applied id, the
    /// newest entry at or below it promotes into `live`, so a poll never pairs
    /// one patch's metadata with another patch's still-live state.
    pending: Vec<(u64, HashMap<String, Box<dyn ModuleStateMeta>>)>,
}

impl ModuleStateMetaCache {
    /// Promote pending metadata and prune dropped modules' slots once the swap
    /// applied. Called from the poll path and `apply_patch` so orphan slots can't
    /// accumulate while the editor isn't polling.
    fn promote_if_applied(&mut self, states: &mut ModuleStateMap, applied_update_id: u64) {
        // Entries are id-ascending, so the applied ones are exactly a prefix.
        // Only the newest applied entry promotes: an older one was superseded
        // and its patch is no longer the one playing.
        let applied = self
            .pending
            .iter()
            .take_while(|(id, _)| *id <= applied_update_id)
            .count();
        let Some((_, metas)) = self.pending.drain(..applied).last() else {
            return;
        };
        // Keep any slot a still-pending update pre-added — its patch may swap
        // in next and the audio thread only ever writes into existing slots.
        states.retain(|id, _| {
            metas.contains_key(id) || self.pending.iter().any(|(_, m)| m.contains_key(id))
        });
        self.live = metas;
    }
}

use crate::commands::{
    AudioError, COMMAND_QUEUE_CAPACITY, ERROR_QUEUE_CAPACITY, GARBAGE_QUEUE_CAPACITY, GarbageItem,
    GraphCommand, PatchUpdate, QueuedTrigger, TransportMeta,
};
use crate::midi::MidiInputManager;
use crate::recording::{RecordingFeed, RecordingSession};

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
                self.output_devices
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
        self.output_devices
            .values()
            .flatten()
            .find(|d| d.id == device_id)
    }

    /// Find an input device by ID
    pub fn find_input_device(&self, device_id: &str) -> Option<&AudioDeviceInfo> {
        self.input_devices
            .values()
            .flatten()
            .find(|d| d.id == device_id)
    }

    /// Get output devices for a specific host
    pub fn output_devices_for_host(&self, host_id: &str) -> Vec<AudioDeviceInfo> {
        self.output_devices
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

    host.devices()
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

    host.input_devices()
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
    host.output_devices()
        .ok()?
        .find(|d| d.id().ok() == cpal::DeviceId::from_str(id).ok())
}

/// Find an input device by id
pub fn find_input_device(id: &str) -> Option<cpal::Device> {
    let host = get_host_by_preference();
    host.input_devices()
        .ok()?
        .find(|d| d.id().ok() == cpal::DeviceId::from_str(id).ok())
}

/// Find an output device by id in a specific host
pub fn find_output_device_in_host(host: &Host, id: &str) -> Option<cpal::Device> {
    host.output_devices()
        .ok()?
        .find(|d| d.id().ok() == cpal::DeviceId::from_str(id).ok())
}

/// Find an input device by id in a specific host
pub fn find_input_device_in_host(host: &Host, id: &str) -> Option<cpal::Device> {
    host.input_devices()
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
        let clipped =
            SAFETY_CLIP_KNEE + SAFETY_CLIP_HEADROOM * (excess / SAFETY_CLIP_HEADROOM).tanh();
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

/// Sample-rate ring buffer for the $scopeXY visualizer.
///
/// One buffer per (xChannel, yChannel) pair. The audio callback pushes
/// (x, y) pairs at full sample rate — no decimation, no trigger, no
/// double-buffering — so the renderer can draw a woscope-style XY beam
/// over the most recent CAPACITY samples (~43 ms at 48 kHz).
pub const SCOPE_XY_CAPACITY: usize = 2048;

/// Audio-thread-private write ring. Touched only by the audio thread, only
/// through `&self` (see the `UnsafeCell` in `ScopeXyBuffer`). The main thread
/// never reads it.
struct ScopeXyPrivate {
    x: [f32; SCOPE_XY_CAPACITY],
    y: [f32; SCOPE_XY_CAPACITY],
    write_idx: usize,
    /// Samples written so far, saturating at CAPACITY. Lets `snapshot` return
    /// only valid samples before the ring first fills (no leading zeros).
    filled: usize,
}

/// Lock-free single-producer XY scope buffer.
///
/// The audio thread appends samples to the private ring every frame, then
/// publishes the whole ring into a SeqLock-guarded region once per callback.
/// The main thread reads the published region without taking any lock,
/// retrying only across the microsecond window of an in-flight publish. This
/// keeps the audio thread off any lock on the per-sample path and lets the
/// renderer pick up a coherent frame on every poll.
pub struct ScopeXyBuffer {
    private: UnsafeCell<ScopeXyPrivate>,
    /// SeqLock sequence: even = stable, odd = publish in flight.
    seq: AtomicU32,
    pub_x: UnsafeCell<[f32; SCOPE_XY_CAPACITY]>,
    pub_y: UnsafeCell<[f32; SCOPE_XY_CAPACITY]>,
    pub_head: AtomicU32,
    /// Published count of valid samples (saturates at CAPACITY).
    pub_filled: AtomicU32,
}

// SAFETY: the audio thread is the sole writer of both the private ring and the
// published region. The main thread only reads the published region, gated by
// the SeqLock retry protocol in `snapshot`. No other path aliases the
// `UnsafeCell` contents, so concurrent access is data-race free.
unsafe impl Sync for ScopeXyBuffer {}

impl ScopeXyBuffer {
    pub fn new() -> Self {
        Self {
            private: UnsafeCell::new(ScopeXyPrivate {
                x: [0.0; SCOPE_XY_CAPACITY],
                y: [0.0; SCOPE_XY_CAPACITY],
                write_idx: 0,
                filled: 0,
            }),
            seq: AtomicU32::new(0),
            pub_x: UnsafeCell::new([0.0; SCOPE_XY_CAPACITY]),
            pub_y: UnsafeCell::new([0.0; SCOPE_XY_CAPACITY]),
            pub_head: AtomicU32::new(0),
            pub_filled: AtomicU32::new(0),
        }
    }

    /// Append one sample pair to the private ring. Audio thread only.
    #[inline]
    pub fn push(&self, xv: f32, yv: f32) {
        // SAFETY: the audio thread is the only accessor of `private`.
        let p = unsafe { &mut *self.private.get() };
        p.x[p.write_idx] = xv;
        p.y[p.write_idx] = yv;
        p.write_idx = (p.write_idx + 1) % SCOPE_XY_CAPACITY;
        if p.filled < SCOPE_XY_CAPACITY {
            p.filled += 1;
        }
    }

    /// Copy the private ring into the published region under the SeqLock. Audio
    /// thread only, once per callback. Allocation-free.
    pub fn publish(&self) {
        // SAFETY: audio thread is the sole writer; the odd sequence value below
        // tells a concurrent reader its copy is mid-update and must be retried.
        let p = unsafe { &*self.private.get() };
        // Mark the publish in flight (odd), then a release fence so the data
        // writes below cannot be reordered ahead of the marker. A plain Release on
        // the increment would only bound prior accesses, not the following copy;
        // the fence is what guarantees a reader observing even→even saw a complete
        // frame. The closing Release increment bounds the data writes from below.
        self.seq.fetch_add(1, Ordering::Relaxed); // -> odd: publish in flight
        std::sync::atomic::fence(Ordering::Release);
        unsafe {
            (*self.pub_x.get()).copy_from_slice(&p.x);
            (*self.pub_y.get()).copy_from_slice(&p.y);
        }
        self.pub_head.store(p.write_idx as u32, Ordering::Relaxed);
        self.pub_filled.store(p.filled as u32, Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Release); // -> even: stable
    }

    /// Read the published ring on the main thread, retrying while a publish is
    /// in flight. Returns (x, y) in chronological order: element 0 is the oldest
    /// sample of the window, the last element the newest. Before the ring first
    /// fills, only the samples written so far are returned. Vec allocation is
    /// safe here (main thread).
    pub fn snapshot(&self) -> (Float32Array, Float32Array) {
        let mut x = vec![0.0f32; SCOPE_XY_CAPACITY];
        let mut y = vec![0.0f32; SCOPE_XY_CAPACITY];
        let (head, filled) = loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            // SAFETY: shared read of the published region. A publish concurrent
            // with this copy bumps `seq`, so the s1 == s2 check below discards the
            // torn copy and retries.
            unsafe {
                x.copy_from_slice(&*self.pub_x.get());
                y.copy_from_slice(&*self.pub_y.get());
            }
            let h = self.pub_head.load(Ordering::Relaxed);
            let f = self.pub_filled.load(Ordering::Relaxed);
            // Acquire fence so the data reads above complete before the second
            // sequence load; pairs with the writer's release fence so a publish that
            // starts mid-read is reliably caught by the s1 == s2 check below.
            std::sync::atomic::fence(Ordering::Acquire);
            let s2 = self.seq.load(Ordering::Relaxed);
            if s1 == s2 {
                break (h as usize, f as usize);
            }
        };
        if filled >= SCOPE_XY_CAPACITY {
            // Full ring: `head` (write_idx) points at the oldest slot; rotate it to
            // the front so element 0 is the oldest sample.
            x.rotate_left(head);
            y.rotate_left(head);
        } else {
            // Partially filled: slots 0..filled are already chronological (no
            // wraparound yet); drop the zero-initialized tail so the renderer never
            // draws a phantom stroke through the leading zeros.
            x.truncate(filled);
            y.truncate(filled);
        }
        (Float32Array::new(x), Float32Array::new(y))
    }
}

impl Default for ScopeXyBuffer {
    fn default() -> Self {
        Self::new()
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
    /// XY scope collection - shared with audio thread for UI reads.
    /// Replaced wholesale (single global $scopeXY); each pair owns its own ring buffer.
    scope_xy_collection: Arc<Mutex<HashMap<ScopeXyBufferKey, Arc<ScopeXyBuffer>>>>,
    /// Display ranges for the active $scopeXY (global, last-call-wins). Written by
    /// the audio thread in `apply_patch_update` (atomically with the XY buffer
    /// swap, so the window updates exactly when the patch applies);
    /// read on the main thread by `get_scope_xy_buffers` to ship the
    /// volt→clip window. Shared with the audio thread via `AudioSharedState`.
    scope_xy_ranges: Arc<Mutex<Option<ScopeXyRanges>>>,
    /// Audio-thread half of the active recording session, shared with the
    /// callback: a lock-free ring the callback pushes f32 samples into. All
    /// disk I/O happens on the session's writer thread.
    recording_feed: Arc<Mutex<Option<RecordingFeed>>>,
    /// Main-thread handle to the active recording session's writer thread.
    recording_session: Mutex<Option<RecordingSession>>,
    /// Sample rate
    sample_rate: f32,
    /// Output channels
    channels: u16,
    /// Audio budget meter - written by audio thread, read by main thread
    audio_budget_meter: Arc<AudioBudgetMeter>,
    /// Live per-module editor state, written by the audio thread and read by the
    /// main thread. The audio thread only writes into existing slots; the main
    /// thread owns adding and removing keys.
    module_states: Arc<Mutex<ModuleStateMap>>,
    /// Main-thread cache of per-module state metadata (live + pending), used to
    /// build the editor JSON in `get_module_states`. Single lock for `&self`
    /// mutation; the audio thread never touches it. See [`ModuleStateMetaCache`].
    module_state_meta: Mutex<ModuleStateMetaCache>,
    /// The last successfully built editor-state JSON, served when a poll loses
    /// the `module_states` try_lock race with the audio thread — so contention
    /// reads as an unchanged poll, never as "all modules removed".
    module_states_snapshot: Mutex<HashMap<String, serde_json::Value>>,
    /// MIDI input manager - shared with audio thread for polling
    midi_manager: Arc<MidiInputManager>,
    /// Transport state meter - written by audio thread, read by main thread
    pub transport_meter: Arc<TransportMeter>,
    /// Set true if the audio callback caught an unwinding panic. Once set the
    /// callback writes silence forever; the stream must be torn down and the
    /// Synthesizer recreated to recover.
    pub audio_thread_panicked: Arc<AtomicBool>,
    /// Internal block size used when constructing modules. Today the audio
    /// callback drives the inner loop at `block_size=1` regardless of the
    /// CPAL buffer size; the block-aware callback rewrite lands separately.
    /// Plumbed through `make_stream` so the audio thread can be flipped to
    /// a real block size without touching every constructor call site.
    pub block_size: usize,
    /// Per-module profile snapshot. Audio thread writes via try_lock at
    /// callback end (see `modular_core::profiling::flush_into`); main thread
    /// drains via `get_module_profile`.
    module_profile_collection: ModuleProfileCollection,
    /// Refcount of enable requests. The underlying profiling global is on
    /// iff this is > 0. Lets multiple consumers (UI panel, future telemetry
    /// hooks) coexist without one clobbering another's enable state.
    module_profiling_enable_count: Arc<AtomicU32>,
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
        block_size: usize,
    ) -> Self {
        Self {
            command_tx: Mutex::new(command_tx),
            error_rx: Mutex::new(error_rx),
            garbage_rx: Mutex::new(garbage_rx),
            stopped: Arc::new(AtomicBool::new(true)),
            scope_collection: Arc::new(Mutex::new(HashMap::new())),
            scope_xy_collection: Arc::new(Mutex::new(HashMap::new())),
            scope_xy_ranges: Arc::new(Mutex::new(None)),
            recording_feed: Arc::new(Mutex::new(None)),
            recording_session: Mutex::new(None),
            sample_rate,
            channels,
            audio_budget_meter: Arc::new(AudioBudgetMeter::default()),
            module_states: Arc::new(Mutex::new(HashMap::new())),
            module_state_meta: Mutex::new(ModuleStateMetaCache::default()),
            module_states_snapshot: Mutex::new(HashMap::new()),
            midi_manager,
            transport_meter: Arc::new(TransportMeter::default()),
            audio_thread_panicked: Arc::new(AtomicBool::new(false)),
            block_size,
            module_profile_collection: modular_core::profiling::new_collection(),
            module_profiling_enable_count: Arc::new(AtomicU32::new(0)),
        }
    }

    /// True if the audio callback has caught a panic and is now emitting silence.
    /// Recovery requires recreating the Synthesizer.
    pub fn is_audio_thread_panicked(&self) -> bool {
        self.audio_thread_panicked.load(Ordering::SeqCst)
    }

    /// Send a command to the audio thread
    pub(crate) fn send_command(&self, cmd: GraphCommand) -> Result<()> {
        let mut tx = self.command_tx.lock();
        tx.push(cmd).map_err(|_| {
            napi::Error::from_reason(
                "Command queue full - audio thread may be overloaded".to_string(),
            )
        })
    }

    /// Drain deferred deallocations from the audio thread.
    /// Items are simply dropped on the main thread where allocation/deallocation is safe.
    /// Also drains the error queue: an error's payload can carry a garbage item
    /// that overflowed the garbage queue, so dropping it here (with a log line)
    /// is part of the same off-audio-thread deallocation contract.
    pub fn drain_garbage(&self) {
        let mut rx = self.garbage_rx.lock();
        while let Ok(_item) = rx.pop() {
            // Item is dropped here on the main thread - this is the whole point
        }
        drop(rx);
        let mut errors = self.error_rx.lock();
        while let Ok(err) = errors.pop() {
            eprintln!("[audio] {}", err);
        }
    }

    pub fn take_audio_thread_budget_snapshot_and_reset(&self) -> AudioBudgetSnapshot {
        self.audio_budget_meter
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
            scope_xy_collection: self.scope_xy_collection.clone(),
            scope_xy_ranges: self.scope_xy_ranges.clone(),
            recording_feed: self.recording_feed.clone(),
            audio_budget_meter: self.audio_budget_meter.clone(),
            module_states: self.module_states.clone(),
            midi_manager: self.midi_manager.clone(),
            transport_meter: self.transport_meter.clone(),
            audio_thread_panicked: self.audio_thread_panicked.clone(),
            module_profile_collection: self.module_profile_collection.clone(),
        }
    }

    /// Enable / disable per-module profiling. Refcounted: each `true` must
    /// be balanced by a `false`. The underlying profiling global is on iff
    /// the refcount is > 0, so multiple consumers (UI panel + future
    /// telemetry, etc.) can coexist without one clobbering another's state.
    pub fn set_module_profiling_enabled(&self, on: bool) {
        if on {
            let prev = self
                .module_profiling_enable_count
                .fetch_add(1, Ordering::Relaxed);
            if prev == 0 {
                modular_core::profiling::set_enabled(true);
            }
        } else {
            let prev = self
                .module_profiling_enable_count
                .fetch_sub(1, Ordering::Relaxed);
            if prev == 1 {
                modular_core::profiling::set_enabled(false);
            } else if prev == 0 {
                // Unbalanced disable — restore the counter and ignore so callers
                // who forget a prior enable don't underflow the global.
                self.module_profiling_enable_count
                    .store(0, Ordering::Relaxed);
            }
        }
    }

    /// Profile 1-of-N audio callbacks. 1 = every callback.
    pub fn set_module_profiling_sample_rate(&self, rate: u32) {
        modular_core::profiling::set_sample_rate(rate);
    }

    /// Drain the accumulated per-module profile data. Returns one entry per
    /// module that had activity since the last drain.
    pub fn get_module_profile(&self) -> Vec<(String, ModuleProfileAccum)> {
        modular_core::profiling::drain_collection(&self.module_profile_collection)
    }

    pub fn start_recording(&self, filename: Option<String>) -> Result<String> {
        let filename =
            filename.unwrap_or_else(|| format!("recording_{}.wav", chrono_simple_timestamp()));
        let path = PathBuf::from(&filename);

        let (feed, session) = crate::recording::start(path, self.sample_rate as u32)
            .map_err(|e| napi::Error::from_reason(format!("Failed to start file write: {}", e)))?;
        *self.recording_feed.lock() = Some(feed);
        *self.recording_session.lock() = Some(session);

        Ok(filename)
    }

    pub fn stop_recording(&self) -> Result<Option<String>> {
        // Remove the callback's feed first: dropping it closes the ring's write
        // side, so the writer thread's final drain sees every pushed sample.
        drop(self.recording_feed.lock().take());
        let session = self.recording_session.lock().take();

        match session {
            Some(session) => {
                let path = session.finish().map_err(|e| {
                    napi::Error::from_reason(format!("Failed to finalize file writer: {}", e))
                })?;
                Ok(Some(path.to_string_lossy().to_string()))
            }
            None => Ok(None),
        }
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

    /// Snapshot every active $scopeXY pair as (key, xSamples, ySamples, ranges).
    /// Mirrors `get_audio_buffers` — skipped while stopped, never blocks on
    /// the audio-thread mutex.
    pub fn get_scope_xy_buffers(
        &self,
    ) -> Vec<(ScopeXyBufferKey, Float32Array, Float32Array, ScopeXyRanges)> {
        if self.is_stopped() {
            return Vec::new();
        }
        // The collection mutex guards only membership, which the audio thread
        // touches solely on patch updates (microseconds). Spin try_lock instead
        // of taking the lock so the audio thread is never blocked by this read; a
        // patch-swap collision resolves within a few spins. The buffer contents
        // are read lock-free via the SeqLock in `snapshot`.
        let mut spins = 0;
        let lock = loop {
            if let Some(lock) = self.scope_xy_collection.try_lock() {
                break lock;
            }
            spins += 1;
            if spins >= 4096 {
                return Vec::new();
            }
            std::hint::spin_loop();
        };
        // The active display window applies to every pair (global $scopeXY).
        let ranges = self.scope_xy_ranges.lock().unwrap_or(ScopeXyRanges {
            x_min: -5.0,
            x_max: 5.0,
            y_min: -5.0,
            y_max: 5.0,
        });
        let mut out = Vec::with_capacity(lock.len());
        for (key, buffer) in lock.iter() {
            let (x, y) = buffer.snapshot();
            out.push((key.clone(), x, y, ranges));
        }
        out
    }

    pub fn get_module_states(&self) -> HashMap<String, serde_json::Value> {
        // Snapshot under the lock, then build JSON after releasing it, so the audio
        // thread's `try_lock` never fails across JSON construction.
        let mut states_guard = match self.module_states.try_lock() {
            Some(guard) => guard,
            // The audio thread is writing; serve the previous snapshot so the
            // renderer never mistakes contention for module removal.
            None => return self.module_states_snapshot.lock().clone(),
        };
        let mut meta = self.module_state_meta.lock();
        // Promotion (and the slot prune it drives) happens once the swap applies, so
        // a removed module keeps highlighting until its swap lands. The applied-id
        // gate keeps new metadata from pairing with old state.
        meta.promote_if_applied(
            &mut states_guard,
            self.transport_meter.read_applied_update_id(),
        );
        // Clone only the slots that have paired metadata; an unpaired slot (a freshly
        // added module whose metadata is still pending) would be discarded below, so
        // skip its `clone_box` entirely.
        let states: Vec<(String, Box<dyn ModuleLiveState>)> = states_guard
            .iter()
            .filter(|(id, _)| meta.live.contains_key(*id))
            .map(|(id, s)| (id.clone(), s.clone_box()))
            .collect();
        drop(states_guard);
        let mut out = HashMap::with_capacity(states.len());
        for (id, live) in states {
            if let Some(m) = meta.live.get(&id) {
                out.insert(id, m.build_json(live.as_ref()));
            }
        }
        *self.module_states_snapshot.lock() = out.clone();
        out
    }

    /// `set_module_param` swaps a module bypassing `apply_patch`'s promote path,
    /// so refresh its editor state here. `None` drops any existing state for `id`.
    pub(crate) fn refresh_single_module_state(
        &self,
        id: String,
        refreshed: Option<(Box<dyn ModuleLiveState>, Box<dyn ModuleStateMeta>)>,
    ) {
        let mut states = self.module_states.lock();
        let mut meta = self.module_state_meta.lock();
        // Promote any already-applied pending first: a swap that landed since the last
        // poll must clear `pending` before this insert, otherwise the next promotion
        // would overwrite the entry written here with the patch's apply-time metadata.
        meta.promote_if_applied(&mut states, self.transport_meter.read_applied_update_id());
        match refreshed {
            Some((live, m)) => {
                // Insert a fresh slot: new params may change geometry, so a blank frame
                // beats pairing stale spans with the new metadata.
                states.insert(id.clone(), live);
                meta.live.insert(id, m);
            }
            None => {
                states.remove(&id);
                meta.live.remove(&id);
            }
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
        transport_meta: Option<TransportMeta>,
        reset_clock: bool,
    ) -> Result<()> {
        let PatchGraph {
            modules,
            module_id_remaps,
            scopes,
            scope_xy,
            ..
        } = desired_graph;

        // Build PatchUpdate with all the info needed
        let mut update = PatchUpdate::new(sample_rate);
        update.update_id = update_id;

        // Resolve id renames into the per-module transfer-source map the audio
        // thread reads during the swap.
        update.set_remaps(&module_id_remaps.unwrap_or_default());

        // Build maps for efficient lookup
        let desired_modules: HashMap<String, _> =
            modules.into_iter().map(|m| (m.id.clone(), m)).collect();

        // Build the complete next scope membership: one fresh buffer per
        // desired per-channel key (a key includes the scope's config, so a
        // config change is a new key). At apply time the audio thread moves
        // each carried-over key's live buffer state into this map and swaps it
        // in wholesale, so the resulting membership is exactly this update's
        // desired set no matter which other updates apply in between.
        update.scope_next = scopes
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
            .map(|key| {
                let buffer = ScopeBuffer::new(key.ms_per_frame, key.trigger_threshold, sample_rate);
                (key, buffer)
            })
            .collect();

        // Build the complete next XY-scope membership. Single global $scopeXY;
        // keys are pair-indexed so re-ordering one pair forces a full rebuild
        // (intentional — buffers are sample-rate ring buffers that would smear
        // if reused). Pairs already present keep their live buffer `Arc` (ring
        // continuity); new pairs get fresh buffers. The audio thread swaps the
        // map and list in wholesale at apply time, so both are allocated here.
        {
            let scope_xy_collection = self.scope_xy_collection.lock();

            let desired_keys: Vec<ScopeXyBufferKey> = match scope_xy.as_ref() {
                Some(sx) => sx
                    .pairs
                    .iter()
                    .enumerate()
                    .map(|(i, p)| ScopeXyBufferKey {
                        index: i as u32,
                        pair: p.clone(),
                    })
                    .collect(),
                None => Vec::new(),
            };

            update.scope_xy_next = desired_keys
                .into_iter()
                .map(|key| {
                    let buffer = scope_xy_collection
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| Arc::new(ScopeXyBuffer::new()));
                    (key, buffer)
                })
                .collect();
            update.scope_xy_audio_next = update
                .scope_xy_next
                .iter()
                .map(|(key, buffer)| (key.clone(), Arc::clone(buffer)))
                .collect();
        }

        // Carry the active display window (global, last-call-wins) on the update so
        // the audio thread publishes it atomically with the XY buffer swap — a
        // queued update's window then takes effect exactly when its patch applies.
        // `None` clears the window when the patch has no $scopeXY.
        update.scope_xy_ranges = scope_xy.as_ref().map(|sx| ScopeXyRanges {
            x_min: sx.x_range.0,
            x_max: sx.x_range.1,
            y_min: sx.y_range.0,
            y_max: sx.y_range.1,
        });

        // Pass 1 — deserialize every module's params and collect the cable
        // adjacency. Keep the deserialized params alongside the module type so
        // pass 2 can pick the right constructor without re-deserializing.
        let mut deserialized_modules: Vec<(
            String,
            String,
            modular_core::params::DeserializedParams,
        )> = Vec::with_capacity(desired_modules.len());
        let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
        // Build each stateful module's editor-state halves from the raw params now,
        // before the deserialize step below consumes them: the immutable metadata
        // (cached to build the editor JSON on poll) and the pre-allocated live slot
        // the audio thread will fill. Only modules registered in
        // `get_module_state_builders` produce any.
        let state_builders = modular_core::dsp::get_module_state_builders();
        let mut state_metas: HashMap<String, Box<dyn ModuleStateMeta>> = HashMap::new();
        let mut state_live: HashMap<String, Box<dyn ModuleLiveState>> = HashMap::new();
        for (id, module_state) in desired_modules {
            if let Some(builder) = state_builders.get(&module_state.module_type)
                && let Some((live, meta)) = builder(&module_state.params)
            {
                state_metas.insert(id.clone(), meta);
                state_live.insert(id.clone(), live);
            }
            let deserialized =
                crate::deserialize_params(&module_state.module_type, module_state.params, true)
                    .map_err(|e| {
                        napi::Error::from_reason(format!(
                            "Failed to deserialize params for {}: {}",
                            id, e
                        ))
                    })?;
            let mut producers = Vec::new();
            deserialized.params.collect_cables(&mut producers);
            adjacency.insert(id.clone(), producers);
            deserialized_modules.push((id, module_state.module_type, deserialized));
        }

        // One Tarjan SCC pass over the adjacency map yields both the per-module
        // processing mode and a cache-efficient processing order. Cycle
        // participants get `Sample` mode so the wrapper computes one sample at a
        // time and the 1-sample feedback delay invariant holds; everyone else
        // gets `Block`. The order lists producers before consumers.
        let analysis = crate::graph_analysis::analyze(&adjacency);
        let mode_map = analysis.modes;
        update.process_order_ids = analysis.order;
        // Empty storage, allocated here, that the audio thread swaps in for its
        // pointer list so resolving every order id never grows a Vec there.
        update.process_order_scratch = Vec::with_capacity(update.process_order_ids.len());

        // Pass 2 — resolve each module's processing mode, then construct on the
        // main thread into the unconnected `new_patch`. The audio thread copies
        // runtime state from the live patch into these modules, connects, then
        // swaps the whole patch in. `block_ids`/`sample_ids` partition the
        // constructed set by mode to seed the profiler maps below.
        let mut to_construct: Vec<(
            String,
            String,
            modular_core::params::DeserializedParams,
            modular_core::types::ProcessingMode,
        )> = Vec::with_capacity(deserialized_modules.len());
        let mut block_ids: Vec<String> = Vec::new();
        let mut sample_ids: Vec<String> = Vec::new();
        for (id, module_type, deserialized) in deserialized_modules {
            // ROOT_CLOCK is always Sample mode: the block-aware audio callback
            // (when it lands) needs to eager-fill its trigger outputs one sample
            // at a time so the queued-update trigger check can fire on an exact
            // sample boundary, not a block boundary. At today's effective
            // `block_size=1` the choice is moot, but baking the override in here
            // keeps it correct once the audio loop flips.
            let mode = if id == *modular_core::types::ROOT_CLOCK_ID {
                modular_core::types::ProcessingMode::Sample
            } else {
                mode_map
                    .get(&id)
                    .copied()
                    .unwrap_or(modular_core::types::ProcessingMode::Block)
            };
            match mode {
                modular_core::types::ProcessingMode::Block => block_ids.push(id.clone()),
                modular_core::types::ProcessingMode::Sample => sample_ids.push(id.clone()),
            }
            to_construct.push((id, module_type, deserialized, mode));
        }
        update
            .new_patch
            .insert_modules(to_construct, sample_rate, self.block_size)
            .map_err(napi::Error::from_reason)?;
        block_ids.sort();
        sample_ids.sort();
        println!(
            "[patch] update_id={} modules:\n  block=[\n    {}\n  ]\n  sample=[\n    {}\n  ]",
            update.update_id,
            block_ids.join(",\n    "),
            sample_ids.join(",\n    ")
        );

        update.new_patch.rebuild_message_listeners();

        // Profiler seed maps for the audio thread's TLS records and shared map.
        // One entry per constructed id (HIDDEN_AUDIO_IN carries no record), two
        // maps because each `swap_*` consumes its operand. `block_ids` and
        // `sample_ids` together are exactly the constructed set.
        update.profile_records_seed =
            modular_core::profiling::build_seed(block_ids.iter().chain(&sample_ids).cloned());
        update.profile_shared_seed =
            modular_core::profiling::build_seed(block_ids.iter().chain(&sample_ids).cloned());

        // Run main-thread resource preparation (e.g. FFT-based mipmap generation for
        // wavetable oscillators). Called here because allocation and file-backed
        // data access must not happen on the audio thread.
        for module in update.new_patch.sampleables.values() {
            module.prepare_resources(&wav_data);
        }

        // Populate wav_data from the cache snapshot
        update.new_patch.wav_data = wav_data;

        // Carry tempo/time-sig to apply time: the meter write and (when $setTempo
        // was called) the Link tempo push happen in apply_patch_update, atomically
        // with the module swap.
        update.transport_meta = transport_meta;

        // Restart the transport when applying this update (set on a buffer switch)
        update.reset_clock = reset_clock;

        // Add-only: the audio thread must write only into pre-existing slots, so a
        // dropped module's removal is deferred until its swap lands. Promote any
        // prior applied update first (bounds slots if the editor isn't polling).
        {
            let mut states = self.module_states.lock();
            let mut meta = self.module_state_meta.lock();
            meta.promote_if_applied(&mut states, self.transport_meter.read_applied_update_id());
            for (id, live) in state_live {
                states.entry(id).or_insert(live);
            }
            meta.pending.push((update_id, state_metas));
        }

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
        transport_meta: Option<TransportMeta>,
        reset_clock: bool,
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
            let _ = self.send_command(GraphCommand::ClearPatch {
                fresh_patch: Patch::new(),
            });
            self.scope_collection.lock().clear();
            self.scope_xy_collection.lock().clear();
            self.request_start();
        }

        // Apply patch
        if let Err(e) = self.apply_patch(
            patch_graph,
            sample_rate,
            trigger,
            update_id,
            wav_data,
            transport_meta,
            reset_clock,
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
    pub scope_xy_collection: Arc<Mutex<HashMap<ScopeXyBufferKey, Arc<ScopeXyBuffer>>>>,
    /// Display ranges for the active $scopeXY — written by the audio thread on
    /// apply, read by the main thread for the renderer.
    pub scope_xy_ranges: Arc<Mutex<Option<ScopeXyRanges>>>,
    /// Audio-thread half of the active recording session: a lock-free ring the
    /// callback pushes f32 samples into. Disk I/O stays on the session's
    /// writer thread.
    pub recording_feed: Arc<Mutex<Option<RecordingFeed>>>,
    pub audio_budget_meter: Arc<AudioBudgetMeter>,
    /// Live per-module editor state - written by audio thread, read by main thread
    pub module_states: Arc<Mutex<ModuleStateMap>>,
    /// MIDI input manager for polling MIDI messages
    pub midi_manager: Arc<MidiInputManager>,
    /// Transport state meter - written by audio thread, read by main thread
    pub transport_meter: Arc<TransportMeter>,
    /// Set by the audio callback after catching a panic. The callback then
    /// writes silence forever; the stream must be torn down to recover.
    pub audio_thread_panicked: Arc<AtomicBool>,
    /// Per-module profiler snapshot — audio thread flushes here via try_lock
    /// at the end of every callback.
    pub module_profile_collection: ModuleProfileCollection,
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
/// A raw handle into an audio-thread-owned module box, used to drive every
/// module's processing in a cache-efficient order without a per-module hash
/// lookup. SAFETY mirrors the cable `cached_source_ptr` (see
/// `modular_core::types::SampleablePtr`): the pointer is resolved from a box
/// in `AudioProcessor::patch.sampleables` after `connect()`, rebuilt on every
/// structural patch change (insert/remove/remap/replace), and dereferenced
/// only on the audio thread, which has exclusive access to the patch.
/// `ensure_processed_to` takes `&self` and mutates only through the module's
/// `UnsafeCell`, so shared aliasing with the owning `Box` is sound.
pub(crate) struct ProcessHandle(modular_core::types::SampleablePtr);

// SAFETY: the pointed-to module is `Send` (Sampleable: Send) and is only ever
// dereferenced on the audio thread that owns the patch. This mirrors the
// `unsafe impl Send` on `Signal`/`Buffer`, which cache the same pointer type.
unsafe impl Send for ProcessHandle {}

struct AudioProcessor {
    /// The DSP patch graph - owned directly, no mutex needed
    patch: Patch,
    /// Cache-efficient processing order: raw handles into every module box in
    /// `patch.sampleables`, ordered producers-before-consumers. Walked once per
    /// block by `process_all_modules_to` so every module advances each block,
    /// whether or not it is reachable from the output or a scope. Rebuilt from
    /// `process_order_ids` on every structural patch change.
    process_order: Vec<ProcessHandle>,
    /// The module IDs behind `process_order`, in the same order, kept so the
    /// pointer list can be rebuilt after a box moves (e.g. `SingleModuleUpdate`)
    /// without re-running graph analysis.
    process_order_ids: Vec<String>,
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
    /// Shared XY scope collection (single global $scopeXY at most). Holds the
    /// canonical membership; co-owns each buffer with `scope_xy_audio`.
    scope_xy_collection: Arc<Mutex<HashMap<ScopeXyBufferKey, Arc<ScopeXyBuffer>>>>,
    /// Audio-thread-private view of the XY scope buffers. Iterated every frame
    /// to append samples, so the per-sample path never touches the collection
    /// mutex. Each patch update swaps in a complete main-thread-built
    /// replacement (`PatchUpdate::scope_xy_audio_next`); this thread never
    /// pushes to or drops from it.
    scope_xy_audio: Vec<(ScopeXyBufferKey, Arc<ScopeXyBuffer>)>,
    /// Shared XY-scope display window. Written here in `apply_patch_update` so the
    /// volt→clip mapping swaps atomically with the buffers; read by the main
    /// thread for the renderer.
    scope_xy_ranges: Arc<Mutex<Option<ScopeXyRanges>>>,
    /// Shared live per-module editor state. Written into existing slots each
    /// callback; never adds or removes keys (the main thread owns those).
    module_states: Arc<Mutex<ModuleStateMap>>,
    /// MIDI input manager for polling
    midi_manager: Arc<MidiInputManager>,
    /// Queued patch update waiting for its trigger condition
    queued_update: Option<(PatchUpdate, QueuedTrigger)>,
    /// Transport state meter - written each frame, read by main thread
    transport_meter: Arc<TransportMeter>,
    /// Sample rate of the audio stream
    sample_rate: f32,
    /// Internal block size used for module construction (matches the
    /// `block_size` each wrapper allocates `BlockPort`s for). The audio
    /// callback drains `block_size` cpal frames per block-level pass.
    block_size: usize,
    /// Position within the current internal block. Persists across CPAL
    /// callbacks. Initialised to `block_size` so the first cpal frame
    /// triggers block-level work immediately.
    block_pos: usize,
    /// Pre-allocated scratch buffer for one internal block's worth of host
    /// audio input. Filled at the block boundary from `input_reader` and
    /// injected into `HiddenAudioIn` via `inject_audio_in_block`. Sized to
    /// `block_size` once in `new()` so the audio thread never allocates.
    input_block_scratch: Vec<[f32; PORT_MAX_CHANNELS]>,
    /// Ableton Link integration (audio-thread side). Owns the live
    /// `rusty_link` resources when active and exposes only RT-safe operations.
    link: crate::link::LinkState,
    /// Shared per-module profile snapshot — flushed at end of each callback.
    module_profile_collection: ModuleProfileCollection,
    /// Profiler shared-map seed whose swap lost the `try_lock` race with the
    /// main-thread drain. Retried on a later callback (see
    /// `retry_pending_profile_seed`) so the audio thread never blocks on the
    /// profiler mutex. Normally `None`.
    pending_profile_shared_seed: Option<HashMap<String, ModuleProfileAccum>>,
    /// Pre-allocated buffer the pending MIDI queue is swapped into each
    /// callback (see `MidiInputManager::take_messages_into`), sorted in place,
    /// dispatched, and cleared. `Message` is drop-free (its device name is
    /// interned), so the clear never deallocates here.
    midi_scratch: Vec<crate::midi::TimestampedMessage>,
}

impl AudioProcessor {
    fn new(
        command_rx: CommandConsumer,
        error_tx: ErrorProducer,
        garbage_tx: GarbageProducer,
        shared: AudioSharedState,
        sample_rate: f32,
        block_size: usize,
    ) -> Self {
        // Force the lazy module-id statics while still on the main thread:
        // their first deref allocates, and the audio callback derefs them
        // every buffer.
        {
            use modular_core::types::{ROOT_CLOCK_ID, ROOT_ID, ROOT_OUTPUT_PORT};
            let _ = (&*ROOT_ID, &*ROOT_CLOCK_ID, &*ROOT_OUTPUT_PORT);
        }
        Self {
            patch: Patch::new(),
            process_order: Vec::new(),
            process_order_ids: Vec::new(),
            command_rx,
            error_tx,
            garbage_tx,
            stopped: shared.stopped,
            scope_collection: shared.scope_collection,
            scope_xy_collection: shared.scope_xy_collection,
            scope_xy_audio: Vec::new(),
            scope_xy_ranges: shared.scope_xy_ranges,
            module_states: shared.module_states,
            midi_manager: shared.midi_manager,
            queued_update: None,
            transport_meter: shared.transport_meter,
            sample_rate,
            block_size,
            block_pos: block_size,
            input_block_scratch: vec![[0.0f32; PORT_MAX_CHANNELS]; block_size],
            link: crate::link::LinkState::new(),
            module_profile_collection: shared.module_profile_collection,
            pending_profile_shared_seed: None,
            midi_scratch: Vec::with_capacity(crate::midi::MIDI_BUFFER_SIZE),
        }
    }

    /// Route an evicted profiler map to the garbage queue for drop on the
    /// main thread.
    fn drop_profile_map(&mut self, map: HashMap<String, ModuleProfileAccum>) {
        self.try_push_garbage_item(GarbageItem::ProfileMap(map));
    }

    /// Retry a deferred profiler shared-map swap. A swap is deferred when
    /// [`modular_core::profiling::try_swap_shared`] loses the `try_lock` race
    /// with the main-thread drain; retrying here keeps the audio thread from
    /// ever blocking on the profiler mutex. No-op when nothing is pending.
    fn retry_pending_profile_seed(&mut self) {
        let Some(seed) = self.pending_profile_shared_seed.take() else {
            return;
        };
        match modular_core::profiling::try_swap_shared(&self.module_profile_collection, seed) {
            Ok(old) => self.drop_profile_map(old),
            Err(seed) => {
                self.pending_profile_shared_seed = Some(seed);
            }
        }
    }

    /// Ship `item` to the main thread for deallocation. On a full garbage
    /// queue the item reroutes through the error queue, which the main thread
    /// also drains and drops; only with both queues saturated does the drop
    /// land here (memory-safe but a violation of the no-audio-thread-dealloc
    /// invariant, so both capacities are sized to make that unreachable).
    fn try_push_garbage_item(&mut self, item: GarbageItem) {
        if let Err(err) = self.garbage_tx.push(item) {
            let _ = self
                .error_tx
                .push(AudioError::GarbageQueueFull { message: err });
        }
    }

    /// Process all pending commands from the main thread and poll MIDI.
    /// Called at the start of each audio callback before processing frames.
    fn process_commands(&mut self) {
        // Poll MIDI and dispatch directly to the patch — on the audio thread
        // for low-latency response. The pending queue is swapped into the
        // pre-allocated scratch and sorted in place; devices interleave, so
        // temporal order is (timestamp, arrival seq).
        self.midi_manager.take_messages_into(&mut self.midi_scratch);
        self.midi_scratch
            .sort_unstable_by_key(|tm| (tm.timestamp_us, tm.seq));
        for tm in self.midi_scratch.iter() {
            if let Err(e) = self.patch.dispatch_message(&tm.message) {
                let _ = self
                    .error_tx
                    .push(AudioError::MessageDispatchFailed { message: e });
            }
        }
        self.midi_scratch.clear();

        // Process commands from the main thread
        while let Ok(cmd) = self.command_rx.pop() {
            match cmd {
                GraphCommand::QueuedPatchUpdate {
                    mut update,
                    trigger,
                } => {
                    // The Link tempo push and meter write are applied in
                    // `apply_patch_update`, atomically with the module swap. Pushing the
                    // tempo while the update is only queued would advance the
                    // still-playing patch's phase at the new tempo before its swap.

                    // If there's already a queued update, discard it and apply the new one
                    // immediately. This is intentional: when the user triggers a second
                    // update before the first one fires (e.g. pressing Ctrl+Enter twice),
                    // we treat it as "apply now" rather than re-queuing for the next
                    // bar/beat.
                    if let Some((old_update, _)) = self.queued_update.take() {
                        // Carry a pending transport reset forward: if the superseded update
                        // was a buffer switch that never fired, the immediate apply still
                        // lands on a different song than what's playing, so it must still
                        // restart the clock.
                        update.reset_clock |= old_update.reset_clock;
                        self.try_push_garbage_item(GarbageItem::PatchUpdate(old_update));
                        self.queued_update = Some((update, QueuedTrigger::Immediate));
                    } else {
                        self.queued_update = Some((update, trigger));
                    }
                }
                GraphCommand::SingleModuleUpdate {
                    module_id,
                    module: new_module,
                } => {
                    // State transfer + replace, then reconnect. `remove_entry`
                    // keeps the map's owned key so the re-insert reuses it (no
                    // clone, and no growth after a same-key remove); the key's
                    // strings never drop here. Message listeners are untouched:
                    // they are keyed by id, and the replacement has the same id
                    // and type — hence the same static `handled_message_tags` —
                    // so the existing entries already route to the new box.
                    if let Some((key, old_module)) = self.patch.sampleables.remove_entry(&module_id)
                    {
                        new_module.transfer_state_from(old_module.as_ref());
                        self.patch.sampleables.insert(key, new_module);
                        self.try_push_garbage_item(GarbageItem::Module((module_id, old_module)));
                        // Reconnect all modules so the new module picks up its
                        // connections, then refresh module-local caches rebuilt
                        // from params.
                        self.patch.connect_all();
                        // The old box was dropped and a fresh one inserted at this id, so its
                        // cached pointer now dangles. The graph shape is unchanged (same
                        // modules and cables — only this module's params differ), so reuse
                        // `process_order_ids` and just re-resolve the pointers.
                        self.rebuild_process_order_ptrs();
                    } else {
                        // No module at this id (it raced a removal in a patch
                        // swap). Inserting would grow the map and register a
                        // module absent from the processing order, so ship the
                        // unmatched replacement straight back for deallocation.
                        self.try_push_garbage_item(GarbageItem::Module((module_id, new_module)));
                    }
                }
                GraphCommand::DispatchMessage(msg) => {
                    if let Err(e) = self.patch.dispatch_message(&msg) {
                        let _ = self
                            .error_tx
                            .push(AudioError::MessageDispatchFailed { message: e });
                    }
                }
                GraphCommand::Start => {
                    if self.link.is_active() {
                        // With Link: keep `stopped=true` and arm a pending start.
                        // The buffer-level phase check releases `stopped` when the
                        // shared Link timeline reaches a bar boundary.
                        self.stopped
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                        self.link.request_quantized_start();
                    } else {
                        // Free-run: flip immediately and reset the clock.
                        self.stopped
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
                GraphCommand::ClearPatch { mut fresh_patch } => {
                    // Discard any pending queued update
                    if let Some((old_update, _)) = self.queued_update.take() {
                        self.try_push_garbage_item(GarbageItem::PatchUpdate(old_update));
                    }
                    // Swap in the main-thread-built empty patch (hidden audio-in
                    // and message listeners already registered) and ship the
                    // whole old patch off for main-thread deallocation.
                    std::mem::swap(&mut self.patch, &mut fresh_patch);
                    self.try_push_garbage_item(GarbageItem::Patch(fresh_patch));
                    // Take the audio-private XY scope mirror to match the main
                    // thread's `scope_xy_collection.clear()` on stop. Without
                    // this, a stopped rerun swaps in the new membership while
                    // these stale entries would already be gone from the
                    // collection, pinning the orphaned buffers. `take` leaves an
                    // unallocated Vec; the next patch update swaps in a fresh
                    // main-thread-built list.
                    let xy_audio = std::mem::take(&mut self.scope_xy_audio);
                    self.try_push_garbage_item(GarbageItem::ScopeXyAudio(xy_audio));
                    // No user modules remain (only the hidden audio-in, which is driven
                    // by injection), so there is nothing to force-process. The
                    // pointer list has no heap contents (`ProcessHandle` is a raw
                    // pointer), so clearing it deallocates nothing; the id list's
                    // strings must drop on the main thread.
                    self.process_order.clear();
                    let order_ids = std::mem::take(&mut self.process_order_ids);
                    self.try_push_garbage_item(GarbageItem::OrderIds(order_ids));
                }
                GraphCommand::SetLink(new_resources) => {
                    // Construction/destruction of `AblLink` (and `enable()`) are
                    // realtime-unsafe per Ableton's documentation. They run on the
                    // main thread; the audio thread only swaps the prepared resources
                    // in/out and ships any old set off to the garbage queue for
                    // main-thread drop.
                    let was_active = self.link.is_active();
                    if let Some(old) = self.link.install(new_resources) {
                        self.try_push_garbage_item(GarbageItem::Link(old));
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

    /// Apply a queued patch update. `swap_pos` is the per-block slot index at
    /// which the swap is happening; every newly-inserted module's per-block
    /// cursor is set to `swap_pos` so subsequent `ensure_processed_to(i)`
    /// calls within this block fill slots `[swap_pos, block_size)` only.
    /// Slots `[0, swap_pos)` were already emitted by the pre-swap modules.
    ///
    /// Everything the update displaces — the old patch, processing order and
    /// pointer capacity, scope map, XY scope map and list, evicted profiler
    /// maps — is stowed back into the consumed `update` (the "husk") and
    /// shipped to the garbage queue in a single push at the end, so applying a
    /// patch neither allocates nor deallocates on the audio thread.
    fn apply_patch_update(&mut self, mut update: PatchUpdate, swap_pos: usize) {
        // The new patch arrives fully constructed but unconnected. Bring it to liveness
        // without ever touching the live patch's map, then swap.

        // Copy runtime state from the live (old) patch into each new module.
        for (id, module) in update.new_patch.sampleables.iter() {
            let source_id = match update.transfer_sources.get(id) {
                Some(Some(old_id)) => Some(old_id.as_str()),
                Some(None) => None,
                None => Some(id.as_str()),
            };
            if let Some(old_id) = source_id
                && let Some(old) = self.patch.sampleables.get(old_id)
            {
                module.transfer_state_from(old.as_ref());
            }
        }

        // `connect` resolves raw pointers into upstream modules' just-transferred
        // output buffers, so it MUST run after the state copy above.
        update.new_patch.connect_all();

        if swap_pos > 0 {
            for module in update.new_patch.sampleables.values() {
                module.set_initial_index(swap_pos);
            }
        }

        std::mem::swap(&mut self.patch, &mut update.new_patch);
        std::mem::swap(&mut self.process_order_ids, &mut update.process_order_ids);
        // The scratch arrives empty with capacity for every order id, so the
        // rebuild below fills the pointer list without growing it.
        std::mem::swap(&mut self.process_order, &mut update.process_order_scratch);
        // run after every structural change above so no pointer dangles
        self.rebuild_process_order_ptrs();

        // Swap in the complete new scope membership built on the main thread.
        // A key also present in the live map first takes over that buffer's
        // state (`mem::swap` in place — no allocation), so an unchanged
        // scope's display stays continuous across the swap. The displaced map
        // — removed keys' buffers plus the unused fresh ones — rides the husk
        // back for main-thread drop.
        {
            let mut scope_collection = self.scope_collection.lock();
            for (key, buffer) in update.scope_next.iter_mut() {
                if let Some(live) = scope_collection.get_mut(key) {
                    std::mem::swap(buffer, live);
                }
            }
            std::mem::swap(&mut *scope_collection, &mut update.scope_next);
        }

        // Profiler-map swap: `swap_records` updates the audio thread's TLS
        // records, `try_swap_shared` updates the cross-thread snapshot. Both
        // operands were allocated on the main thread; the evicted maps ride
        // the husk back for a main-thread drop. Counters accumulated since the
        // last UI drain do not survive the swap — a patch swap is a graph
        // discontinuity, and pre-swap stats are not comparable to post-swap
        // ones.
        {
            let records_seed = std::mem::take(&mut update.profile_records_seed);
            update.profile_records_seed = modular_core::profiling::swap_records(records_seed);
            // Non-blocking shared-map swap. On `try_lock` contention with the
            // main-thread drain, stash the seed and retry on a later callback
            // (see `retry_pending_profile_seed`) so the audio thread never blocks.
            let shared_seed = std::mem::take(&mut update.profile_shared_seed);
            match modular_core::profiling::try_swap_shared(
                &self.module_profile_collection,
                shared_seed,
            ) {
                Ok(old_shared) => update.profile_shared_seed = old_shared,
                Err(seed) => {
                    if let Some(superseded) = self.pending_profile_shared_seed.replace(seed) {
                        update.profile_shared_seed = superseded;
                    }
                }
            }
        }

        // Swap in the complete new XY-scope membership built on the main
        // thread: the shared map (canonical membership, read by the renderer)
        // and the audio-private list the per-sample path iterates. Kept pairs
        // carry the same buffer `Arc`s, so their rings stay continuous across
        // the swap; the displaced map and list ride the husk.
        {
            let mut m = self.scope_xy_collection.lock();
            std::mem::swap(&mut *m, &mut update.scope_xy_next);
        }
        std::mem::swap(&mut self.scope_xy_audio, &mut update.scope_xy_audio_next);

        // Publish the XY display window now, atomically with the buffer swap above,
        // so the renderer maps this patch's signal against this patch's volt→clip
        // window from the instant the patch applies. `None` clears it.
        *self.scope_xy_ranges.lock() = update.scope_xy_ranges;

        // Restart the transport when this update switches playback to a different
        // buffer (song). Runs after the inserts/transfer/connect above so
        // ROOT_CLOCK's message listener is registered; the swap-site eager-fill
        // then refills the block.
        if update.reset_clock {
            if self.link.is_active() {
                // Under Ableton Link the shared session timeline owns the bar phase —
                // a full Clock::Start would be re-overridden by the next synced sample
                // and would spuriously re-fire the bar/beat triggers. The swap is
                // quantized to a bar boundary (NextBar; see `update_patch`), so reset
                // only the local bar (loop) index: the incoming song's bar count
                // restarts at zero while the phase stays locked to the peer timeline.
                use modular_core::types::ROOT_CLOCK_ID;
                if let Some(root_clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) {
                    root_clock.reset_loop_index();
                }
            } else {
                // Free-run: restart the whole transport (phase, beat, bar count) from
                // zero. The swap-site eager-fill then refills the block from phase 0.
                let _ = self
                    .patch
                    .dispatch_message(&Message::Clock(ClockMessages::Start));
            }
        }

        // Apply tempo/time-sig now, atomically with the swap. The meter readout
        // always reflects the patch that just applied; the Link push only happens
        // when the DSL explicitly set the tempo, so a plain live-coding edit never
        // overrides a peer-driven Link tempo.
        if let Some(t) = &update.transport_meta {
            self.transport_meter
                .write_tempo(t.tempo, t.numerator, t.denominator);
            if t.tempo_set {
                self.link.set_tempo_now(t.tempo);
            }
        }

        // Ship the husk — old patch, old order ids and pointer capacity, the
        // displaced scope map, old XY map/list, evicted profiler maps — in a
        // single push for main-thread deallocation.
        self.try_push_garbage_item(GarbageItem::PatchUpdate(update));
    }

    /// Pull one full block of host audio input from `input_reader` into
    /// `input_block_scratch` and hand it to `HiddenAudioIn` via
    /// `inject_audio_in_block`. Called once per internal block at the
    /// boundary from the cpal callback.
    fn pull_input_block(&mut self, input_reader: &mut InputBufferReader) {
        use modular_core::types::WellKnownModule;
        for slot in self.input_block_scratch.iter_mut() {
            let frame = input_reader.read_frame();
            for ch in 0..PORT_MAX_CHANNELS {
                slot[ch] = frame[ch] * AUDIO_INPUT_GAIN;
            }
        }
        if let Some(audio_in) = self
            .patch
            .sampleables
            .get(WellKnownModule::HiddenAudioIn.id())
        {
            audio_in.inject_audio_in_block(&self.input_block_scratch);
        }
    }

    /// Rebuild the cache-efficient processing-order pointer list from the
    /// stored `process_order_ids`. Each `SampleablePtr` targets a module's heap
    /// pointee (via `module.as_ref()`), which stays put when its `Box` is moved
    /// (a HashMap rehash relocates the fat pointer, not the allocation) — that
    /// stability is exactly what makes the cached-pointer scheme sound. A handle
    /// only dangles when its module is dropped or replaced (the pointee is
    /// freed), so this must run after ANY mutation of `patch.sampleables` that
    /// removes/replaces a module — the same discipline the cable
    /// `cached_source_ptr`s follow, which is why every such site also re-runs
    /// `connect()`. IDs absent from the patch (e.g. dangling cable targets) are
    /// skipped. Never grows the list on this thread: every patch update swaps
    /// in main-thread-allocated storage with capacity for its full id set
    /// (`PatchUpdate::process_order_scratch`), and the other caller
    /// (`SingleModuleUpdate`) reuses the same ids, so the `reserve` is always
    /// satisfied by existing capacity.
    fn rebuild_process_order_ptrs(&mut self) {
        let AudioProcessor {
            patch,
            process_order,
            process_order_ids,
            ..
        } = self;
        process_order.clear();
        process_order.reserve(process_order_ids.len());
        for id in process_order_ids.iter() {
            if let Some(module) = patch.sampleables.get(id) {
                process_order.push(ProcessHandle(modular_core::types::SampleablePtr::from(
                    module.as_ref(),
                )));
            }
        }
    }

    /// Force every module — including those not reachable from the output or any
    /// scope — to advance to slot `end` within the current internal block, in
    /// cache-efficient producer-before-consumer order. Each wrapper memoises per
    /// slot, so this is idempotent and any later root/scope `get_value_at` read
    /// of an already-advanced slot is a pure cache hit. Cycles stay correct
    /// regardless of which member is reached first: the wrapper's reentrancy
    /// guard preserves the 1-sample feedback delay.
    #[inline]
    fn process_all_modules_to(&self, end: usize) {
        for handle in &self.process_order {
            // SAFETY: see `ProcessHandle`. Audio-thread-exclusive access; the
            // pointer is live (rebuilt on every structural patch change) and
            // `ensure_processed_to` mutates only through the module's UnsafeCell.
            unsafe { handle.0.as_ref().ensure_processed_to(end) };
        }
    }

    /// Eager-fill ROOT_CLOCK from per-block slot `from` (inclusive) to `end`
    /// (exclusive). Each slot is synced under this callback's Link host-time
    /// anchor — the cpal-frame index for in-block slot `i` is
    /// `written_at_call + (i - from)`. Subsequent `get_value_at(port, ch, i)`
    /// reads on ROOT_CLOCK are pure cache hits.
    fn eager_fill_clock(&self, from: usize, end: usize, written_at_call: usize) {
        use modular_core::types::ROOT_CLOCK_ID;
        let Some(root_clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) else {
            return;
        };
        for i in from..end {
            let cb_frame = written_at_call + (i - from);
            if let Some((bar_phase, tempo)) = self.link.phase_at_frame(cb_frame) {
                root_clock.sync_external_clock(modular_core::types::ExternalClockState {
                    bar_phase,
                    bpm: tempo,
                });
            }
            root_clock.ensure_processed_to(i + 1);
        }
    }

    /// Scan ROOT_CLOCK's `port` trigger output for the first slot in
    /// `[from, end)` where it goes `>= 1.0`. Pure cache reads — eager-fill
    /// has already populated the requested range. Returns `None` if no
    /// trigger fires within the range.
    ///
    /// Fallback: when ROOT_CLOCK is absent (e.g. immediately after a clear
    /// patch leaves only `HiddenAudioIn`), returns `Some(from)` so queued
    /// patches still apply rather than sticking around forever.
    fn scan_trigger(&self, port: &str, from: usize, end: usize) -> Option<usize> {
        use modular_core::types::ROOT_CLOCK_ID;
        let Some(root_clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) else {
            return Some(from);
        };
        for i in from..end {
            if root_clock.get_value_at(port, 0, i) >= 1.0 {
                return Some(i);
            }
        }
        None
    }

    fn collect_module_states(&self) {
        if let Some(mut states) = self.module_states.try_lock() {
            for (id, slot) in states.iter_mut() {
                match self.patch.sampleables.get(id) {
                    Some(module) => module.write_module_state(&mut **slot),
                    None => slot.reset(),
                }
            }
        }
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

pub fn make_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    command_rx: CommandConsumer,
    error_tx: ErrorProducer,
    garbage_tx: GarbageProducer,
    shared: AudioSharedState,
    mut input_reader: InputBufferReader,
    block_size: usize,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let num_channels = config.channels as usize;

    let err_fn = |err| eprintln!("Error building output sound stream: {err}");

    let time_at_start = std::time::Instant::now();
    println!("Time at start: {time_at_start:?}");

    // Clone shared state for the closure
    let recording_feed = shared.recording_feed.clone();
    let audio_budget_meter = shared.audio_budget_meter.clone();
    let panicked_flag = shared.audio_thread_panicked.clone();

    // Create the audio processor that owns the patch
    let sample_rate = config.sample_rate as f32;
    let mut audio_processor = AudioProcessor::new(
        command_rx,
        error_tx,
        garbage_tx,
        shared,
        sample_rate,
        block_size,
    );

    let mut final_state_processor = FinalStateProcessor::new();

    let stream = device
        .build_output_stream(
            config,
            move |output: &mut [T], _info: &cpal::OutputCallbackInfo| {
                // If a previous callback panicked, never touch the captured state
                // again — it may be in an inconsistent state. Emit silence until the
                // stream is torn down and the Synthesizer recreated.
                if panicked_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    for s in output.iter_mut() {
                        *s = T::from_sample(0.0);
                    }
                    return;
                }
                // Dev-only: mark this thread as the audio thread so the global
                // allocation detector records (and attributes) any alloc/dealloc
                // made below. Placed after the panicked-flag early return and
                // outside `catch_unwind`, so the guard's `Drop` runs even on a
                // caught unwind. Compiles to nothing without `--features=alloc-detector`.
                #[cfg(feature = "alloc-detector")]
                let _alloc_scope = crate::alloc_detector::AudioThreadScope::enter();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    use modular_core::types::{ROOT_CLOCK_ID, ROOT_ID};
                    profiling::scope!("audio_callback");

                    let callback_start = Instant::now();

                    // Mirror the main-thread enable atomic into the audio thread's
                    // profiler TLS. Cheap relaxed load once per callback.
                    modular_core::profiling::refresh_enabled();

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
                    audio_processor
                        .link
                        .write_meter(&audio_processor.transport_meter);

                    let num_frames = output.len() / num_channels;
                    let block_size = audio_processor.block_size;

                    // Initial block-boundary entry: either fresh callback at a clean
                    // boundary, or resuming mid-block from the previous callback.
                    // Either way, eager-fill ROOT_CLOCK under THIS callback's
                    // host-time anchor for the samples we will emit.
                    if audio_processor.block_pos >= block_size {
                        for module in audio_processor.patch.sampleables.values() {
                            module.start_block();
                        }
                        audio_processor.pull_input_block(&mut input_reader);
                        audio_processor.block_pos = 0;
                    }

                    let mut written: usize = 0;

                    {
                        let eager_end = block_size.min(audio_processor.block_pos + num_frames);
                        audio_processor.eager_fill_clock(
                            audio_processor.block_pos,
                            eager_end,
                            written,
                        );
                    }

                    {
                        profiling::scope!("process_frames");
                        while written < num_frames {
                            // Cross internal block boundary mid-callback.
                            if audio_processor.block_pos >= block_size {
                                for module in audio_processor.patch.sampleables.values() {
                                    module.start_block();
                                }
                                audio_processor.pull_input_block(&mut input_reader);
                                audio_processor.block_pos = 0;
                                let eager_end = block_size.min(num_frames - written);
                                audio_processor.eager_fill_clock(0, eager_end, written);
                            }

                            let scan_end =
                                block_size.min(audio_processor.block_pos + (num_frames - written));

                            // Resolve trigger sample for queued patch swap.
                            let trigger_sample: Option<usize> =
                                match audio_processor.queued_update.as_ref().map(|(_, t)| t) {
                                    Some(QueuedTrigger::Immediate) => {
                                        Some(audio_processor.block_pos)
                                    }
                                    Some(QueuedTrigger::NextBar) => audio_processor.scan_trigger(
                                        "barTrigger",
                                        audio_processor.block_pos,
                                        scan_end,
                                    ),
                                    Some(QueuedTrigger::NextBeat) => audio_processor.scan_trigger(
                                        "beatTrigger",
                                        audio_processor.block_pos,
                                        scan_end,
                                    ),
                                    None => None,
                                };

                            let end = trigger_sample.map(|n| n.min(scan_end)).unwrap_or(scan_end);

                            // Force every module to advance to `end`, in cache-efficient
                            // producer-before-consumer order, so modules not reachable from
                            // the output or any scope still process. The root/scope reads in
                            // the drain below then become pure cache hits. Skipped while
                            // stopped so the patch freezes on stop — the drain still ramps
                            // connected modules out via the root pull during the fade.
                            if end > audio_processor.block_pos && !audio_processor.is_stopped() {
                                audio_processor.process_all_modules_to(end);
                            }

                            // Drain [block_pos, end) into cpal output, inlining the
                            // volume-change state machine + soft clip that
                            // `FinalStateProcessor` used to provide per-frame.
                            if end > audio_processor.block_pos {
                                let mut scope_guard = audio_processor.scope_collection.try_lock();
                                let mut recording_guard = recording_feed.try_lock();
                                for i in audio_processor.block_pos..end {
                                    let is_stopped = audio_processor.is_stopped();
                                    match (final_state_processor.prev_is_stopped, is_stopped) {
                                        (true, false) => {
                                            final_state_processor.volume_change =
                                                VolumeChange::None;
                                            final_state_processor.attenuation_factor = 1.0;
                                        }
                                        (false, true) => {
                                            final_state_processor.volume_change =
                                                VolumeChange::Decrease;
                                        }
                                        _ => {}
                                    }
                                    final_state_processor.prev_is_stopped = is_stopped;
                                    if matches!(
                                        final_state_processor.volume_change,
                                        VolumeChange::Decrease
                                    ) {
                                        final_state_processor.attenuation_factor *= 0.999;
                                        if final_state_processor.attenuation_factor < 0.0001 {
                                            final_state_processor.attenuation_factor = 0.0;
                                            final_state_processor.volume_change =
                                                VolumeChange::None;
                                        }
                                    }

                                    let frame_start = written * num_channels;

                                    if final_state_processor.attenuation_factor < f32::EPSILON {
                                        for ch in 0..num_channels {
                                            output[frame_start + ch] = T::from_sample(0.0f32);
                                        }
                                        written += 1;
                                        continue;
                                    }

                                    if let Some(root) =
                                        audio_processor.patch.sampleables.get(&*ROOT_ID)
                                    {
                                        let mut any_audible = false;
                                        let mut samples = [0.0f32; PORT_MAX_CHANNELS];
                                        for ch in 0..num_channels.min(PORT_MAX_CHANNELS) {
                                            let raw = root.get_value_at(&ROOT_OUTPUT_PORT, ch, i)
                                                * AUDIO_OUTPUT_ATTENUATION;
                                            let sample = safety_soft_clip(
                                                raw * final_state_processor.attenuation_factor,
                                            );
                                            samples[ch] = sample;
                                            if sample.abs() >= 0.0005 {
                                                any_audible = true;
                                            }
                                        }
                                        if is_stopped && !any_audible {
                                            final_state_processor.attenuation_factor = 0.0;
                                            final_state_processor.volume_change =
                                                VolumeChange::None;
                                            for ch in 0..num_channels {
                                                output[frame_start + ch] = T::from_sample(0.0f32);
                                            }
                                        } else {
                                            for ch in 0..num_channels {
                                                let v = if ch < PORT_MAX_CHANNELS {
                                                    samples[ch]
                                                } else {
                                                    0.0
                                                };
                                                output[frame_start + ch] = T::from_sample(v);
                                            }
                                        }
                                        // Recorded samples stay f32 — the WAV
                                        // header declares Float32 regardless of
                                        // the stream's sample type `T`.
                                        if let Some(feed_lock) = recording_guard.as_mut()
                                            && let Some(feed) = feed_lock.as_mut()
                                        {
                                            let v = root.get_value_at(&ROOT_OUTPUT_PORT, 0, i)
                                                * AUDIO_OUTPUT_ATTENUATION
                                                * final_state_processor.attenuation_factor;
                                            feed.push(safety_soft_clip(v));
                                        }
                                        if let Some(scope_lock) = scope_guard.as_mut() {
                                            for (key, scope_buffer) in scope_lock.iter_mut() {
                                                if let Some(module) = audio_processor
                                                    .patch
                                                    .sampleables
                                                    .get(&key.module_id)
                                                {
                                                    let s = module.get_value_at(
                                                        &key.port_name,
                                                        key.channel as usize,
                                                        i,
                                                    );
                                                    scope_buffer.push(s);
                                                }
                                            }
                                        }
                                        for (key, xy_buffer) in &audio_processor.scope_xy_audio {
                                            let (Some(x_mod), Some(y_mod)) = (
                                                audio_processor
                                                    .patch
                                                    .sampleables
                                                    .get(&key.pair.x.module_id),
                                                audio_processor
                                                    .patch
                                                    .sampleables
                                                    .get(&key.pair.y.module_id),
                                            ) else {
                                                continue;
                                            };
                                            let xv = x_mod.get_value_at(
                                                &key.pair.x.port_name,
                                                key.pair.x.channel as usize,
                                                i,
                                            );
                                            let yv = y_mod.get_value_at(
                                                &key.pair.y.port_name,
                                                key.pair.y.channel as usize,
                                                i,
                                            );
                                            xy_buffer.push(xv, yv);
                                        }
                                    } else {
                                        for ch in 0..num_channels {
                                            output[frame_start + ch] = T::from_sample(0.0f32);
                                        }
                                    }

                                    written += 1;
                                }
                                // Publish each XY buffer's ring to its SeqLock region once per
                                // block so the main thread can read a coherent frame without a
                                // lock.
                                for (_key, xy_buffer) in &audio_processor.scope_xy_audio {
                                    xy_buffer.publish();
                                }
                                audio_processor.block_pos = end;
                            }

                            // Apply queued swap if we stopped at the trigger sample.
                            if trigger_sample == Some(audio_processor.block_pos)
                                && audio_processor.block_pos < block_size
                            {
                                let (update, _) = audio_processor.queued_update.take().unwrap();
                                let applied_id = update.update_id;
                                let swap_pos = audio_processor.block_pos;
                                audio_processor.apply_patch_update(update, swap_pos);
                                audio_processor
                                    .transport_meter
                                    .write_applied_update_id(applied_id);
                                // ROOT_CLOCK's transport state carries across the swap via
                                // `transfer_state_from`. Its cache
                                // for `[swap_pos, block_size)` was filled under the OLD params;
                                // refill the remainder of this callback's range from `swap_pos`
                                // forward under the NEW patch.
                                if let Some(root_clock) =
                                    audio_processor.patch.sampleables.get(&*ROOT_CLOCK_ID)
                                {
                                    root_clock.set_initial_index(swap_pos);
                                }
                                let eager_end = block_size
                                    .min(audio_processor.block_pos + (num_frames - written));
                                audio_processor.eager_fill_clock(swap_pos, eager_end, written);
                            }
                        }
                    }

                    // Transport meter — read at the last sample we just consumed.
                    {
                        let last = audio_processor.block_pos.saturating_sub(1);
                        let has_queued = audio_processor.queued_update.is_some();
                        if let Some(clock) = audio_processor.patch.sampleables.get(&*ROOT_CLOCK_ID)
                        {
                            let bar_phase = clock.get_value_at("playhead", 0, last) as f64;
                            let bar_count = clock.get_value_at("playhead", 1, last) as u64;
                            let beat_in_bar = clock.get_value_at("beatInBar", 0, last) as u32;
                            let is_playing = !audio_processor.is_stopped();
                            audio_processor.transport_meter.write_from_audio(
                                bar_phase,
                                bar_count,
                                beat_in_bar,
                                is_playing,
                                has_queued,
                            );
                        } else {
                            audio_processor
                                .transport_meter
                                .write_from_audio(0.0, 0, 0, false, has_queued);
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

                    // Retry any deferred profiler shared-map swap, then flush this
                    // callback's per-module profile data into the cross-thread
                    // snapshot. Both are no-ops when nothing is pending / profiling
                    // is disabled (early-out inside the calls).
                    audio_processor.retry_pending_profile_seed();
                    modular_core::profiling::flush_into(&audio_processor.module_profile_collection);

                    let elapsed_ns = callback_start.elapsed().as_nanos() as u64;

                    audio_budget_meter.record_chunk(output.len() as u64, elapsed_ns);
                }));
                if result.is_err() {
                    // Panic hook (panic_log::install_panic_hook) has already written
                    // a log file with payload + backtrace. Mark the audio thread as
                    // poisoned and emit silence for this and every future buffer.
                    panicked_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    for s in output.iter_mut() {
                        *s = T::from_sample(0.0);
                    }
                }
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
    let config = device.default_output_config().map_err(|e| {
        napi::Error::from_reason(format!("Failed to get default output config: {}", e))
    })?;
    Ok(config.sample_rate() as f32)
}

enum VolumeChange {
    Decrease,
    None,
}
/// Per-stream attenuation state for fade-in/fade-out on stop/start
/// transitions. Lives in the cpal closure and gets ticked once per
/// emitted sample by the per-sample drain loop.
struct FinalStateProcessor {
    attenuation_factor: f32,
    volume_change: VolumeChange,
    prev_is_stopped: bool,
}

impl FinalStateProcessor {
    fn new() -> Self {
        Self {
            attenuation_factor: 0.0,
            volume_change: VolumeChange::None,
            prev_is_stopped: true,
        }
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
        self.bar_phase_bits
            .store(bar_phase.to_bits(), Ordering::Relaxed);
        self.bar_count.store(bar_count, Ordering::Relaxed);
        self.beat_in_bar
            .store(beat_in_bar as u64, Ordering::Relaxed);
        self.is_playing.store(is_playing, Ordering::Relaxed);
        self.has_queued_update
            .store(has_queued_update, Ordering::Relaxed);
    }

    /// Write tempo and time signature. Called when params change (less frequently).
    #[inline]
    pub fn write_tempo(&self, bpm: f64, numerator: u32, denominator: u32) {
        self.bpm_bits.store(bpm.to_bits(), Ordering::Relaxed);
        self.time_sig_numerator
            .store(numerator as u64, Ordering::Relaxed);
        self.time_sig_denominator
            .store(denominator as u64, Ordering::Relaxed);
    }

    /// Record that the audio thread applied a patch update with this ID.
    #[inline]
    pub fn write_applied_update_id(&self, update_id: u64) {
        self.last_applied_update_id
            .store(update_id, Ordering::Relaxed);
    }

    /// Read the ID of the most recently applied patch update (used by the main
    /// thread to prune deferred MIDI disconnects once their update has applied).
    #[inline]
    pub fn read_applied_update_id(&self) -> u64 {
        self.last_applied_update_id.load(Ordering::Relaxed)
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
        self.link_phase_bits
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
    use modular_core::types::ModuleIdRemap;
    use modular_core::types::{Message, MessageHandler, MessageTag, MidiNoteOn};
    use std::sync::atomic::AtomicUsize;

    // ============================================================================
    // ScopeXyBuffer tests
    // ============================================================================

    #[test]
    fn test_scope_xy_snapshot_partial_fill_trims_zeros() {
        let buf = ScopeXyBuffer::new();
        buf.push(1.0, 10.0);
        buf.push(2.0, 20.0);
        buf.push(3.0, 30.0);
        buf.publish();
        let (x, y) = buf.snapshot();
        // Only the written samples are returned, chronological, no zero padding.
        assert_eq!(&x[..], &[1.0, 2.0, 3.0]);
        assert_eq!(&y[..], &[10.0, 20.0, 30.0]);
    }

    #[test]
    fn test_scope_xy_snapshot_full_ring_is_chronological() {
        let buf = ScopeXyBuffer::new();
        // Overflow the ring so it wraps; the newest CAPACITY samples are retained.
        let total = SCOPE_XY_CAPACITY + 5;
        for i in 0..total {
            buf.push(i as f32, 0.0);
        }
        buf.publish();
        let (x, _y) = buf.snapshot();
        assert_eq!(x.len(), SCOPE_XY_CAPACITY);
        // Element 0 is the oldest retained sample, last element the newest.
        assert_eq!(x[0], (total - SCOPE_XY_CAPACITY) as f32);
        assert_eq!(x[SCOPE_XY_CAPACITY - 1], (total - 1) as f32);
    }

    #[test]
    fn test_scope_xy_snapshot_empty() {
        let buf = ScopeXyBuffer::new();
        buf.publish();
        let (x, y) = buf.snapshot();
        assert!(x.is_empty());
        assert!(y.is_empty());
    }

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
            scope_xy_collection: Arc::new(Mutex::new(HashMap::new())),
            scope_xy_ranges: Arc::new(Mutex::new(None)),
            recording_feed: Arc::new(Mutex::new(None)),
            audio_budget_meter: Arc::new(AudioBudgetMeter::new()),
            module_states: Arc::new(Mutex::new(HashMap::new())),
            midi_manager: Arc::new(MidiInputManager::new()),
            transport_meter: Arc::new(TransportMeter::default()),
            audio_thread_panicked: Arc::new(AtomicBool::new(false)),
            module_profile_collection: modular_core::profiling::new_collection(),
        };

        let processor = AudioProcessor::new(
            cmd_consumer,
            err_producer,
            garbage_producer,
            shared,
            44100.0,
            1,
        );

        // Processor starts with empty patch (may have hidden audio_in)
        assert!(processor.patch.sampleables.is_empty() || processor.patch.sampleables.len() == 1);
    }

    /// Test double for the whole-patch swap.
    struct MockModule {
        label: String,
        current_id: String,
        state: std::cell::Cell<f32>,
        /// The last `swap_pos` passed to `set_initial_index`, so tests can assert
        /// a mid-block swap moves every module's per-block cursor.
        last_initial_index: std::cell::Cell<Option<usize>>,
    }

    impl MockModule {
        fn new(label: &str) -> Box<dyn modular_core::types::Sampleable> {
            Self::tagged(label, 0.0)
        }
        fn tagged(label: &str, state: f32) -> Box<dyn modular_core::types::Sampleable> {
            Box::new(Self {
                label: label.to_string(),
                current_id: label.to_string(),
                state: std::cell::Cell::new(state),
                last_initial_index: std::cell::Cell::new(None),
            })
        }
    }

    impl modular_core::types::MessageHandler for MockModule {}

    impl modular_core::types::Sampleable for MockModule {
        fn get_id(&self) -> &str {
            &self.current_id
        }
        fn get_module_type(&self) -> &str {
            &self.label
        }
        fn connect(&self, _patch: &modular_core::patch::Patch) {}
        fn transfer_state_from(&self, old: &dyn modular_core::types::Sampleable) {
            if let Some(old) = old.as_any().downcast_ref::<MockModule>() {
                self.state.set(old.state.get());
            }
        }
        fn set_initial_index(&self, slot: usize) {
            self.last_initial_index.set(Some(slot));
        }
        fn start_block(&self) {}
        fn ensure_processed_to(&self, _target: usize) {}
        fn ensure_processed(&self) {}
        fn get_value_at(&self, _port: &str, _ch: usize, _index: usize) -> f32 {
            self.state.get()
        }
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
        fn get_module_type(&self) -> &str {
            &self.label
        }
        fn connect(&self, _patch: &modular_core::patch::Patch) {}
        fn start_block(&self) {}
        fn ensure_processed_to(&self, _target: usize) {}
        fn ensure_processed(&self) {}
        fn get_value_at(&self, _port: &str, _ch: usize, _index: usize) -> f32 {
            0.0
        }
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
        fn get_module_type(&self) -> &str {
            "constant-output"
        }
        fn connect(&self, _patch: &modular_core::patch::Patch) {}
        fn start_block(&self) {}
        fn ensure_processed_to(&self, _target: usize) {}
        fn ensure_processed(&self) {}
        fn get_value_at(&self, _port: &str, _ch: usize, _index: usize) -> f32 {
            self.value
        }
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
        fn start_block(&self) {}
        fn ensure_processed_to(&self, _target: usize) {}
        fn ensure_processed(&self) {}
        fn get_value_at(&self, _port: &str, _ch: usize, _index: usize) -> f32 {
            self.cached_signal.lock().get_value()
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// A module whose `ensure_processed_to` bumps a shared counter, so tests can
    /// verify the audio thread force-processes it even with no connections.
    struct CountingProcessModule {
        current_id: String,
        processed: Arc<AtomicUsize>,
    }

    impl CountingProcessModule {
        fn new(id: &str, processed: Arc<AtomicUsize>) -> Box<dyn modular_core::types::Sampleable> {
            Box::new(Self {
                current_id: id.to_string(),
                processed,
            })
        }
    }

    impl MessageHandler for CountingProcessModule {}

    impl modular_core::types::Sampleable for CountingProcessModule {
        fn get_id(&self) -> &str {
            &self.current_id
        }
        fn get_module_type(&self) -> &str {
            "counting-process"
        }
        fn connect(&self, _patch: &modular_core::patch::Patch) {}
        fn start_block(&self) {}
        fn ensure_processed_to(&self, _target: usize) {
            self.processed.fetch_add(1, Ordering::SeqCst);
        }
        fn ensure_processed(&self) {
            self.ensure_processed_to(1);
        }
        fn get_value_at(&self, _port: &str, _ch: usize, _index: usize) -> f32 {
            0.0
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
            scope_xy_collection: Arc::new(Mutex::new(HashMap::new())),
            scope_xy_ranges: Arc::new(Mutex::new(None)),
            recording_feed: Arc::new(Mutex::new(None)),
            audio_budget_meter: Arc::new(AudioBudgetMeter::new()),
            module_states: Arc::new(Mutex::new(HashMap::new())),
            midi_manager: Arc::new(MidiInputManager::new()),
            transport_meter: Arc::new(TransportMeter::default()),
            audio_thread_panicked: Arc::new(AtomicBool::new(false)),
            module_profile_collection: modular_core::profiling::new_collection(),
        };

        let processor = AudioProcessor::new(
            cmd_consumer,
            err_producer,
            garbage_producer,
            shared,
            44100.0,
            1,
        );

        (cmd_producer, processor)
    }

    /// Build a `PatchUpdate` whose `new_patch` holds the given `(id, module)`
    /// pairs (plus the implicit HIDDEN_AUDIO_IN), message listeners registered —
    /// mirroring the fully-built, unconnected patch the main thread hands the
    /// audio thread.
    fn update_with(
        sample_rate: f32,
        modules: Vec<(&str, Box<dyn modular_core::types::Sampleable>)>,
    ) -> PatchUpdate {
        let mut update = PatchUpdate::new(sample_rate);
        for (id, module) in modules {
            update.new_patch.sampleables.insert(id.to_string(), module);
        }
        update.new_patch.rebuild_message_listeners();
        update
    }

    #[test]
    fn remap_chain_transfers_state_to_renamed_ids() {
        // A whole-patch swap with a remap chain (cycle-2→cycle-3, cycle-3→cycle-4)
        // transfers each outgoing module's runtime state to the module at its
        // renamed id. The reverse lookup reads the old patch and never mutates it,
        // so chained renames resolve correctly regardless of iteration order.
        let (_cmd_producer, mut processor) = create_test_processor();

        // Old patch: tagged modules at the source ids.
        processor
            .patch
            .sampleables
            .insert("cycle-2".into(), MockModule::tagged("shift", 22.0));
        processor
            .patch
            .sampleables
            .insert("cycle-3".into(), MockModule::tagged("thirds", 33.0));

        // New patch: freshly-built modules at the destination ids (state 0).
        let mut update = update_with(
            48000.0,
            vec![
                ("cycle-3", MockModule::tagged("new-cycle-3", 0.0)),
                ("cycle-4", MockModule::tagged("new-cycle-4", 0.0)),
            ],
        );
        update.set_remaps(&[
            ModuleIdRemap {
                from: "cycle-2".into(),
                to: "cycle-3".into(),
            },
            ModuleIdRemap {
                from: "cycle-3".into(),
                to: "cycle-4".into(),
            },
        ]);

        processor.apply_patch_update(update, 0);

        // Identity comes from the new patch; only state followed the rename.
        let c3 = processor.patch.sampleables.get("cycle-3").unwrap();
        assert_eq!(c3.get_module_type(), "new-cycle-3");
        assert_eq!(
            c3.get_value_at("", 0, 0),
            22.0,
            "cycle-3 inherits cycle-2's state"
        );
        let c4 = processor.patch.sampleables.get("cycle-4").unwrap();
        assert_eq!(c4.get_module_type(), "new-cycle-4");
        assert_eq!(
            c4.get_value_at("", 0, 0),
            33.0,
            "cycle-4 inherits cycle-3's state"
        );
        // The old source id is gone — the whole patch was replaced wholesale.
        assert!(!processor.patch.sampleables.contains_key("cycle-2"));
    }

    #[test]
    fn remap_swap_crosses_state_between_both_ids() {
        // A simultaneous swap (osc-1→osc-2, osc-2→osc-1): each new module pulls
        // state from the other id's outgoing module.
        let (_cmd_producer, mut processor) = create_test_processor();

        processor
            .patch
            .sampleables
            .insert("osc-1".into(), MockModule::tagged("alpha", 1.0));
        processor
            .patch
            .sampleables
            .insert("osc-2".into(), MockModule::tagged("beta", 2.0));

        let mut update = update_with(
            48000.0,
            vec![
                ("osc-1", MockModule::tagged("new-osc-1", 0.0)),
                ("osc-2", MockModule::tagged("new-osc-2", 0.0)),
            ],
        );
        update.set_remaps(&[
            ModuleIdRemap {
                from: "osc-1".into(),
                to: "osc-2".into(),
            },
            ModuleIdRemap {
                from: "osc-2".into(),
                to: "osc-1".into(),
            },
        ]);

        processor.apply_patch_update(update, 0);

        assert_eq!(
            processor
                .patch
                .sampleables
                .get("osc-1")
                .unwrap()
                .get_value_at("", 0, 0),
            2.0,
            "osc-1 inherits the old osc-2's state"
        );
        assert_eq!(
            processor
                .patch
                .sampleables
                .get("osc-2")
                .unwrap()
                .get_value_at("", 0, 0),
            1.0,
            "osc-2 inherits the old osc-1's state"
        );
    }

    #[test]
    fn remap_simple_rename_carries_state() {
        // Single rename vca-1→vca-2: the new module at vca-2 inherits vca-1's
        // runtime state, and the old id disappears.
        let (_cmd_producer, mut processor) = create_test_processor();

        processor
            .patch
            .sampleables
            .insert("vca-1".into(), MockModule::tagged("my-vca", 7.0));

        let mut update = update_with(
            48000.0,
            vec![("vca-2", MockModule::tagged("renamed-vca", 0.0))],
        );
        update.set_remaps(&[ModuleIdRemap {
            from: "vca-1".into(),
            to: "vca-2".into(),
        }]);

        processor.apply_patch_update(update, 0);

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
                .get_value_at("", 0, 0),
            7.0,
            "vca-2 inherits vca-1's state across the rename"
        );
    }

    #[test]
    fn remap_shift_chain_does_not_double_read_a_reused_id() {
        // Adding a same-type module ahead of existing ones recycles auto-generated
        // ids: the similarity matcher emits a shift chain (a→b, b→c) while a fresh
        // module lands on the vacated head id `a`. Each outgoing module's state
        // must transfer to exactly one new module — the fresh `a` must NOT steal
        // the state the rename moved to `b`.
        let (_cmd_producer, mut processor) = create_test_processor();

        processor
            .patch
            .sampleables
            .insert("a".into(), MockModule::tagged("old-a", 1.0));
        processor
            .patch
            .sampleables
            .insert("b".into(), MockModule::tagged("old-b", 2.0));

        let mut update = update_with(
            48000.0,
            vec![
                ("a", MockModule::tagged("fresh-a", 0.0)),
                ("b", MockModule::tagged("new-b", 0.0)),
                ("c", MockModule::tagged("new-c", 0.0)),
            ],
        );
        update.set_remaps(&[
            ModuleIdRemap {
                from: "a".into(),
                to: "b".into(),
            },
            ModuleIdRemap {
                from: "b".into(),
                to: "c".into(),
            },
        ]);

        processor.apply_patch_update(update, 0);

        let value = |id: &str| {
            processor
                .patch
                .sampleables
                .get(id)
                .unwrap()
                .get_value_at("", 0, 0)
        };
        assert_eq!(value("b"), 1.0, "b inherits the renamed old-a's state");
        assert_eq!(value("c"), 2.0, "c inherits the renamed old-b's state");
        assert_eq!(
            value("a"),
            0.0,
            "the fresh module reusing id `a` must NOT steal old-a's state"
        );
    }

    #[test]
    fn swap_keeps_hidden_audio_in_present() {
        use modular_core::types::WellKnownModule;
        let (_cmd_producer, mut processor) = create_test_processor();

        // A whole-patch swap replaces the patch wholesale; the reserved
        // HIDDEN_AUDIO_IN must still be present afterward (Patch::new injects it)
        // so per-block input injection keeps targeting it by id.
        let update = update_with(48000.0, vec![("osc", MockModule::tagged("osc", 0.0))]);
        processor.apply_patch_update(update, 0);

        assert!(
            processor
                .patch
                .sampleables
                .contains_key(WellKnownModule::HiddenAudioIn.id()),
            "HIDDEN_AUDIO_IN must survive the swap"
        );
    }

    #[test]
    fn swap_preserves_injected_audio_input() {
        use modular_core::types::WellKnownModule;
        let (_cmd_producer, mut processor) = create_test_processor();

        // Inject a host-input frame into the live HIDDEN_AUDIO_IN.
        let mut block = [[0.0f32; PORT_MAX_CHANNELS]; 1];
        block[0][0] = 0.42;
        processor
            .patch
            .sampleables
            .get(WellKnownModule::HiddenAudioIn.id())
            .unwrap()
            .inject_audio_in_block(&block);

        // The swap installs a fresh HIDDEN_AUDIO_IN; `transfer_state_from` must
        // carry the injected frame across so consumers don't read silence for
        // the remainder of the block.
        let update = update_with(48000.0, vec![("osc", MockModule::tagged("osc", 0.0))]);
        processor.apply_patch_update(update, 0);

        assert_eq!(
            processor
                .patch
                .sampleables
                .get(WellKnownModule::HiddenAudioIn.id())
                .unwrap()
                .get_value_at("", 0, 0),
            0.42,
            "injected host input survives the whole-patch swap"
        );
    }

    #[test]
    fn mid_block_swap_sets_every_module_cursor() {
        let (_cmd_producer, mut processor) = create_test_processor();

        // A swap at a non-zero in-block position moves every new module's
        // per-block cursor to `swap_pos` so slots [0, swap_pos) — already emitted
        // by the pre-swap patch — are not re-filled.
        let update = update_with(
            48000.0,
            vec![
                ("a", MockModule::tagged("a", 0.0)),
                ("b", MockModule::tagged("b", 0.0)),
            ],
        );
        processor.apply_patch_update(update, 3);

        for id in ["a", "b"] {
            let recorded = processor
                .patch
                .sampleables
                .get(id)
                .unwrap()
                .as_any()
                .downcast_ref::<MockModule>()
                .unwrap()
                .last_initial_index
                .get();
            assert_eq!(
                recorded,
                Some(3),
                "module {id} cursor should advance to swap_pos"
            );
        }
    }

    /// Construct a fresh, unstarted ROOT_CLOCK at 120 BPM 4/4 (phase 0). The
    /// caller inserts it into a patch; a whole-patch swap then carries any
    /// running clock's phase into it via `transfer_state_from`.
    fn build_root_clock(block_size: usize) -> Box<dyn modular_core::types::Sampleable> {
        let id = modular_core::types::ROOT_CLOCK_ID.to_string();
        let constructors = modular_core::dsp::get_constructors();
        let constructor = constructors.get("_clock").expect("_clock constructor");
        let params = crate::deserialize_params(
            "_clock",
            serde_json::json!({ "tempo": 120.0, "numerator": 4, "denominator": 4 }),
            true,
        )
        .expect("deserialize clock params");
        constructor(
            &id,
            44_100.0,
            params,
            block_size,
            modular_core::types::ProcessingMode::Sample,
        )
        .expect("construct clock")
    }

    /// Build a real running ROOT_CLOCK, insert it into the live patch, register it
    /// as a message listener, and start it from phase 0. Returns its module id.
    fn insert_running_root_clock(processor: &mut AudioProcessor) -> String {
        let id = modular_core::types::ROOT_CLOCK_ID.to_string();
        let clock = build_root_clock(processor.block_size);
        processor.patch.sampleables.insert(id.clone(), clock);
        processor.patch.add_message_listeners_for_module(&id);
        processor
            .patch
            .dispatch_message(&Message::Clock(ClockMessages::Start))
            .expect("start clock");
        id
    }

    /// Advance the clock by `samples` and return its resulting bar phase. The test
    /// processor uses `block_size == 1`, so each block is one `update`.
    fn advance_clock(processor: &AudioProcessor, id: &str, samples: usize) -> f32 {
        let clock = processor.patch.sampleables.get(id).unwrap();
        let mut phase = 0.0;
        for _ in 0..samples {
            clock.start_block();
            phase = clock.get_value_at("playhead", 0, 0);
        }
        phase
    }

    #[test]
    fn apply_patch_update_with_reset_clock_restarts_transport() {
        let (_cmd_producer, mut processor) = create_test_processor();
        let id = insert_running_root_clock(&mut processor);

        // Run well into the bar so the phase is unambiguously non-zero.
        let phase_before = advance_clock(&processor, &id, 20_000);
        assert!(
            phase_before > 0.1,
            "clock should have advanced before reset, got {phase_before}"
        );

        // The new patch carries a fresh clock at the same id; the swap transfers
        // the running phase into it, then reset_clock restarts the transport.
        let mut update = update_with(
            44_100.0,
            vec![(id.as_str(), build_root_clock(processor.block_size))],
        );
        update.reset_clock = true;
        processor.apply_patch_update(update, 0);

        // Re-fill the block from the top (mirrors the swap-site eager-fill) and
        // confirm the transport restarted near zero.
        let phase_after = advance_clock(&processor, &id, 1);
        assert!(
            phase_after < 0.001,
            "transport should restart at ~0 after reset, got {phase_after}"
        );
    }

    #[test]
    fn apply_patch_update_without_reset_clock_keeps_transport_running() {
        let (_cmd_producer, mut processor) = create_test_processor();
        let id = insert_running_root_clock(&mut processor);

        let phase_before = advance_clock(&processor, &id, 20_000);
        assert!(
            phase_before > 0.1,
            "clock should have advanced, got {phase_before}"
        );

        // The new patch's fresh clock inherits the running phase via
        // transfer_state_from (ROOT_CLOCK matched by its constant id); with no
        // reset, the transport keeps advancing from where it was.
        let update = update_with(
            44_100.0,
            vec![(id.as_str(), build_root_clock(processor.block_size))],
        );
        processor.apply_patch_update(update, 0);

        let phase_after = advance_clock(&processor, &id, 1);
        assert!(
            phase_after > phase_before,
            "transport should keep running (no reset): before={phase_before} after={phase_after}"
        );
    }

    #[test]
    fn superseded_queued_clock_reset_is_inherited() {
        // A buffer-switch update (reset_clock=true) queued for the next bar, then
        // superseded by a same-buffer re-eval (reset_clock=false) before it fires,
        // must still carry the pending transport reset into the immediate apply.
        let (mut cmd_producer, mut processor) = create_test_processor();

        let mut switch_update = PatchUpdate::new(44_100.0);
        switch_update.reset_clock = true;
        cmd_producer
            .push(GraphCommand::QueuedPatchUpdate {
                update: switch_update,
                trigger: QueuedTrigger::NextBar,
            })
            .unwrap();

        let mut reeval_update = PatchUpdate::new(44_100.0);
        reeval_update.reset_clock = false;
        cmd_producer
            .push(GraphCommand::QueuedPatchUpdate {
                update: reeval_update,
                trigger: QueuedTrigger::NextBar,
            })
            .unwrap();

        processor.process_commands();

        let (queued, trigger) = processor
            .queued_update
            .as_ref()
            .expect("an update should remain queued");
        assert!(
            queued.reset_clock,
            "pending clock reset must survive being superseded"
        );
        assert!(
            matches!(trigger, QueuedTrigger::Immediate),
            "superseding update applies immediately"
        );
    }

    #[test]
    fn queued_update_defers_meter_and_tempo_to_apply() {
        // A queued (NextBar) update must not touch the transport meter or Link
        // tempo at queue time — both are carried on the update and applied later.
        let (mut cmd_producer, mut processor) = create_test_processor();

        let mut update = PatchUpdate::new(44_100.0);
        update.transport_meta = Some(TransportMeta {
            tempo: 90.0,
            numerator: 3,
            denominator: 4,
            tempo_set: true,
        });
        cmd_producer
            .push(GraphCommand::QueuedPatchUpdate {
                update,
                trigger: QueuedTrigger::NextBar,
            })
            .unwrap();

        processor.process_commands();

        // Meter still shows the defaults — nothing written at queue time.
        let snap = processor.transport_meter.snapshot();
        assert_eq!(snap.bpm, 120.0, "meter tempo must not change until apply");
        assert_eq!(snap.time_sig_numerator, 4);
        assert_eq!(snap.time_sig_denominator, 4);

        // The tempo is held on the queued update for deferred apply (the only
        // observable proof the Link push is deferred — set_tempo_now no-ops here).
        let (queued, trigger) = processor
            .queued_update
            .as_ref()
            .expect("an update should remain queued");
        assert!(matches!(trigger, QueuedTrigger::NextBar));
        let meta = queued
            .transport_meta
            .as_ref()
            .expect("transport_meta carried on queued update");
        assert_eq!(meta.tempo, 90.0);
        assert!(meta.tempo_set);
    }

    #[test]
    fn apply_patch_update_writes_meter() {
        let (_cmd_producer, mut processor) = create_test_processor();
        let mut update = PatchUpdate::new(44_100.0);
        update.transport_meta = Some(TransportMeta {
            tempo: 90.0,
            numerator: 3,
            denominator: 4,
            tempo_set: false,
        });
        processor.apply_patch_update(update, 0);

        let snap = processor.transport_meter.snapshot();
        assert_eq!(snap.bpm, 90.0);
        assert_eq!(snap.time_sig_numerator, 3);
        assert_eq!(snap.time_sig_denominator, 4);
    }

    #[test]
    fn apply_patch_update_without_transport_meta_leaves_meter() {
        let (_cmd_producer, mut processor) = create_test_processor();
        // transport_meta defaults to None (patch with no ROOT_CLOCK).
        let update = PatchUpdate::new(44_100.0);
        processor.apply_patch_update(update, 0);

        let snap = processor.transport_meter.snapshot();
        assert_eq!(snap.bpm, 120.0);
        assert_eq!(snap.time_sig_numerator, 4);
        assert_eq!(snap.time_sig_denominator, 4);
    }

    #[test]
    fn queued_update_defers_scope_xy_ranges() {
        let (mut cmd_producer, mut processor) = create_test_processor();
        let ranges = ScopeXyRanges {
            x_min: -1.0,
            x_max: 1.0,
            y_min: -2.0,
            y_max: 2.0,
        };

        let mut update = PatchUpdate::new(44_100.0);
        update.scope_xy_ranges = Some(ranges);
        cmd_producer
            .push(GraphCommand::QueuedPatchUpdate {
                update,
                trigger: QueuedTrigger::NextBar,
            })
            .unwrap();

        processor.process_commands();

        // Display window not published at queue time; carried for deferred apply.
        assert_eq!(*processor.scope_xy_ranges.lock(), None);
        let (queued, _) = processor.queued_update.as_ref().expect("queued update");
        assert_eq!(queued.scope_xy_ranges, Some(ranges));
    }

    #[test]
    fn apply_patch_update_writes_scope_xy_ranges() {
        let (_cmd_producer, mut processor) = create_test_processor();
        let ranges = ScopeXyRanges {
            x_min: -1.0,
            x_max: 1.0,
            y_min: -2.0,
            y_max: 2.0,
        };
        let mut update = PatchUpdate::new(44_100.0);
        update.scope_xy_ranges = Some(ranges);
        processor.apply_patch_update(update, 0);

        assert_eq!(*processor.scope_xy_ranges.lock(), Some(ranges));
    }

    #[test]
    fn superseded_update_meter_not_written_and_latest_wins() {
        // Queue B(90, $setTempo) then C(80, $setTempo) before B fires. C supersedes
        // B and applies immediately; only C's tempo should win, and neither writes
        // the meter at queue time.
        let (mut cmd_producer, mut processor) = create_test_processor();

        let mut b = PatchUpdate::new(44_100.0);
        b.transport_meta = Some(TransportMeta {
            tempo: 90.0,
            numerator: 4,
            denominator: 4,
            tempo_set: true,
        });
        cmd_producer
            .push(GraphCommand::QueuedPatchUpdate {
                update: b,
                trigger: QueuedTrigger::NextBar,
            })
            .unwrap();

        let mut c = PatchUpdate::new(44_100.0);
        c.transport_meta = Some(TransportMeta {
            tempo: 80.0,
            numerator: 4,
            denominator: 4,
            tempo_set: true,
        });
        cmd_producer
            .push(GraphCommand::QueuedPatchUpdate {
                update: c,
                trigger: QueuedTrigger::NextBar,
            })
            .unwrap();

        processor.process_commands();

        assert_eq!(
            processor.transport_meter.snapshot().bpm,
            120.0,
            "neither queued update writes the meter at queue time"
        );
        let (queued, trigger) = processor
            .queued_update
            .as_ref()
            .expect("an update should remain queued");
        assert!(matches!(trigger, QueuedTrigger::Immediate));
        assert_eq!(
            queued.transport_meta.as_ref().expect("meta carried").tempo,
            80.0,
            "the superseding update's tempo wins; the discarded one is not merged"
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
            PatchUpdateSensitiveModule::new("dep", Signal::cable("src", "out", 0)),
        );

        processor.patch.connect_all();

        let initial = processor
            .patch
            .sampleables
            .get("dep")
            .unwrap()
            .get_value_at("out", 0, 0);
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
            .get_value_at("out", 0, 0);
        assert_eq!(updated, 7.25);
    }

    #[test]
    fn process_order_drives_disconnected_module() {
        // A module with no cables, feeding neither the output nor any scope, must
        // still be force-processed each block.
        let (_cmd_producer, mut processor) = create_test_processor();
        let processed = Arc::new(AtomicUsize::new(0));

        let mut update = update_with(
            48000.0,
            vec![(
                "iso",
                CountingProcessModule::new("iso", Arc::clone(&processed)),
            )],
        );
        update.process_order_ids = vec!["iso".into()];

        processor.apply_patch_update(update, 0);

        // apply_patch_update resolved the id order into a live pointer list.
        assert_eq!(processor.process_order.len(), 1);

        processor.process_all_modules_to(1);
        assert_eq!(processed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn process_order_skips_ids_absent_from_patch() {
        // A dangling id in the order (e.g. a cable target that isn't a module) is
        // skipped rather than resolved to a bogus pointer.
        let (_cmd_producer, mut processor) = create_test_processor();
        let processed = Arc::new(AtomicUsize::new(0));

        let mut update = update_with(
            48000.0,
            vec![(
                "real",
                CountingProcessModule::new("real", Arc::clone(&processed)),
            )],
        );
        update.process_order_ids = vec!["real".into(), "ghost".into()];

        processor.apply_patch_update(update, 0);

        // Only the real module yields a pointer; "ghost" is silently dropped.
        assert_eq!(processor.process_order.len(), 1);
        processor.process_all_modules_to(1);
        assert_eq!(processed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn process_order_repointed_after_single_module_update() {
        // SingleModuleUpdate moves the box, so the cached pointer must be rebuilt
        // to target the replacement, never the freed original.
        let (mut cmd_producer, mut processor) = create_test_processor();
        let old_processed = Arc::new(AtomicUsize::new(0));
        let new_processed = Arc::new(AtomicUsize::new(0));

        let mut update = update_with(
            48000.0,
            vec![(
                "m",
                CountingProcessModule::new("m", Arc::clone(&old_processed)),
            )],
        );
        update.process_order_ids = vec!["m".into()];
        processor.apply_patch_update(update, 0);

        cmd_producer
            .push(GraphCommand::SingleModuleUpdate {
                module_id: "m".into(),
                module: CountingProcessModule::new("m", Arc::clone(&new_processed)),
            })
            .unwrap();
        processor.process_commands();

        processor.process_all_modules_to(1);
        assert_eq!(old_processed.load(Ordering::SeqCst), 0);
        assert_eq!(new_processed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn process_order_cleared_on_clear_patch() {
        let (mut cmd_producer, mut processor) = create_test_processor();

        let mut update = update_with(
            48000.0,
            vec![(
                "m",
                CountingProcessModule::new("m", Arc::new(AtomicUsize::new(0))),
            )],
        );
        update.process_order_ids = vec!["m".into()];
        processor.apply_patch_update(update, 0);
        assert_eq!(processor.process_order.len(), 1);

        cmd_producer
            .push(GraphCommand::ClearPatch {
                fresh_patch: Patch::new(),
            })
            .unwrap();
        processor.process_commands();

        assert!(processor.process_order.is_empty());
        assert!(processor.process_order_ids.is_empty());
    }

    #[test]
    fn remap_routes_messages_to_the_new_id() {
        // After a whole-patch swap with a rename old-id→new-id, the module that
        // now occupies new-id (built into the new patch with its listeners
        // registered) receives dispatched messages; old-id is gone.
        let (_cmd_producer, mut processor) = create_test_processor();

        let old_hits = Arc::new(AtomicUsize::new(0));
        processor.patch.sampleables.insert(
            "old-id".into(),
            CountingMessageModule::new("old-id", Arc::clone(&old_hits)),
        );
        processor.patch.rebuild_message_listeners();

        let new_hits = Arc::new(AtomicUsize::new(0));
        let mut update = update_with(
            48000.0,
            vec![(
                "new-id",
                CountingMessageModule::new("new-id", Arc::clone(&new_hits)),
            )],
        );
        update.set_remaps(&[ModuleIdRemap {
            from: "old-id".into(),
            to: "new-id".into(),
        }]);

        processor.apply_patch_update(update, 0);

        let message = Message::MidiNoteOn(MidiNoteOn {
            device: None,
            channel: 0,
            note: 60,
            velocity: 100,
        });
        processor.patch.dispatch_message(&message).unwrap();

        assert_eq!(
            new_hits.load(Ordering::SeqCst),
            1,
            "the module at the new id receives the message"
        );
        assert_eq!(old_hits.load(Ordering::SeqCst), 0, "old id is gone");
        assert!(!processor.patch.sampleables.contains_key("old-id"));
        assert!(processor.patch.sampleables.contains_key("new-id"));
    }

    // ============================================================================
    // Scope collection swap tests
    // ============================================================================

    fn scope_key(module_id: &str) -> ScopeBufferKey {
        ScopeBufferKey {
            module_id: module_id.into(),
            port_name: "output".into(),
            channel: 0,
            ms_per_frame: 100,
            trigger_threshold: None,
        }
    }

    #[test]
    fn scope_swap_membership_is_exactly_the_last_applied_updates_set() {
        // Two updates built back-to-back before either applies: each carries
        // its complete desired membership, so after both apply the collection
        // holds exactly the later update's set — no orphan from the first, no
        // dependence on the collection state at build time.
        let (_cmd_producer, mut processor) = create_test_processor();
        let key1 = scope_key("m1");
        let key2 = scope_key("m2");

        let mut u1 = PatchUpdate::new(44_100.0);
        u1.scope_next
            .insert(key1.clone(), ScopeBuffer::new(100, None, 44_100.0));
        let mut u2 = PatchUpdate::new(44_100.0);
        u2.scope_next
            .insert(key2.clone(), ScopeBuffer::new(100, None, 44_100.0));

        processor.apply_patch_update(u1, 0);
        processor.apply_patch_update(u2, 0);

        let collection = processor.scope_collection.lock();
        assert!(collection.contains_key(&key2));
        assert!(
            !collection.contains_key(&key1),
            "a key absent from the applied update's membership must not linger"
        );
        assert_eq!(collection.len(), 1);
    }

    #[test]
    fn scope_swap_carries_live_buffer_state_for_kept_keys() {
        // A key present in both the live collection and the incoming
        // membership keeps its buffer contents across the swap, so an
        // unchanged scope's display never blanks on a patch edit.
        let (_cmd_producer, mut processor) = create_test_processor();
        let kept = scope_key("kept");
        let added = scope_key("added");

        let mut u1 = PatchUpdate::new(44_100.0);
        u1.scope_next
            .insert(kept.clone(), ScopeBuffer::new(100, None, 44_100.0));
        processor.apply_patch_update(u1, 0);
        processor
            .scope_collection
            .lock()
            .get_mut(&kept)
            .unwrap()
            .push(0.75);

        let mut u2 = PatchUpdate::new(44_100.0);
        u2.scope_next
            .insert(kept.clone(), ScopeBuffer::new(100, None, 44_100.0));
        u2.scope_next
            .insert(added.clone(), ScopeBuffer::new(100, None, 44_100.0));
        processor.apply_patch_update(u2, 0);

        let collection = processor.scope_collection.lock();
        assert_eq!(
            collection.get(&kept).unwrap().get_buffer()[0],
            0.75,
            "a kept scope's buffer state survives the swap"
        );
        assert!(collection.contains_key(&added));
    }

    // ============================================================================
    // Module-state metadata cache tests
    // ============================================================================

    #[derive(Clone)]
    struct TestLiveState;

    impl ModuleLiveState for TestLiveState {
        fn reset(&mut self) {}
        fn clone_box(&self) -> Box<dyn ModuleLiveState> {
            Box::new(self.clone())
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    struct TestStateMeta(&'static str);

    impl ModuleStateMeta for TestStateMeta {
        fn build_json(&self, _live: &dyn ModuleLiveState) -> serde_json::Value {
            serde_json::Value::String(self.0.into())
        }
    }

    fn pending_entry(
        id: u64,
        module: &str,
        tag: &'static str,
    ) -> (u64, HashMap<String, Box<dyn ModuleStateMeta>>) {
        let mut metas: HashMap<String, Box<dyn ModuleStateMeta>> = HashMap::new();
        metas.insert(module.into(), Box::new(TestStateMeta(tag)));
        (id, metas)
    }

    #[test]
    fn pending_meta_survives_supersession_until_its_update_applies() {
        // Update 1 is registered, then update 2 before 1 applies. Update 1's
        // quantized trigger can still fire before update 2's command is
        // popped, so its metadata must promote at watermark 1 — with update
        // 2's entry (and its pre-added slot) intact for the later swap.
        let mut cache = ModuleStateMetaCache::default();
        let mut states: ModuleStateMap = HashMap::new();
        states.insert("a".into(), Box::new(TestLiveState));
        cache.pending.push(pending_entry(1, "a", "one"));
        states.insert("b".into(), Box::new(TestLiveState));
        cache.pending.push(pending_entry(2, "b", "two"));

        cache.promote_if_applied(&mut states, 1);
        assert!(cache.live.contains_key("a"));
        assert!(!cache.live.contains_key("b"));
        assert_eq!(cache.pending.len(), 1);
        assert!(
            states.contains_key("b"),
            "a still-pending update's pre-added slot survives the prune"
        );

        cache.promote_if_applied(&mut states, 2);
        assert!(cache.live.contains_key("b"));
        assert!(!cache.live.contains_key("a"));
        assert!(!states.contains_key("a"));
        assert!(cache.pending.is_empty());
    }

    #[test]
    fn promotion_picks_newest_applied_pending_entry() {
        // A watermark covering several pendings promotes only the newest one;
        // the older entries were superseded before ever pairing with state.
        let mut cache = ModuleStateMetaCache::default();
        let mut states: ModuleStateMap = HashMap::new();
        states.insert("a".into(), Box::new(TestLiveState));
        states.insert("b".into(), Box::new(TestLiveState));
        cache.pending.push(pending_entry(1, "a", "one"));
        cache.pending.push(pending_entry(2, "b", "two"));

        cache.promote_if_applied(&mut states, 2);
        assert!(cache.live.contains_key("b"));
        assert!(!cache.live.contains_key("a"));
        assert!(!states.contains_key("a"));
        assert!(cache.pending.is_empty());
    }

    fn create_test_audio_state() -> AudioState {
        let (
            cmd_producer,
            _cmd_consumer,
            _err_producer,
            err_consumer,
            _garbage_producer,
            garbage_consumer,
        ) = create_audio_channels();
        AudioState::new_with_channels(
            cmd_producer,
            err_consumer,
            garbage_consumer,
            44_100.0,
            2,
            Arc::new(MidiInputManager::new()),
            1,
        )
    }

    #[test]
    fn module_states_poll_under_contention_serves_last_snapshot() {
        let state = create_test_audio_state();
        state
            .module_states
            .lock()
            .insert("m1".into(), Box::new(TestLiveState));
        state
            .module_state_meta
            .lock()
            .live
            .insert("m1".into(), Box::new(TestStateMeta("one")));

        let first = state.get_module_states();
        assert_eq!(
            first.get("m1"),
            Some(&serde_json::Value::String("one".into()))
        );

        // Hold the live-state lock as the audio thread does mid-callback: the
        // poll must serve the previous snapshot, never an empty map the
        // renderer would read as "all modules removed".
        let states_arc = Arc::clone(&state.module_states);
        let _audio_thread_guard = states_arc.lock();
        let contended = state.get_module_states();
        assert_eq!(contended, first);
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
