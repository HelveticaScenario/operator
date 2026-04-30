//! Core patch structure for DSP processing
//!
//! This module contains the core `Patch` struct which represents a graph of
//! connected audio modules. The patch contains sampleable modules and tracks
//! that can be processed to generate audio.

use parking_lot::Mutex;

use crate::PolyOutput;
use crate::dsp::core::audio_in::AudioIn;
use crate::types::{
    Message, MessageTag, ROOT_ID, ROOT_OUTPUT_PORT, Sampleable, SampleableMap, WavData,
    WellKnownModule,
};

use std::collections::HashMap;
use std::sync::Arc;

/// The core patch structure containing the DSP graph
pub struct Patch {
    pub audio_in: Arc<Mutex<PolyOutput>>,
    pub sampleables: SampleableMap,
    pub wav_data: HashMap<String, Arc<WavData>>,
    message_listeners: HashMap<MessageTag, Vec<String>>,
}

impl Patch {
    /// Create a new empty patch
    pub fn new() -> Self {
        let mut sampleables: SampleableMap = Default::default();
        let audio_in_sampleable: AudioIn = Default::default();
        let audio_in = audio_in_sampleable.input.clone();

        sampleables.insert(
            audio_in_sampleable.get_id().to_string(),
            Box::new(audio_in_sampleable),
        );
        println!("sampleables {:?}", sampleables.keys());
        let mut patch = Patch {
            audio_in,
            sampleables,
            wav_data: HashMap::new(),
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
        self.sampleables.insert(id, Box::new(audio_in_sampleable));
    }

    pub fn rebuild_message_listeners(&mut self) {
        self.message_listeners.clear();
        let ids: Vec<String> = self.sampleables.keys().cloned().collect();
        for id in ids {
            self.add_message_listeners_for_module(&id);
        }
    }

    /// Add message listener entries for a single module (incremental update).
    pub fn add_message_listeners_for_module(&mut self, id: &str) {
        let Some(sampleable) = self.sampleables.get(id) else {
            return;
        };

        for tag in sampleable.handled_message_tags() {
            self.message_listeners
                .entry(*tag)
                .or_default()
                .push(id.to_string());
        }
    }

    /// Remove all message listener entries for a given module id.
    pub fn remove_message_listeners_for_module(&mut self, module_id: &str) {
        for listeners in self.message_listeners.values_mut() {
            listeners.retain(|id| id != module_id);
        }
    }

    pub fn dispatch_message(&mut self, message: &Message) -> napi::Result<()> {
        let Some(listener_ids) = self.message_listeners.get(&message.tag()) else {
            return Ok(());
        };

        for id in listener_ids {
            if let Some(sampleable) = self.sampleables.get(id) {
                sampleable.handle_message(message)?;
            }
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
        use crate::params::{DeserializedParams, extract_argument_spans};

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
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex as StdMutex, OnceLock};

    struct CountingAllocator;

    static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
    static TRACKING_ENABLED: AtomicUsize = AtomicUsize::new(0);
    static TRACKING_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();

    #[global_allocator]
    static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if TRACKING_ENABLED.load(Ordering::Relaxed) != 0 {
                ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    fn allocation_tracking_lock() -> &'static StdMutex<()> {
        TRACKING_LOCK.get_or_init(|| StdMutex::new(()))
    }

    fn assert_message_listener_dispatch_does_not_allocate() {
        let hits = Arc::new(AtomicUsize::new(0));
        let s: Box<dyn Sampleable> = Box::new(CountingMessageSampleable {
            id: "m1".to_string(),
            hits: Arc::clone(&hits),
        });

        let mut patch = Patch::new();
        patch.sampleables.insert("m1".to_string(), s);
        patch.rebuild_message_listeners();

        let message = Message::MidiNoteOn(crate::types::MidiNoteOn {
            device: None,
            note: 60,
            velocity: 100,
            channel: 0,
        });

        let _guard = allocation_tracking_lock().lock().unwrap();
        ALLOCATIONS.store(0, Ordering::SeqCst);
        TRACKING_ENABLED.store(1, Ordering::SeqCst);
        patch.dispatch_message(&message).unwrap();
        TRACKING_ENABLED.store(0, Ordering::SeqCst);

        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_patch_new_has_hidden_audio_in() {
        let patch = Patch::new();
        // Patch::new() inserts HIDDEN_AUDIO_IN which is managed internally
        assert!(
            patch
                .sampleables
                .contains_key(WellKnownModule::HiddenAudioIn.id())
        );
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

        fn connect(&self, _patch: &Patch) {}

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
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
    fn message_listener_index_stores_ids_only() {
        let s: Box<dyn Sampleable> = Box::new(DummyMessageSampleable {
            id: "m1".to_string(),
        });

        let mut patch = Patch::new();
        patch.sampleables.insert("m1".to_string(), s);
        patch.rebuild_message_listeners();

        assert_eq!(
            patch
                .message_listeners
                .get(&MessageTag::MidiNoteOn)
                .cloned(),
            Some(vec!["m1".to_string()])
        );
    }

    struct CountingMessageSampleable {
        id: String,
        hits: Arc<AtomicUsize>,
    }

    impl Sampleable for CountingMessageSampleable {
        fn get_id(&self) -> &str {
            &self.id
        }

        fn tick(&self) {}

        fn update(&self) {}

        fn get_poly_sample(&self, _port: &str) -> Result<crate::poly::PolyOutput> {
            Ok(crate::poly::PolyOutput::default())
        }

        fn get_module_type(&self) -> &str {
            "counting"
        }

        fn connect(&self, _patch: &Patch) {}

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl MessageHandler for CountingMessageSampleable {
        fn handled_message_tags(&self) -> &'static [MessageTag] {
            &[MessageTag::MidiNoteOn]
        }

        fn handle_message(&self, _message: &Message) -> Result<()> {
            self.hits.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn message_listener_removed_module_is_not_dispatched() {
        let hits = Arc::new(AtomicUsize::new(0));
        let s: Box<dyn Sampleable> = Box::new(CountingMessageSampleable {
            id: "m1".to_string(),
            hits: Arc::clone(&hits),
        });

        let mut patch = Patch::new();
        patch.sampleables.insert("m1".to_string(), s);
        patch.rebuild_message_listeners();

        let message = Message::MidiNoteOn(crate::types::MidiNoteOn {
            device: None,
            note: 60,
            velocity: 100,
            channel: 0,
        });

        patch.dispatch_message(&message).unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        patch.sampleables.remove("m1");
        patch.remove_message_listeners_for_module("m1");

        patch.dispatch_message(&message).unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(
            patch
                .message_listeners
                .get(&MessageTag::MidiNoteOn)
                .map(Vec::len),
            Some(0)
        );
    }

    #[test]
    fn message_listener_dispatch_does_not_allocate() {
        const ISOLATED_ALLOC_TEST_ENV: &str = "MODULAR_CORE_ISOLATED_ALLOC_TEST";

        if std::env::var_os(ISOLATED_ALLOC_TEST_ENV).is_some() {
            assert_message_listener_dispatch_does_not_allocate();
            return;
        }

        // The allocator counter is process-global, so run the actual assertion in
        // a child test process to avoid unrelated allocations from concurrently
        // executing tests in the parent harness.
        let output = Command::new(std::env::current_exe().unwrap())
            .env(ISOLATED_ALLOC_TEST_ENV, "1")
            .arg("--exact")
            .arg("patch::tests::message_listener_dispatch_does_not_allocate")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .output()
            .expect("failed to spawn isolated allocation test");

        assert!(
            output.status.success(),
            "isolated allocation test failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
