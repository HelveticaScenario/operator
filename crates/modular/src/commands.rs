//! Command queue types for audio thread communication.
//!
//! This module defines the commands sent from the main thread to the audio thread,
//! and the errors reported back from the audio thread.

use modular_core::profiling::ModuleProfileAccum;
use modular_core::types::{
  Message, ModuleIdRemap, Sampleable, ScopeBufferKey, ScopeXyBufferKey, ScopeXyRanges, WavData,
};
use napi_derive::napi;
use std::collections::HashMap;
use std::sync::Arc;

use crate::audio::{ScopeBuffer, ScopeXyBuffer};
use crate::link::LinkResources;

/// When a queued patch update should be applied.
#[napi(string_enum)]
pub enum QueuedTrigger {
  /// Apply immediately (no waiting).
  Immediate,
  /// Apply at the start of the next bar (ROOT_CLOCK bar_trigger).
  NextBar,
  /// Apply at the next beat (ROOT_CLOCK beat_trigger).
  NextBeat,
}

/// Transport/meter values extracted from the patch's ROOT_CLOCK, carried to the
/// audio thread so the meter write and the Link tempo push happen at apply time,
/// atomically with the module swap. `None` when the patch has no ROOT_CLOCK.
pub struct TransportMeta {
  /// Tempo in BPM.
  pub tempo: f64,
  /// Time signature numerator (beats per bar).
  pub numerator: u32,
  /// Time signature denominator (beat value).
  pub denominator: u32,
  /// The DSL explicitly called `$setTempo` — push `tempo` to Link on apply.
  pub tempo_set: bool,
}

/// A single atomic patch update - always processed as a complete unit.
///
/// This struct ensures the audio thread receives a complete, consistent batch of changes.
/// The main thread computes the entire diff and sends it as one unit.
pub struct PatchUpdate {
  /// Unique ID for this update, used to track apply/discard on the audio thread.
  pub update_id: u64,

  /// Modules to insert (pre-constructed on main thread).
  pub inserts: Vec<(String, Box<dyn modular_core::types::Sampleable>)>,

  /// Set of desired module IDs, pre-computed on the main thread.
  /// Any existing module not in this set (and not reserved) is stale.
  pub desired_ids: std::collections::HashSet<String>,

  /// Module IDs in cache-efficient processing order (producers before the
  /// consumers that read them), computed on the main thread by
  /// `graph_analysis::analyze`. The audio thread resolves these to a
  /// contiguous pointer list it walks every block to force-process every
  /// module — including ones not reachable from the output or any scope.
  pub process_order_ids: Vec<String>,

  /// ID remappings (applied before inserts/deletes)
  pub remaps: Vec<ModuleIdRemap>,

  /// Pre-built scope buffers to add (constructed on main thread)
  pub scope_adds: Vec<(ScopeBufferKey, ScopeBuffer)>,

  /// Scopes to remove
  pub scope_removes: Vec<ScopeBufferKey>,

  /// Pre-built XY scope buffers to add (constructed on main thread)
  pub scope_xy_adds: Vec<(ScopeXyBufferKey, Arc<ScopeXyBuffer>)>,

  /// XY scope buffers to remove
  pub scope_xy_removes: Vec<ScopeXyBufferKey>,

  /// WAV data cache — cloned Arc<WavData> entries from the main-thread WavCache.
  /// Swapped into the Patch on the audio thread so Wav params can resolve during connect().
  pub wav_data: HashMap<String, Arc<WavData>>,

  /// Sample rate for new modules
  pub sample_rate: f32,

  /// Tempo/time-signature for the meter + Link, applied when this update is
  /// applied. `None` when the patch has no ROOT_CLOCK.
  pub transport_meta: Option<TransportMeta>,

  /// XY-scope display window, applied atomically with the XY scope
  /// buffer swap. `None` when the patch has no `$scopeXY` (clears the display).
  pub scope_xy_ranges: Option<ScopeXyRanges>,

  /// When true, restart ROOT_CLOCK's transport (phase, beat, bar count) to zero
  /// as this update is applied. Set by the main thread when the update switches
  /// playback to a different buffer (song); a same-buffer live-coding update
  /// leaves it false so the clock keeps running uninterrupted.
  pub reset_clock: bool,

  /// Pre-allocated TLS profiler records map, one entry per id in
  /// `desired_ids`. Consumed by `profiling::swap_records` on the audio
  /// thread; the evicted map flows back via the garbage queue. Always
  /// populated, even when profiling is disabled — the swap maintains
  /// the audio-thread allocation invariant for the next enable.
  pub profile_records_seed: HashMap<String, ModuleProfileAccum>,

  /// Pre-allocated map for the cross-thread `ModuleProfileCollection`,
  /// consumed by `profiling::try_swap_shared`. Same key set as
  /// `profile_records_seed`; held separately because each swap consumes
  /// its operand.
  pub profile_shared_seed: HashMap<String, ModuleProfileAccum>,
}

