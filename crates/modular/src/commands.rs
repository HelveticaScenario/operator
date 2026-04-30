//! Command queue types for audio thread communication.
//!
//! This module defines the commands sent from the main thread to the audio thread,
//! and the errors reported back from the audio thread.

use modular_core::types::{Message, ModuleIdRemap, Sampleable, ScopeBufferKey, WavData};
use napi_derive::napi;
use std::collections::HashMap;
use std::sync::Arc;

use crate::audio::ScopeBuffer;

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

  /// ID remappings (applied before inserts/deletes)
  pub remaps: Vec<ModuleIdRemap>,

  /// Pre-built scope buffers to add (constructed on main thread)
  pub scope_adds: Vec<(ScopeBufferKey, ScopeBuffer)>,

  /// Scopes to remove
  pub scope_removes: Vec<ScopeBufferKey>,

  /// WAV data cache — cloned Arc<WavData> entries from the main-thread WavCache.
  /// Swapped into the Patch on the audio thread so Wav params can resolve during connect().
  pub wav_data: HashMap<String, Arc<WavData>>,

  /// Sample rate for new modules
  pub sample_rate: f32,

  /// Whether the DSL explicitly called $setTempo (don't push default 120 to Link)
  pub tempo_override: Option<f64>,
}

impl PatchUpdate {
  /// Create an empty patch update
  pub fn new(sample_rate: f32) -> Self {
    Self {
      update_id: 0,
      inserts: Vec::new(),
      desired_ids: std::collections::HashSet::new(),
      remaps: Vec::new(),
      scope_adds: Vec::new(),
      scope_removes: Vec::new(),
      wav_data: HashMap::new(),
      sample_rate,
      tempo_override: None,
    }
  }

  /// Check if this update has any changes
  pub fn is_empty(&self) -> bool {
    self.inserts.is_empty()
      && self.desired_ids.is_empty()
      && self.remaps.is_empty()
      && self.scope_adds.is_empty()
      && self.scope_removes.is_empty()
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

  /// Enable or disable Ableton Link synchronization
  EnableLink(bool),
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
  /// A queued patch update that was superseded by a newer update before it fired
  PatchUpdate(PatchUpdate),
}

/// Capacity for the garbage queue (audio → main).
/// Generous to avoid blocking the audio thread if main thread is slow to drain.
pub const GARBAGE_QUEUE_CAPACITY: usize = 4096;
