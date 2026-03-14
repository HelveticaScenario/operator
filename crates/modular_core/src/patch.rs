//! Core patch structure for DSP processing
//!
//! This module contains the core `Patch` struct which represents a graph of
//! connected audio modules. The patch contains sampleable modules and tracks
//! that can be processed to generate audio.

use parking_lot::Mutex;

use crate::dsp::core::audio_in::AudioIn;
use crate::types::{
    Message, MessageTag, Sampleable, SampleableMap, WellKnownModule, ROOT_ID, ROOT_OUTPUT_PORT,
};
use crate::PolyOutput;

use std::collections::HashMap;
use std::sync::{Arc, Weak};

#[derive(Clone)]
struct MessageListenerRef {
    id: String,
    weak: Weak<Box<dyn Sampleable>>,
}

/// The core patch structure containing the DSP graph
pub struct Patch {
    pub audio_in: Arc<Mutex<PolyOutput>>,
    pub sampleables: SampleableMap,
    message_listeners: HashMap<MessageTag, Vec<MessageListenerRef>>,
}

impl Patch {
    /// Create a new empty patch
    pub fn new() -> Self {
        let mut sampleables: SampleableMap = Default::default();
        let audio_in_sampleable: AudioIn = Default::default();
        let audio_in = audio_in_sampleable.input.clone();

        sampleables.insert(
            audio_in_sampleable.get_id().to_string(),
            Arc::new(Box::new(audio_in_sampleable)),
        );
        println!("sampleables {:?}", sampleables.keys());
        let mut patch = Patch {
            audio_in,
            sampleables,
            message_listeners: HashMap::new(),
        };
        patch.rebuild_message_listeners();
        patch
    }

    /// Re-insert the AudioIn module into sampleables.
    /// Called after sampleables.clear() to restore the hidden audio input module.
    pub fn insert_audio_in(&mut self) {
        let audio_in_sampleable = AudioIn {
            input: self.audio_in.clone(),
        };
        let id = WellKnownModule::HiddenAudioIn.id().to_string();
        self.sampleables
            .insert(id, Arc::new(Box::new(audio_in_sampleable)));
    }

    pub fn rebuild_message_listeners(&mut self) {
        self.message_listeners.clear();
        for (id, sampleable) in &self.sampleables {
            for tag in sampleable.handled_message_tags() {
                self.message_listeners
                    .entry(*tag)
                    .or_default()
                    .push(MessageListenerRef {
                        id: id.clone(),
                        weak: Arc::downgrade(sampleable),
                    });
            }
        }
    }

    /// Add message listener entries for a single module (incremental update).
    pub fn add_message_listeners_for_module(
        &mut self,
        id: &str,
        sampleable: &Arc<Box<dyn Sampleable>>,
    ) {
        for tag in sampleable.handled_message_tags() {
            self.message_listeners
                .entry(*tag)
                .or_default()
                .push(MessageListenerRef {
                    id: id.to_string(),
                    weak: Arc::downgrade(sampleable),
                });
        }
    }

    /// Remove all message listener entries for a given module id.
    pub fn remove_message_listeners_for_module(&mut self, module_id: &str) {
        for listeners in self.message_listeners.values_mut() {
            listeners.retain(|r| r.id != module_id);
        }
    }

    /// Collect strong references to all modules currently in this patch that
    /// have registered to handle the given message tag.
    ///
    /// This method prunes stale entries. In particular, it will never return a
    /// module that is no longer present in `self.sampleables`, even if some
    /// other subsystem still holds a strong `Arc` to that module.
    pub fn message_listeners_for(&mut self, tag: MessageTag) -> Vec<Arc<Box<dyn Sampleable>>> {
        let Some(list) = self.message_listeners.get_mut(&tag) else {
            return Vec::new();
        };

        list.retain(|r| {
            if !self.sampleables.contains_key(&r.id) {
                return false;
            }
            r.weak.upgrade().is_some()
        });

        list.iter()
            .filter(|r| self.sampleables.contains_key(&r.id))
            .filter_map(|r| r.weak.upgrade())
            .collect()
    }

    pub fn dispatch_message(&mut self, message: &Message) -> napi::Result<()> {
        let listeners = self.message_listeners_for(message.tag());
        for s in listeners {
            s.handle_message(message)?;
        }
        Ok(())
    }

    /// Get the output sample from the root module
    pub fn get_output(&self) -> f32 {
        if let Some(root) = self.sampleables.get(&*ROOT_ID) {
            root.get_poly_sample(&ROOT_OUTPUT_PORT)
                .map(|p| p.get(0))
                .unwrap_or_default()
        } else {
            0.0
        }
    }