impl PatchUpdate {
  /// Create an empty patch update
  pub fn new(sample_rate: f32) -> Self {
    Self {
      update_id: 0,
      inserts: Vec::new(),
      desired_ids: std::collections::HashSet::new(),
      process_order_ids: Vec::new(),
      remaps: Vec::new(),
      scope_adds: Vec::new(),
      scope_removes: Vec::new(),
      scope_xy_adds: Vec::new(),
      scope_xy_removes: Vec::new(),
      wav_data: HashMap::new(),
      sample_rate,
      transport_meta: None,
      scope_xy_ranges: None,
      reset_clock: false,
      profile_records_seed: HashMap::new(),
      profile_shared_seed: HashMap::new(),
    }
  }

  /// Check if this update has any changes
  pub fn is_empty(&self) -> bool {
    self.inserts.is_empty()
      && self.desired_ids.is_empty()
      && self.remaps.is_empty()
      && self.scope_adds.is_empty()
      && self.scope_removes.is_empty()
      && self.scope_xy_adds.is_empty()
      && self.scope_xy_removes.is_empty()
  }
}

/// Commands sent to audio thread via the command queue.
pub enum GraphCommand {
  /// Queued patch update - stored and applied when the trigger condition is met.
  /// `Immediate` applies on the next frame; `NextBar`/`NextBeat` wait for
  /// ROOT_CLOCK's bar_trigger or beat_trigger output respectively.
  QueuedPatchUpdate {
    update: PatchUpdate,
    trigger: QueuedTrigger,
  },

  /// Lightweight single-module update (e.g., slider changes).
  /// The module is pre-constructed on the main thread; the audio thread
  /// does state transfer + replacement, then reconnects.
  SingleModuleUpdate {
    module_id: String,
    module: Box<dyn Sampleable>,
  },

  /// MIDI/control messages (can be sent individually)
  DispatchMessage(Message),

  /// Transport control: start playback
  Start,

  /// Transport control: stop playback
  Stop,

  /// Clear the entire patch (used when stopped to reset state)
  ClearPatch,

  /// Install or remove the live Ableton Link session.
  ///
  /// `Some(resources)` hands a fully constructed and enabled `AblLink`
  /// (alongside its `HostTimeFilter` and `SessionState`) to the audio thread.
  /// `None` tells the audio thread to relinquish its current resources.
  ///
  /// Construction, `enable()`, and drop of `AblLink` are documented as
  /// realtime-unsafe by Ableton and must run on the main thread. The audio
  /// thread only ever uses the RT-safe capture/commit/clock_micros API on
  /// the resources it holds. Old resources removed by this command are
  /// pushed to the garbage queue so the main thread can drop them safely.
  SetLink(Option<Box<LinkResources>>),
}

/// Error types that can be reported from the audio thread back to the main thread.
#[derive(Debug, Clone)]
pub enum AudioError {
  /// Failed to update module parameters
  ParamUpdateFailed { module_id: String, message: String },

  /// Failed to dispatch a message
  MessageDispatchFailed { message: String },

  /// Module not found when trying to perform an operation
  ModuleNotFound { module_id: String },

  /// Generic error during patch processing
  PatchProcessingError { message: String },
}

impl std::fmt::Display for AudioError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      AudioError::ParamUpdateFailed { module_id, message } => {
        write!(f, "Failed to update params for {}: {}", module_id, message)
      }
      AudioError::MessageDispatchFailed { message } => {
        write!(f, "Failed to dispatch message: {}", message)
      }
      AudioError::ModuleNotFound { module_id } => {
        write!(f, "Module not found: {}", module_id)
      }
      AudioError::PatchProcessingError { message } => {
        write!(f, "Patch processing error: {}", message)
      }
    }
  }
}

impl std::error::Error for AudioError {}

/// Capacity for the command queue (main → audio)
pub const COMMAND_QUEUE_CAPACITY: usize = 1024;

/// Capacity for the error queue (audio → main)
pub const ERROR_QUEUE_CAPACITY: usize = 256;

/// Items to be deallocated on the main thread instead of the audio thread.
/// The audio thread pushes removed modules here; the main thread drains and drops them.
/// Fields are intentionally never read — the value of this type is in its `Drop`.
#[allow(dead_code)]
pub enum GarbageItem {
  /// A module removed from the patch
  Module(Box<dyn Sampleable>),
  /// A scope buffer removed from the collection
  Scope(ScopeBuffer),
  /// An XY scope buffer removed from the collection
  ScopeXy(Arc<ScopeXyBuffer>),
  /// A queued patch update that was superseded by a newer update before it fired
  PatchUpdate(PatchUpdate),
  /// Live Link resources removed from the audio thread. Drop tears down
  /// internal networking threads and sockets — must happen on the main thread.
  Link(Box<LinkResources>),
  /// Profiler records map evicted by `swap_records` / `try_swap_shared`.
  /// Drops on the main thread so the `HashMap`'s bucket deallocation
  /// stays off the audio thread.
  ProfileMap(HashMap<String, ModuleProfileAccum>),
}

/// Capacity for the garbage queue (audio → main).
/// Generous to avoid blocking the audio thread if main thread is slow to drain.
pub const GARBAGE_QUEUE_CAPACITY: usize = 4096;
