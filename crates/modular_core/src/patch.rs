//! Core patch structure for DSP processing
//!
//! This module contains the core `Patch` struct which represents a graph of
//! connected audio modules. The patch contains sampleable modules and tracks
//! that can be processed to generate audio.

use crate::dsp::core::audio_in::AudioIn;
use crate::types::{
    Message, MessageTag, ROOT_ID, ROOT_OUTPUT_PORT, Sampleable, SampleableMap, WavData,
    WellKnownModule,
};

use std::collections::HashMap;
use std::sync::Arc;

/// The core patch structure containing the DSP graph
pub struct Patch {
    pub sampleables: SampleableMap,
    pub wav_data: HashMap<String, Arc<WavData>>,
    message_listeners: HashMap<MessageTag, Vec<String>>,
}

impl Patch {
    /// Create a new empty patch
    pub fn new() -> Self {
        let mut sampleables: SampleableMap = Default::default();
        let audio_in_sampleable = AudioIn::default();

        sampleables.insert(
            audio_in_sampleable.get_id().to_string(),
            Box::new(audio_in_sampleable),
        );
        let mut patch = Patch {
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
        let audio_in_sampleable = AudioIn::default();
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

    /// Get the output sample from the root module (channel 0, slot 0).
    pub fn get_output(&self) -> f32 {
        self.sampleables
            .get(&*ROOT_ID)
            .map(|root| root.get_value_at(&ROOT_OUTPUT_PORT, 0, 0))
            .unwrap_or_default()
    }

    /// Construct each module from already-deserialized params at its resolved
    /// processing mode and insert it unconnected. Main thread only.
    pub fn insert_modules(
        &mut self,
        modules: impl IntoIterator<
            Item = (
                String,
                String,
                crate::params::DeserializedParams,
                crate::types::ProcessingMode,
            ),
        >,
        sample_rate: f32,
        block_size: usize,
    ) -> Result<(), String> {
        let constructors = crate::dsp::get_constructors();
        for (id, module_type, params, mode) in modules {
            let constructor = constructors
                .get(&module_type)
                .ok_or_else(|| format!("Unknown module type: {}", module_type))?;
            let module = constructor(&id, sample_rate, params, block_size, mode)
                .map_err(|e| format!("Failed to create module {}: {}", id, e))?;
            self.sampleables.insert(id, module);
        }
        Ok(())
    }

    /// Resolve every module's cable pointers, then notify each that the patch is
    /// ready. `connect` reads raw pointers into upstream modules' output buffers,
    /// so on a hot-swap this MUST run only after `transfer_state_from` has
    /// swapped those buffers in. Allocation-free and safe on the audio thread.
    pub fn connect_all(&self) {
        for module in self.sampleables.values() {
            module.connect(self);
        }
        for module in self.sampleables.values() {
            module.on_patch_update();
        }
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

        fn get_module_type(&self) -> &str {
            "dummy"
        }

        fn connect(&self, _patch: &Patch) {}

        fn start_block(&self) {}

        fn ensure_processed_to(&self, _target: usize) {}

        fn ensure_processed(&self) {}

        fn get_value_at(&self, _port: &str, _ch: usize, _index: usize) -> f32 {
            0.0
        }

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

        fn get_module_type(&self) -> &str {
            "counting"
        }

        fn connect(&self, _patch: &Patch) {}

        fn start_block(&self) {}

        fn ensure_processed_to(&self, _target: usize) {}

        fn ensure_processed(&self) {}

        fn get_value_at(&self, _port: &str, _ch: usize, _index: usize) -> f32 {
            0.0
        }

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
