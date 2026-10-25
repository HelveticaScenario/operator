//! Audio input module - reads from the audio input ring buffer.
//!
//! This module allows reading audio from the system's audio input device.

use std::cell::UnsafeCell;

use crate::{
    Sampleable,
    poly::PORT_MAX_CHANNELS,
    types::{MessageHandler, WellKnownModule},
};

/// Upper bound on per-block input frames the module can store. Pre-allocated
/// once so the audio thread never touches the heap on `inject_audio_in`.
const AUDIO_IN_MAX_BLOCK: usize = 4096;

/// Hidden audio-input module. The audio callback pulls one CPAL frame per
/// slot of the current internal block into `block` via `inject_audio_in`,
/// and consumers read per-slot values via
/// `get_value_at(_, ch, slot)`.
pub struct AudioIn {
    /// Per-block input frames, one entry per slot, each holding all
    /// channels. Layout mirrors `BlockPort`: `block[sample_index][channel_index]`.
    ///
    /// Heap-allocated (`Box<[_]>`) so that constructing `AudioIn` does not
    /// push a 256 KB array through the stack: a stack-resident
    /// `[[f32; 16]; 4096]` temp would overflow a small thread stack's guard
    /// page → SIGBUS.
    ///
    /// # Safety
    ///
    /// Accessed only from the audio thread:
    ///   - Written during `inject_audio_in` (before the slots it fills are
    ///     processed).
    ///   - Read during `get_value_at` (inside module processing).
    /// These phases are serialised on the same thread — no concurrent access.
    block: UnsafeCell<Box<[[f32; PORT_MAX_CHANNELS]]>>,
    /// Number of valid samples in `block`: the slots of the current internal
    /// block injected so far.
    block_len: UnsafeCell<usize>,
}

fn make_empty_block() -> Box<[[f32; PORT_MAX_CHANNELS]]> {
    vec![[0.0f32; PORT_MAX_CHANNELS]; AUDIO_IN_MAX_BLOCK].into_boxed_slice()
}

impl Default for AudioIn {
    fn default() -> Self {
        Self {
            block: UnsafeCell::new(make_empty_block()),
            block_len: UnsafeCell::new(0),
        }
    }
}

// SAFETY: See `block` field documentation above.
unsafe impl Sync for AudioIn {}

impl Sampleable for AudioIn {
    fn get_id(&self) -> &str {
        WellKnownModule::HiddenAudioIn.id()
    }

    fn get_module_type(&self) -> &str {
        WellKnownModule::HiddenAudioIn.id()
    }

    fn connect(&self, _patch: &crate::Patch) {}

    fn start_block(&self) {}

    fn ensure_processed_to(&self, _target: usize) {}

    fn ensure_processed(&self) {}

    /// Store host input frames for slots `first_slot..`, clamped to
    /// `AUDIO_IN_MAX_BLOCK`.
    fn inject_audio_in(&self, first_slot: usize, frames: &[[f32; PORT_MAX_CHANNELS]]) {
        let end = (first_slot + frames.len()).min(AUDIO_IN_MAX_BLOCK);
        let Some(len) = end.checked_sub(first_slot) else {
            return;
        };
        unsafe {
            let stored = &mut *self.block.get();
            stored[first_slot..end].copy_from_slice(&frames[..len]);
            *self.block_len.get() = end;
        }
    }

    /// Per-slot read of the injected block. Returns 0.0 for out-of-range
    /// slot/channel indices, including when no block has been injected yet.
    fn get_value_at(&self, _port: &str, ch: usize, index: usize) -> f32 {
        let len = unsafe { *self.block_len.get() };
        if index >= len || ch >= PORT_MAX_CHANNELS {
            return 0.0;
        }
        unsafe { (*self.block.get())[index][ch] }
    }

    fn transfer_state_from(&self, old: &dyn Sampleable, _ids: &dyn crate::types::PreviousIds) {
        let Some(old) = old.as_any().downcast_ref::<AudioIn>() else {
            return;
        };
        if std::ptr::eq(self, old) {
            return;
        }
        unsafe {
            let len = *old.block_len.get();
            let src = &*old.block.get();
            let dst = &mut *self.block.get();
            dst[..len].copy_from_slice(&src[..len]);
            *self.block_len.get() = len;
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl MessageHandler for AudioIn {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injected_ranges_fill_their_own_slots() {
        let audio_in = AudioIn::default();
        let frame = |v: f32| {
            let mut f = [0.0; PORT_MAX_CHANNELS];
            f[0] = v;
            f
        };
        audio_in.inject_audio_in(0, &[frame(1.0), frame(2.0)]);
        audio_in.inject_audio_in(2, &[frame(3.0)]);
        let read = |slot| audio_in.get_value_at("output", 0, slot);
        assert_eq!([read(0), read(1), read(2), read(3)], [1.0, 2.0, 3.0, 0.0]);
    }
}