    /// Build a Patch from a [`PatchGraph`] for testing.
    ///
    /// Replicates the logic of `AudioState::apply_patch()` without the command
    /// queue or audio-thread indirection: instantiate modules, deserialize params
    /// on the calling thread, apply them, connect cables, and fire `on_patch_update`.
    pub fn from_graph(graph: &crate::types::PatchGraph, sample_rate: f32) -> Result<Self, String> {
        use crate::dsp::{get_constructors, get_params_deserializers};
        use crate::params::{extract_argument_spans, DeserializedParams};

        let constructors = get_constructors();
        let params_deserializers = get_params_deserializers();
        let mut patch = Patch::new();

        // 1. Instantiate all modules with their deserialized params
        for module_state in &graph.modules {
            let constructor = constructors
                .get(&module_state.module_type)
                .ok_or_else(|| format!("Unknown module type: {}", module_state.module_type))?;
            let deserializer = params_deserializers
                .get(&module_state.module_type)
                .ok_or_else(|| {
                    format!(
                        "No params deserializer for module type: {}",
                        module_state.module_type
                    )
                })?;
            let (stripped, argument_spans) = extract_argument_spans(module_state.params.clone());
            let cached = deserializer(stripped).map_err(|e| {
                format!(
                    "Failed to deserialize params for {}: {}",
                    module_state.id, e
                )
            })?;
            let deserialized = DeserializedParams {
                params: cached.params,
                argument_spans,
                channel_count: cached.channel_count,
            };
            let module = constructor(&module_state.id, sample_rate, deserialized)
                .map_err(|e| format!("Failed to create {}: {}", module_state.id, e))?;
            patch.sampleables.insert(module_state.id.clone(), module);
        }

        // 3. Connect all modules (resolves Cable weak pointers)
        for module in patch.sampleables.values() {
            module.connect(&patch);
        }

        // 4. Notify modules that patch is ready
        for module in patch.sampleables.values() {
            module.on_patch_update();
        }

        patch.rebuild_message_listeners();
        Ok(patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::types::MessageHandler;
    use napi::Result;

    #[test]
    fn test_patch_new_has_hidden_audio_in() {
        let patch = Patch::new();
        // Patch::new() inserts HIDDEN_AUDIO_IN which is managed internally
        assert!(patch
            .sampleables
            .contains_key(WellKnownModule::HiddenAudioIn.id()));
        assert_eq!(patch.sampleables.len(), 1);
    }

    #[test]
    fn test_patch_get_output_no_root() {
        let patch = Patch::new();
        let output = patch.get_output();
        assert!(
            (output - 0.0).abs() < 0.0001,
            "No root module should return 0.0"
        );
    }

    struct DummyMessageSampleable {
        id: String,
    }

    impl Sampleable for DummyMessageSampleable {
        fn get_id(&self) -> &str {
            &self.id
        }

        fn tick(&self) {}

        fn update(&self) {}

        fn get_poly_sample(&self, _port: &str) -> Result<crate::poly::PolyOutput> {
            Ok(crate::poly::PolyOutput::default())
        }

        fn get_module_type(&self) -> &str {
            "dummy"
        }

        fn apply_deserialized_params(
            &self,
            _deserialized: crate::params::DeserializedParams,
        ) -> Result<()> {
            Ok(())
        }

        fn connect(&self, _patch: &Patch) {}
    }

    impl MessageHandler for DummyMessageSampleable {
        fn handled_message_tags(&self) -> &'static [MessageTag] {
            &[MessageTag::MidiNoteOn]
        }

        fn handle_message(&self, _message: &Message) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn message_listeners_never_return_removed_modules() {
        let s: Arc<Box<dyn Sampleable>> = Arc::new(Box::new(DummyMessageSampleable {
            id: "m1".to_string(),
        }));

        let mut patch = Patch::new();
        patch.sampleables.insert("m1".to_string(), Arc::clone(&s));
        patch.rebuild_message_listeners();

        // Index should include it.
        assert_eq!(patch.message_listeners_for(MessageTag::MidiNoteOn).len(), 1);

        // Remove from patch but keep an external strong ref (`s`).
        patch.sampleables.remove("m1");

        // Rebuild/prune and ensure it is not returned.
        assert_eq!(patch.message_listeners_for(MessageTag::MidiNoteOn).len(), 0);
    }
}
