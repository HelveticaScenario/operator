//! Recording buffers handed to the recording voices of `$granular` and
//! `$clockedDelay` across patch updates.

use super::buffer::GrainBuffer;
use super::chain::Media;

/// Silent recording buffers made on the main thread, one per voice. A voice
/// takes one in `on_patch_update` when it has no recording at the module's
/// quality, and leaves the buffer it gives up here, to drop with the params on
/// the main thread. A clone starts empty, as `init` fills it.
#[derive(Default)]
pub struct FreshBuffers(Vec<GrainBuffer>);

impl FreshBuffers {
    /// A buffer as long as `media` records for each of `voices`. Main thread
    /// only.
    pub fn allocate(voices: usize, media: Media) -> Self {
        Self(
            (0..voices)
                .map(|_| {
                    let mut buffer = GrainBuffer::default();
                    buffer.allocate_frames(media.max_delay());
                    buffer
                })
                .collect(),
        )
    }

    /// Give voice `voice` a fresh buffer in place of `buffer` when `buffer`
    /// has no recording, or when `cleared` because the quality changed.
    /// Returns whether it did; the fresh buffer needs no clearing.
    /// Allocation-free.
    pub fn hand_out(&mut self, voice: usize, buffer: &mut GrainBuffer, cleared: bool) -> bool {
        match self.0.get_mut(voice) {
            Some(fresh) if (cleared || !buffer.records()) && fresh.records() => {
                std::mem::swap(buffer, fresh);
                true
            }
            _ => false,
        }
    }
}

impl Clone for FreshBuffers {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl crate::types::Connect for FreshBuffers {
    fn apply_default_connections(&mut self) {}
    fn connect(&mut self, _patch: &crate::patch::Patch) {}
    fn collect_cables(&self, _sink: &mut Vec<String>) {}
    fn inject_index_ptr(&mut self, _ptr: *const std::cell::Cell<usize>) {}
    fn mark_glides(
        &mut self,
        _old: Option<&Self>,
        _generation: u64,
        _ids: &dyn crate::types::PreviousIds,
    ) {
    }
}
