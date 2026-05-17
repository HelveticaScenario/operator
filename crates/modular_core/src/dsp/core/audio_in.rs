//! Audio input module - reads from the audio input ring buffer.
//!
//! This module allows reading audio from the system's audio input device.

use std::cell::UnsafeCell;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::{
    Sampleable,
    poly::{PORT_MAX_CHANNELS, PolyOutput},
    types::{MessageHandler, WellKnownModule},
};

/// Upper bound on per-block input frames the module can store. Pre-allocated
/// once so the audio thread never touches the heap on `inject_audio_in_block`.
const AUDIO_IN_MAX_BLOCK: usize = 4096;

/// Hidden audio-input module. The audio callback pulls one CPAL frame per
/// slot of the current internal block into `block` via
/// `inject_audio_in_block`, and consumers read per-slot values via
/// `get_value_at(_, ch, slot)`.
pub struct AudioIn {
    /// Shared with `Patch::audio_in` so `Patch::insert_audio_in` can
    /// reconstruct the module pointing at the patch's `Arc<Mutex<PolyOutput>>`.
    /// Held but unused for sample reads now that `block` is the source of
    /// truth — kept for backwards-compat with existing `Patch` construction.
    pub input: Arc<Mutex<PolyOutput>>,
    /// Per-block input frames, one entry per slot, each holding all
    /// channels. Layout mirrors `BlockPort`: `block[sample_index][channel_index]`.
    ///
    /// Heap-allocated (`Box<[_]>`) so that constructing `AudioIn` does not
    /// push a 256 KB array through the stack. `Patch::insert_audio_in` runs
    /// on the CoreAudio IO thread (stack ≈ 512 KB), and a stack-resident
    /// `[[f32; 16]; 4096]` temp would overflow the guard page → SIGBUS.
    ///
    /// # Safety
    ///
    /// Accessed only from the audio thread:
    ///   - Written during `inject_audio_in_block` (block boundary, before
    ///     any module processing).
    ///   - Read during `get_value_at` (inside module processing).
    /// These phases are serialised on the same thread — no concurrent access.
    block: UnsafeCell<Box<[[f32; PORT_MAX_CHANNELS]]>>,
    /// Number of valid samples in `block` (= current internal block size).
    block_len: UnsafeCell<usize>,
}

fn make_empty_block() -> Box<[[f32; PORT_MAX_CHANNELS]]> {
    vec![[0.0f32; PORT_MAX_CHANNELS]; AUDIO_IN_MAX_BLOCK].into_boxed_slice()
}

impl Default for AudioIn {
    fn default() -> Self {
        Self {
            input: Arc::new(Mutex::new(PolyOutput::default())),
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

    /// Store one block's worth of host input frames. `block.len()` is
    /// clamped to `AUDIO_IN_MAX_BLOCK`.
    fn inject_audio_in_block(&self, block: &[[f32; PORT_MAX_CHANNELS]]) {
        let len = block.len().min(AUDIO_IN_MAX_BLOCK);
        unsafe {
            let stored = &mut *self.block.get();
            stored[..len].copy_from_slice(&block[..len]);
            *self.block_len.get() = len;
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl AudioIn {
    /// Construct an `AudioIn` sharing an existing `input` Arc. Used by
    /// `Patch::insert_audio_in` to keep the legacy field hooked up.
    pub fn with_input(input: Arc<Mutex<PolyOutput>>) -> Self {
        Self {
            input,
            block: UnsafeCell::new(make_empty_block()),
            block_len: UnsafeCell::new(0),
        }
    }
}

impl MessageHandler for AudioIn {}
