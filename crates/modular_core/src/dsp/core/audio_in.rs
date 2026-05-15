//! Audio input module - reads from the audio input ring buffer.
//!
//! This module allows reading audio from the system's audio input device.

use std::sync::Arc;

use napi::Result;
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
    fn update(&self) {}

    fn get_id(&self) -> &str {
        WellKnownModule::HiddenAudioIn.id()
    }

    fn tick(&self) {}

    fn get_poly_sample(&self, _port: &str) -> Result<PolyOutput> {
        Ok(*self.input.lock())
    }

    fn get_sample(&self, _port: &str, channel: usize) -> Result<f32> {
        Ok(self.input.lock().get_cycling(channel))
    }

    fn get_module_type(&self) -> &str {
        WellKnownModule::HiddenAudioIn.id()
    }

    fn connect(&self, _patch: &crate::Patch) {}

    fn on_patch_update(&self) {}

    fn get_state(&self) -> Option<serde_json::Value> {
        None
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl MessageHandler for AudioIn {}
