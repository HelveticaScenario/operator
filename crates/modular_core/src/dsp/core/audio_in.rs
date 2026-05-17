//! Audio input module - reads from the audio input ring buffer.
//!
//! This module allows reading audio from the system's audio input device.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::{
    Sampleable,
    poly::PolyOutput,
    types::{MessageHandler, WellKnownModule},
};

#[derive(Default)]
pub struct AudioIn {
    pub input: Arc<Mutex<PolyOutput>>,
}

impl Sampleable for AudioIn {
    fn get_id(&self) -> &str {
        WellKnownModule::HiddenAudioIn.id()
    }

    fn get_module_type(&self) -> &str {
        WellKnownModule::HiddenAudioIn.id()
    }

    fn connect(&self, _patch: &crate::Patch) {}

    /// Reset the per-block cursor. `AudioIn`'s input is a single snapshot
    /// owned by the audio thread — there's nothing per-sample to advance.
    fn start_block(&self) {}

    fn ensure_processed_to(&self, _target: usize) {}

    fn ensure_processed(&self) {}

    /// Snapshot read of the current host audio input frame. The audio
    /// callback writes `self.input` once per block before pulling outputs,
    /// so `get_value_at` returns the same value for every slot in a block.
    fn get_value_at(&self, _port: &str, ch: usize, _index: usize) -> f32 {
        self.input.lock().get(ch)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl MessageHandler for AudioIn {}
