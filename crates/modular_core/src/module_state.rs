//! Generic per-module editor state, split so the audio thread never allocates.
//!
//! A module that wants to publish live state to the editor provides two halves:
//! a [`ModuleLiveState`] — a pre-allocated slot the audio thread mutates in place
//! each callback without allocating — and a [`ModuleStateMeta`] — immutable
//! metadata built once on the main thread from the patch params. On poll the main
//! thread pairs the two into the editor JSON. Modules register a
//! [`ModuleStateBuilder`] next to their own implementation (see
//! `crate::dsp::get_module_state_builders`); `$cycle` is the only one today.

use std::any::Any;

use serde_json::Value;

/// The audio-thread-written half of a module's editor state. The slot is
/// allocated once on the main thread (during the patch update) and the audio
/// thread only mutates it in place — it must never allocate. Stored boxed per
/// module id so different modules can publish different concrete state types.
pub trait ModuleLiveState: Send {
    /// Clear the live snapshot before the audio thread writes the next one.
    fn reset(&mut self);
    /// Clone into a fresh box. The poll path snapshots the live slots under a
    /// brief lock and builds the JSON after releasing it, so the audio thread's
    /// `try_lock` never fails across JSON construction.
    fn clone_box(&self) -> Box<dyn ModuleLiveState>;
    /// Downcast hook so a module's metadata can read back its own concrete live
    /// type when building JSON.
    fn as_any(&self) -> &dyn Any;
    /// Mutable downcast hook so a module's `write_module_state` can recover its
    /// own concrete live type from the type-erased slot.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// The main-thread half: immutable metadata built from the patch params, paired
/// with a live snapshot to produce the editor JSON on poll.
pub trait ModuleStateMeta: Send {
    /// Build the editor JSON for this module from its immutable metadata and a
    /// live snapshot. Implementors downcast `live` to their own
    /// [`ModuleLiveState`] type; a mismatch yields `Value::Null`.
    fn build_json(&self, live: &dyn ModuleLiveState) -> Value;
}

/// Builds a module's editor-state halves from its raw params JSON on the main
/// thread: the empty live slot the audio thread will fill, plus the immutable
/// metadata. Returns `None` for modules that publish no state for these params.
/// Registered per module type in `crate::dsp::get_module_state_builders`.
pub type ModuleStateBuilder =
    fn(&Value) -> Option<(Box<dyn ModuleLiveState>, Box<dyn ModuleStateMeta>)>;
