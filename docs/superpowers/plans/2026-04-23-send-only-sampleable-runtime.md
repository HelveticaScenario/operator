# Send-Only Sampleable Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace fake shared module ownership with a send-only runtime model that keeps true shared assets on `Arc`, moves module-graph connection caches to raw pointers, and precomputes more runtime metadata before audio-thread apply.

**Architecture:** Land this in small, verifiable slices. First replace weak-ref connection caches (`Signal`, `Buffer`, seq caches) and listener weak refs with raw pointers or ids while the graph still uses `Arc`. Then convert the owned module graph, command payloads, and proc-macro constructors from `Arc<Box<dyn Sampleable>>` to `Box<dyn Sampleable>`, drop `Sync` from `Sampleable`, and finish by precomputing more runtime metadata so audio-thread apply stops rebuilding ownership-derived state.

**Tech Stack:** Rust 2024, `napi`, existing proc-macro code in `modular_derive`, `parking_lot`, `rtrb`, existing Rust unit tests in `modular_core` and `modular`.

---

## File Map

- `crates/modular_core/src/types.rs`
  - Owns `Sampleable` trait, `SampleableMap`, `SampleableConstructor`, `Signal`, `Buffer`, and core trait tests.
- `crates/modular_core/src/patch.rs`
  - Owns patch-level module storage and message listener index.
- `crates/modular_core/src/dsp/seq/seq_value.rs`
  - Owns `SeqSourceConnections` and cached source resolution for pattern values.
- `crates/modular_core/src/dsp/seq/seq.rs`
  - Uses `SeqSourceConnections` during playback.
- `crates/modular_core/src/dsp/utilities/math.rs`
  - Constructs `Signal::Cable` values while parsing math expressions.
- `crates/modular_core/src/pattern_system/combinators.rs`
  - Test-only `Signal::Cable` constructors that must follow new field shape.
- `crates/modular_core/src/dsp/utilities/buffer.rs`
  - Integration tests around `Buffer` and `$buffer` ownership behavior.
- `crates/modular_core/src/dsp/samplers/sampler.rs`
  - Test helper returns constructor output type.
- `crates/modular_core/src/dsp/fx/plate.rs`
  - Test helper returns constructor output type.
- `crates/modular_core/src/dsp/fx/dattorro.rs`
  - Test helper returns constructor output type.
- `crates/modular_derive/src/module_attr.rs`
  - Generated wrapper `Send`/`Sync` contract and constructor return type.
- `crates/modular/src/commands.rs`
  - Command queue payload types and garbage queue ownership.
- `crates/modular/src/audio.rs`
  - Audio-thread apply path, runtime metadata prep, and Rust tests.
- `crates/modular/src/lib.rs`
  - Main-thread single-module update path.
- `crates/modular_core/Cargo.toml`
  - Add `static_assertions` dev-dependency for compile-time send/not-sync assertions.

---

### Task 1: Replace `Signal` weak refs with raw-pointer caches

**Files:**
- Modify: `crates/modular_core/src/types.rs`
- Modify: `crates/modular_core/src/dsp/utilities/math.rs`
- Modify: `crates/modular_core/src/pattern_system/combinators.rs`
- Test: `cargo test -p modular_core raw_pointer_signal_ -- --nocapture`

- [ ] **Step 1: Write the failing `Signal` raw-pointer tests**

In `crates/modular_core/src/types.rs`, inside the existing test module near the other `connect()` tests, add this helper and these tests exactly:

```rust
    use std::sync::Arc;

    #[derive(Default)]
    struct SignalProbeSampleable {
        id: String,
        output: f32,
    }

    impl Sampleable for SignalProbeSampleable {
        fn get_id(&self) -> &str {
            &self.id
        }

        fn tick(&self) {}

        fn update(&self) {}

        fn get_poly_sample(&self, _port: &str) -> napi::Result<crate::poly::PolyOutput> {
            Ok(crate::poly::PolyOutput::mono(self.output))
        }

        fn get_module_type(&self) -> &str {
            "$probe"
        }

        fn connect(&self, _patch: &Patch) {}

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl MessageHandler for SignalProbeSampleable {}

    fn signal_probe_patch(output: f32) -> Patch {
        let mut patch = Patch::new();
        patch.sampleables.insert(
            "osc".to_string(),
            Arc::new(Box::new(SignalProbeSampleable {
                id: "osc".to_string(),
                output,
            })),
        );
        patch
    }

    #[test]
    fn raw_pointer_signal_connect_resolves_nonnull_source() {
        let patch = signal_probe_patch(3.25);
        let mut signal = Signal::Cable {
            module: "osc".to_string(),
            resolved: None,
            port: "output".to_string(),
            channel: 0,
        };

        signal.connect(&patch);

        let Signal::Cable { resolved, .. } = &signal else {
            panic!("expected cable signal");
        };
        assert!(resolved.is_some(), "connect() should cache a raw source pointer");
        assert!((signal.get_value() - 3.25).abs() < 1e-6);
    }

    #[test]
    fn raw_pointer_signal_connect_clears_missing_source() {
        let patch = Patch::new();
        let mut signal = Signal::Cable {
            module: "missing".to_string(),
            resolved: None,
            port: "output".to_string(),
            channel: 0,
        };

        signal.connect(&patch);

        let Signal::Cable { resolved, .. } = &signal else {
            panic!("expected cable signal");
        };
        assert!(resolved.is_none(), "missing source should clear cached pointer");
        assert_eq!(signal.get_value(), 0.0);
    }
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run:

```bash
cargo test -p modular_core raw_pointer_signal_ -- --nocapture
```

Expected: FAIL to compile with errors like `variant 'Signal::Cable' has no field named 'resolved'`.

- [ ] **Step 3: Implement the raw-pointer `Signal` cache**

In `crates/modular_core/src/types.rs`, make these exact local replacements:

1. Add `NonNull` to the top-level imports near the other `std` imports:

```rust
use std::ptr::NonNull;
```

2. Add shared pointer alias next to the `SampleableMap` alias block:

```rust
pub type SampleablePtr = NonNull<dyn Sampleable>;
```

3. Replace the `Signal::Cable` field definition with:

```rust
    Cable {
        module: String,
        resolved: Option<SampleablePtr>,
        port: String,
        channel: usize,
    },
```

4. In both serde construction sites, replace `module_ptr: sync::Weak::new()` with:

```rust
resolved: None,
```

5. Replace `Signal::get_value()` with this version:

```rust
    pub fn get_value(&self) -> f32 {
        match self {
            Signal::Volts(v) => *v,
            Signal::Cable {
                module,
                resolved,
                port,
                channel,
                ..
            } => {
                if let Some(sample) = get_materialized_block_sample(module, port, *channel) {
                    return sample;
                }

                match resolved {
                    Some(ptr) => unsafe {
                        ptr.as_ref()
                            .get_poly_sample(port)
                            .map(|p| p.get_cycling(*channel))
                            .unwrap_or(0.0)
                    },
                    None => 0.0,
                }
            }
        }
    }
```

6. Replace `impl Connect for Signal` with:

```rust
impl Connect for Signal {
    fn connect(&mut self, patch: &Patch) {
        if let Signal::Cable {
            module, resolved, ..
        } = self
        {
            *resolved = patch
                .sampleables
                .get(module)
                .map(|sampleable| NonNull::from(sampleable.as_ref()));
        }
    }
}
```

7. Replace `Signal` equality on the cached field with this match arm body:

```rust
                resolved_1 == resolved_2
                    && port_1 == port_2
                    && module_1 == module_2
                    && channel_1 == channel_2
```

and rename the matched fields from `module_ptr_*` to `resolved_*` in the pattern.

8. In `crates/modular_core/src/dsp/utilities/math.rs`, replace the `Signal::Cable` construction with:

```rust
            signals.push(Signal::Cable {
                module,
                resolved: None,
                port,
                channel,
            });
```

9. In `crates/modular_core/src/pattern_system/combinators.rs`, replace both test constructors with:

```rust
        let sig1 = Signal::Cable {
            module: "sine".into(),
            resolved: None,
            port: "output".into(),
            channel: 0,
        };

        let sig2 = Signal::Cable {
            module: "sine".into(),
            resolved: None,
            port: "output".into(),
            channel: 0,
        };
```

- [ ] **Step 4: Run the `Signal` tests again**

Run:

```bash
cargo test -p modular_core raw_pointer_signal_ -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/modular_core/src/types.rs crates/modular_core/src/dsp/utilities/math.rs crates/modular_core/src/pattern_system/combinators.rs
git commit -m "refactor(core): cache signal sources as raw ptrs"
```

---

### Task 2: Convert `Buffer` and seq caches from shared refs to raw pointers

**Files:**
- Modify: `crates/modular_core/src/types.rs`
- Modify: `crates/modular_core/src/dsp/seq/seq_value.rs`
- Modify: `crates/modular_core/src/dsp/seq/seq.rs`
- Test: `cargo test -p modular_core raw_pointer_ -- --nocapture`

- [ ] **Step 1: Write the failing `Buffer` and seq tests**

In `crates/modular_core/src/types.rs`, add these tests inside the existing test module after the `Signal` raw-pointer tests:

```rust
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct BufferProbeSampleable {
        id: String,
        output: Arc<BufferData>,
        updates: Arc<AtomicUsize>,
    }

    impl Sampleable for BufferProbeSampleable {
        fn get_id(&self) -> &str {
            &self.id
        }

        fn tick(&self) {}

        fn update(&self) {
            self.updates.fetch_add(1, Ordering::SeqCst);
        }

        fn get_poly_sample(&self, _port: &str) -> napi::Result<crate::poly::PolyOutput> {
            Ok(crate::poly::PolyOutput::default())
        }

        fn get_module_type(&self) -> &str {
            "$buffer"
        }

        fn connect(&self, _patch: &Patch) {}

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn get_buffer_output(&self, port: &str) -> Option<&Arc<BufferData>> {
            match port {
                "buffer" => Some(&self.output),
                _ => None,
            }
        }
    }

    impl MessageHandler for BufferProbeSampleable {}

    fn buffer_probe_patch(updates: Arc<AtomicUsize>) -> Patch {
        let mut patch = Patch::new();
        patch.sampleables.insert(
            "buffer-src".to_string(),
            Arc::new(Box::new(BufferProbeSampleable {
                id: "buffer-src".to_string(),
                output: Arc::new(BufferData::from_samples(vec![vec![1.0, 2.0, 3.0]])),
                updates,
            })),
        );
        patch
    }

    #[test]
    fn raw_pointer_buffer_connect_populates_source_ptr_and_buffer_arc() {
        let updates = Arc::new(AtomicUsize::new(0));
        let patch = buffer_probe_patch(Arc::clone(&updates));
        let mut buffer = Buffer::new("buffer-src".to_string(), "buffer".to_string(), 1);

        buffer.connect(&patch);

        assert!(buffer.cached_source_ptr.is_some());
        assert!(buffer.cached_buffer.is_some());
        buffer.ensure_source_updated();
        assert_eq!(updates.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn raw_pointer_buffer_connect_clears_missing_source_ptr() {
        let patch = Patch::new();
        let mut buffer = Buffer::new("missing".to_string(), "buffer".to_string(), 1);

        buffer.connect(&patch);

        assert!(buffer.cached_source_ptr.is_none());
        assert!(buffer.cached_buffer.is_none());
        assert_eq!(buffer.read(0, 0), 0.0);
    }
```

In `crates/modular_core/src/dsp/seq/seq_value.rs`, add this test in the existing test module:

```rust
    #[test]
    fn raw_pointer_seq_pattern_connects_nonnull_source_ptrs() {
        let sampleable: Arc<Box<dyn Sampleable>> = Arc::new(Box::new(DummySampleable::new(
            "lfo",
            [("output", 2.5)],
        )));
        let patch = make_patch_with_sampleable(sampleable);
        let mut pattern = SeqPatternParam::parse("module(lfo:output:0)").unwrap();

        pattern.connect(&patch);

        let connected: std::ptr::NonNull<dyn Sampleable> = pattern
            .connected_sampleable("lfo")
            .expect("expected connected sampleable pointer");
        unsafe {
            assert_eq!(connected.as_ref().get_id(), "lfo");
        }

        let signal = match &pattern.cached_haps()[0][0].value {
            SeqValue::Signal { signal, .. } => signal,
            _ => panic!("expected cached signal value"),
        };
        assert!((pattern.resolve_signal_value(signal) - 2.5).abs() < 1e-6);
    }
```

- [ ] **Step 2: Run the new raw-pointer tests to verify they fail**

Run:

```bash
cargo test -p modular_core raw_pointer_ -- --nocapture
```

Expected: FAIL to compile with errors like `no field 'cached_source_ptr' on type 'Buffer'` and `mismatched types: expected NonNull<dyn Sampleable>`.

- [ ] **Step 3: Implement raw-pointer `Buffer` and seq caches**

In `crates/modular_core/src/types.rs`, make these exact local replacements:

1. Replace the `Buffer` field block with:

```rust
pub struct Buffer {
    source_module: String,
    source_port: String,
    cached_source_ptr: Option<SampleablePtr>,
    cached_buffer: Option<Arc<BufferData>>,
    channels: usize,
}
```

2. Update `Buffer::new()` to initialize `cached_source_ptr: None`.

3. Replace `ensure_source_updated()` with:

```rust
    pub fn ensure_source_updated(&self) {
        if let Some(ptr) = self.cached_source_ptr {
            unsafe {
                ptr.as_ref().update();
            }
        }
    }
```

4. Replace `impl Connect for Buffer` with:

```rust
impl Connect for Buffer {
    fn connect(&mut self, patch: &Patch) {
        if let Some(module) = patch.sampleables.get(&self.source_module) {
            self.cached_source_ptr = Some(NonNull::from(module.as_ref()));
            if let Some(buffer_data) = module.get_buffer_output(&self.source_port) {
                self.cached_buffer = Some(Arc::clone(buffer_data));
            } else {
                eprintln!(
                    "[Buffer] module '{}' has no buffer output on port '{}'",
                    self.source_module, self.source_port
                );
                self.cached_buffer = None;
            }
        } else {
            eprintln!(
                "[Buffer] source module '{}' not found in patch",
                self.source_module
            );
            self.cached_source_ptr = None;
            self.cached_buffer = None;
        }
    }
}
```

In `crates/modular_core/src/dsp/seq/seq_value.rs`, make these exact replacements:

1. Change imports at the top to:

```rust
use std::{
    cell::UnsafeCell,
    ptr::NonNull,
    sync::Arc,
};
```

2. Replace `SeqValue::connect_sampleable_map()` with:

```rust
    fn connect_sampleable_map(&mut self, sampleables: &SampleableMap) {
        if let SeqValue::Signal { signal, .. } = self {
            if let Signal::Cable {
                module, resolved, ..
            } = signal
            {
                *resolved = sampleables
                    .get(module)
                    .map(|sampleable| NonNull::from(sampleable.as_ref()));
            }
        }
    }
```

3. Replace `SeqSourceConnections` with:

```rust
#[derive(Debug, Default)]
pub(crate) struct SeqSourceConnections {
    connections: UnsafeCell<Vec<Option<crate::types::SampleablePtr>>>,
}

impl SeqSourceConnections {
    fn new(size: usize) -> Self {
        Self {
            connections: UnsafeCell::new(vec![None; size]),
        }
    }

    fn update(&self, source_module_ids: &[String], patch: &Patch) {
        let connections = unsafe { &mut *self.connections.get() };

        debug_assert_eq!(connections.len(), source_module_ids.len());

        for (module_id, connection) in source_module_ids.iter().zip(connections.iter_mut()) {
            *connection = patch
                .sampleables
                .get(module_id)
                .map(|sampleable| NonNull::from(sampleable.as_ref()));
        }
    }

    pub(crate) fn connected_sampleable(
        &self,
        source_module_ids: &[String],
        module_id: &str,
    ) -> Option<crate::types::SampleablePtr> {
        let index = source_module_ids
            .binary_search_by(|candidate| candidate.as_str().cmp(module_id))
            .ok()?;

        let connections = unsafe { &*self.connections.get() };
        connections.get(index).copied().flatten()
    }
}
```

4. Replace the `#[cfg(test)] fn connected_sampleable` helper with:

```rust
    #[cfg(test)]
    fn connected_sampleable(&self, module_id: &str) -> Option<crate::types::SampleablePtr> {
        self.source_connections
            .connected_sampleable(self.source_module_ids.as_ref(), module_id)
    }
```

5. In `resolve_signal_value()`, replace the cable branch body with:

```rust
            Signal::Cable { module, port, channel, .. } => self
                .connected_sampleable(module)
                .map(|sampleable| unsafe {
                    sampleable
                        .as_ref()
                        .get_poly_sample(port)
                        .map(|output| output.get_cycling(*channel))
                        .unwrap_or(0.0)
                })
                .unwrap_or(0.0),
```

In `crates/modular_core/src/dsp/seq/seq.rs`, replace the cable-resolution branch with:

```rust
            } => connection_state
                .connected_sampleable(source_module_ids, module)
                .map(|sampleable| unsafe {
                    sampleable
                        .as_ref()
                        .get_poly_sample(port)
                        .map(|output| output.get_cycling(*channel))
                        .unwrap_or(0.0)
                })
                .unwrap_or(0.0),
```

- [ ] **Step 4: Run the raw-pointer regression tests again**

Run:

```bash
cargo test -p modular_core raw_pointer_ -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/modular_core/src/types.rs crates/modular_core/src/dsp/seq/seq_value.rs crates/modular_core/src/dsp/seq/seq.rs
git commit -m "refactor(core): use raw ptr caches for buffer and seq"
```

---

### Task 3: Replace patch listener weak refs with listener ids

**Files:**
- Modify: `crates/modular_core/src/patch.rs`
- Test: `cargo test -p modular_core message_listener_ -- --nocapture`

- [ ] **Step 1: Write the failing listener-index tests**

In `crates/modular_core/src/patch.rs`, update the test module with this new helper and these tests:

```rust
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

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
            "dummy"
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
    fn message_listener_index_stores_ids_only() {
        let mut patch = Patch::new();
        patch.sampleables.insert(
            "m1".to_string(),
            Arc::new(Box::new(DummyMessageSampleable {
                id: "m1".to_string(),
            })),
        );

        patch.rebuild_message_listeners();

        assert_eq!(
            patch.message_listeners.get(&MessageTag::MidiNoteOn).cloned(),
            Some(vec!["m1".to_string()]),
        );
    }

    #[test]
    fn message_listener_removed_module_is_not_dispatched() {
        let hits = Arc::new(AtomicUsize::new(0));
        let mut patch = Patch::new();
        patch.sampleables.insert(
            "m1".to_string(),
            Arc::new(Box::new(CountingMessageSampleable {
                id: "m1".to_string(),
                hits: Arc::clone(&hits),
            })),
        );
        patch.rebuild_message_listeners();

        patch
            .dispatch_message(&Message::MidiNoteOn(crate::types::MidiNoteOn {
                device: None,
                channel: 0,
                note: 60,
                velocity: 100,
            }))
            .unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        patch.sampleables.remove("m1");

        patch
            .dispatch_message(&Message::MidiNoteOn(crate::types::MidiNoteOn {
                device: None,
                channel: 0,
                note: 61,
                velocity: 100,
            }))
            .unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
```

- [ ] **Step 2: Run the listener tests to verify they fail**

Run:

```bash
cargo test -p modular_core message_listener_ -- --nocapture
```

Expected: FAIL to compile with a type mismatch because `message_listeners` still stores `MessageListenerRef` values instead of `Vec<String>`.

- [ ] **Step 3: Implement the id-only listener index**

In `crates/modular_core/src/patch.rs`, make these exact replacements:

1. Remove `Weak` from the imports and replace the listener field with:

```rust
    message_listeners: HashMap<MessageTag, Vec<String>>,
```

2. Delete the `MessageListenerRef` struct entirely.

3. Replace `rebuild_message_listeners()` with:

```rust
    pub fn rebuild_message_listeners(&mut self) {
        self.message_listeners.clear();
        for (id, sampleable) in &self.sampleables {
            for tag in sampleable.handled_message_tags() {
                self.message_listeners
                    .entry(*tag)
                    .or_default()
                    .push(id.clone());
            }
        }
    }
```

4. Replace `add_message_listeners_for_module()` with:

```rust
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
```

5. Keep `remove_message_listeners_for_module()` but change the retain closure to:

```rust
            listeners.retain(|id| id != module_id);
```

6. Delete `message_listeners_for()` entirely and replace `dispatch_message()` with:

```rust
    pub fn dispatch_message(&mut self, message: &Message) -> napi::Result<()> {
        let listener_ids = self
            .message_listeners
            .get(&message.tag())
            .cloned()
            .unwrap_or_default();

        for id in listener_ids {
            if let Some(sampleable) = self.sampleables.get(&id) {
                sampleable.handle_message(message)?;
            }
        }

        Ok(())
    }
```

7. Replace the old `message_listeners_never_return_removed_modules()` test with the two new tests above.

- [ ] **Step 4: Run the listener tests again**

Run:

```bash
cargo test -p modular_core message_listener_ -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/modular_core/src/patch.rs
git commit -m "refactor(core): store message listener ids"
```

---

### Task 4: Convert module ownership from `Arc<Box<dyn Sampleable>>` to `Box<dyn Sampleable>`

**Files:**
- Modify: `crates/modular_core/src/types.rs`
- Modify: `crates/modular_core/src/patch.rs`
- Modify: `crates/modular_derive/src/module_attr.rs`
- Modify: `crates/modular/src/commands.rs`
- Modify: `crates/modular/src/audio.rs`
- Modify: `crates/modular/src/lib.rs`
- Modify: `crates/modular_core/src/dsp/seq/seq_value.rs`
- Modify: `crates/modular_core/src/dsp/utilities/buffer.rs`
- Modify: `crates/modular_core/src/dsp/samplers/sampler.rs`
- Modify: `crates/modular_core/src/dsp/fx/plate.rs`
- Modify: `crates/modular_core/src/dsp/fx/dattorro.rs`
- Test: `cargo test -p modular_core`
- Test: `cargo test -p modular`

- [ ] **Step 1: Write the failing owned-box test coverage**

In `crates/modular/src/audio.rs`, update the existing test helpers and add this test:

```rust
  impl MockModule {
    fn new(label: &str) -> Box<dyn modular_core::types::Sampleable> {
      Box::new(Self {
        label: label.to_string(),
        current_id: label.to_string(),
      })
    }
  }

  impl RecordingModule {
    fn new(id: &str, state: Arc<RecordingModuleState>) -> Box<dyn modular_core::types::Sampleable> {
      Box::new(Self {
        id: id.to_string(),
        state,
      })
    }
  }

  #[test]
  fn owned_box_patch_update_replaces_module_without_clone() {
    let mut processor = create_test_processor();
    let first_state = Arc::new(RecordingModuleState::default());
    let second_state = Arc::new(RecordingModuleState::default());

    processor
      .runtime
      .patch
      .sampleables
      .insert("probe".to_string(), RecordingModule::new("probe", Arc::clone(&first_state)));

    let mut update = PatchUpdate::new(TEST_SAMPLE_RATE);
    update.update_id = 400;
    update.desired_ids.insert("probe".to_string());
    update
      .inserts
      .push(("probe".to_string(), RecordingModule::new("probe", Arc::clone(&second_state))));

    processor.apply_patch_update(update);

    assert!(processor.runtime.patch.sampleables.contains_key("probe"));
  }
```

Also update these helper signatures so they intentionally stop compiling until ownership changes land:

In `crates/modular_core/src/dsp/utilities/buffer.rs`:

```rust
    fn make_module(
        module_type: &str,
        id: &str,
        params: serde_json::Value,
    ) -> Box<dyn Sampleable> {
```

In `crates/modular_core/src/dsp/samplers/sampler.rs`:

```rust
    fn make_module(
        module_type: &str,
        id: &str,
        params: serde_json::Value,
    ) -> Box<dyn Sampleable> {
```

In `crates/modular_core/src/dsp/fx/plate.rs`:

```rust
    fn make_plate(params: serde_json::Value) -> Box<dyn Sampleable> {
```

In `crates/modular_core/src/dsp/fx/dattorro.rs`:

```rust
    fn make_dattorro(params: serde_json::Value) -> Box<dyn Sampleable> {
```

- [ ] **Step 2: Run the Rust tests to verify ownership still fails to compile**

Run:

```bash
cargo test -p modular_core
```

Expected: FAIL to compile with mismatches between `Arc<Box<dyn Sampleable>>` and `Box<dyn Sampleable>`.

- [ ] **Step 3: Convert core ownership aliases and proc-macro constructors**

Make these exact replacements:

In `crates/modular_core/src/types.rs`:

```rust
pub type SampleableMap = HashMap<String, Box<dyn Sampleable>>;

pub type SampleableConstructor =
    Box<dyn Fn(&String, f32, crate::params::DeserializedParams) -> Result<Box<dyn Sampleable>>>;
```

In `crates/modular_core/src/patch.rs`, replace the hidden audio input inserts with:

```rust
        sampleables.insert(
            audio_in_sampleable.get_id().to_string(),
            Box::new(audio_in_sampleable),
        );
```

and:

```rust
        self.sampleables
            .insert(id, Box::new(audio_in_sampleable));
```

In `Patch::from_graph()`, keep the same logic but the constructor return type now drops directly into the map without `Arc` wrapping.

In `crates/modular_derive/src/module_attr.rs`, make these exact replacements:

1. Update wrapper safety docs to replace `Arc`s with owned boxes:

```rust
        /// 3. **No Escaping References**: Module boxes are stored in `Patch::sampleables`
        ///    and are never shared back to the main thread after being added to the patch.
```

2. Replace the invariants block lines:

```rust
        /// - **NEVER** move live module boxes back to the main thread after publication
```

3. Change the constructor signature to:

```rust
        fn #constructor_name(id: &String, sample_rate: f32, deserialized: crate::params::DeserializedParams) -> napi::Result<Box<dyn crate::types::Sampleable>> {
```

4. Replace the constructor return statement with:

```rust
            Ok(Box::new(sampleable))
```

- [ ] **Step 4: Convert command queue payloads and audio-thread apply to owned boxes**

In `crates/modular/src/commands.rs`, replace the module ownership types with:

```rust
  pub inserts: Vec<(String, Box<dyn modular_core::types::Sampleable>)>,
```

```rust
  SingleModuleUpdate {
    module_id: String,
    module: Box<dyn Sampleable>,
  },
```

```rust
  Module(Box<dyn Sampleable>),
```

In `crates/modular/src/audio.rs`, make these exact local replacements:

1. In `GraphCommand::SingleModuleUpdate`, replace the insert/listener block with:

```rust
          self
            .runtime
            .patch
            .sampleables
            .insert(module_id.clone(), new_module);
          self.runtime.patch.add_message_listeners_for_module(&module_id);
```

2. In `apply_patch_update()`, replace old-state transfer lines with:

```rust
      if let Some(old_module) = self.runtime.patch.sampleables.remove(&id) {
        new_module.transfer_state_from(old_module.as_ref());
        self.runtime.patch.remove_message_listeners_for_module(&id);
        if self.garbage_tx.push(GarbageItem::Module(old_module)).is_err() {
        }
      }
```

3. Replace incremental listener re-registration with:

```rust
    for id in newly_inserted_ids.iter().chain(remapped_ids.iter()) {
      self.runtime.patch.add_message_listeners_for_module(id);
    }
```

4. Update `MockModule::new()` and `RecordingModule::new()` to the owned-box versions from Step 1.

In `crates/modular/src/lib.rs`, keep the same `set_module_param()` flow, but `constructor(...)` now returns `Box<dyn Sampleable>` and `GraphCommand::SingleModuleUpdate` takes that box by value.

- [ ] **Step 5: Update helper functions and tests to the new ownership type**

Apply these exact helper replacements:

In `crates/modular_core/src/dsp/utilities/buffer.rs`, replace the owner insert and helper return type with:

```rust
        patch
            .sampleables
            .insert(MOCK_BUFFER_MODULE_ID.to_string(), Box::new(owner));
```

and keep all `step(&*module);` calls on boxed sampleables.

In `crates/modular_core/src/dsp/seq/seq_value.rs`, replace helper/test ownership with:

```rust
    fn make_patch_with_sampleable(sampleable: Box<dyn Sampleable>) -> Patch {
        let mut patch = Patch::new();
        patch
            .sampleables
            .insert(sampleable.get_id().to_owned(), sampleable);
        patch
    }
```

and replace test sampleable creation with:

```rust
        let sampleable: Box<dyn Sampleable> = Box::new(DummySampleable::new(
            "lfo",
            [("output", 2.5)],
        ));
```

In `crates/modular_core/src/dsp/samplers/sampler.rs`, `crates/modular_core/src/dsp/fx/plate.rs`, and `crates/modular_core/src/dsp/fx/dattorro.rs`, keep the constructor body unchanged except the return type is now `Box<dyn Sampleable>`.

In `crates/modular_core/src/patch.rs`, replace the test insert with:

```rust
        patch.sampleables.insert(
            "m1".to_string(),
            Box::new(DummyMessageSampleable {
                id: "m1".to_string(),
            }),
        );
```

- [ ] **Step 6: Run `modular_core` tests**

Run:

```bash
cargo test -p modular_core
```

Expected: PASS.

- [ ] **Step 7: Run `modular` tests**

Run:

```bash
cargo test -p modular
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/modular_core/src/types.rs crates/modular_core/src/patch.rs crates/modular_derive/src/module_attr.rs crates/modular/src/commands.rs crates/modular/src/audio.rs crates/modular/src/lib.rs crates/modular_core/src/dsp/seq/seq_value.rs crates/modular_core/src/dsp/utilities/buffer.rs crates/modular_core/src/dsp/samplers/sampler.rs crates/modular_core/src/dsp/fx/plate.rs crates/modular_core/src/dsp/fx/dattorro.rs
git commit -m "refactor(runtime): own sampleables with boxes"
```

---

### Task 5: Drop `Sync` from `Sampleable` and assert send-only trait objects

**Files:**
- Modify: `crates/modular_core/Cargo.toml`
- Modify: `crates/modular_core/src/types.rs`
- Modify: `crates/modular_derive/src/module_attr.rs`
- Test: `cargo test -p modular_core sampleable_trait_objects_are_send_but_not_sync -- --exact`

- [ ] **Step 1: Write the failing send-only assertion test**

In `crates/modular_core/Cargo.toml`, add this dev-dependency block at the end of the file:

```toml
[dev-dependencies]
static_assertions = "1.1"
```

In `crates/modular_core/src/types.rs`, add this test inside the existing test module:

```rust
    #[test]
    fn sampleable_trait_objects_are_send_but_not_sync() {
        use static_assertions::{assert_impl_all, assert_not_impl_any};

        assert_impl_all!(Box<dyn Sampleable>: Send);
        assert_not_impl_any!(Box<dyn Sampleable>: Sync);
    }
```

- [ ] **Step 2: Run the assertion test to verify it fails**

Run:

```bash
cargo test -p modular_core sampleable_trait_objects_are_send_but_not_sync -- --exact
```

Expected: FAIL because `Sampleable` still inherits `Sync`.

- [ ] **Step 3: Remove `Sync` from the trait and generated wrapper**

In `crates/modular_core/src/types.rs`, replace the trait line with:

```rust
pub trait Sampleable: MessageHandler + Send {
```

In `crates/modular_derive/src/module_attr.rs`, delete this line entirely:

```rust
        unsafe impl Sync for #struct_name {}
```

Also update the wrapper safety comment block so it no longer claims shared `Sync` access is part of the design. Replace the opening safety paragraph with:

```rust
        /// This struct uses `UnsafeCell` instead of `Mutex`/`RwLock` for interior mutability.
        /// This is safe because module wrappers are transferred across threads but executed
        /// from one runtime owner at a time.
```

- [ ] **Step 4: Run the assertion test again**

Run:

```bash
cargo test -p modular_core sampleable_trait_objects_are_send_but_not_sync -- --exact
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/modular_core/Cargo.toml crates/modular_core/src/types.rs crates/modular_derive/src/module_attr.rs
git commit -m "refactor(core): make sampleables send-only"
```

---

### Task 6: Precompute runtime metadata before audio-thread apply

**Files:**
- Modify: `crates/modular/src/audio.rs`
- Test: `cargo test -p modular pending_runtime_metadata_ -- --nocapture`
- Test: `cargo test -p modular apply_patch_update_ -- --nocapture`
- Test: `cargo test -p modular`

- [ ] **Step 1: Write the failing pending-metadata test**

In `crates/modular/src/audio.rs`, add this test near the other pending metadata tests:

```rust
  #[test]
  fn pending_runtime_metadata_precomputes_runtime_fields() {
    let (state, _cmd_consumer, _err_producer, _garbage_producer) = create_test_state(TEST_BLOCK_SIZE);
    let patch = feed_forward_allowlist_patch();

    state
      .apply_patch(
        patch,
        TEST_SAMPLE_RATE,
        QueuedTrigger::Immediate,
        91,
        HashMap::new(),
        None,
      )
      .expect("apply_patch should succeed");

    let metadata = state.pending_region_orders.lock();
    let entry = metadata.get(&91).expect("missing pending runtime metadata");

    assert_eq!(
      entry.module_order,
      vec![
        "clamp".to_string(),
        "curve".to_string(),
        "scale".to_string(),
        "wrap".to_string(),
      ]
    );
    assert!(entry.block_region_module_ids.contains("scale"));
    assert_eq!(
      entry
        .region_output_buffers
        .outputs
        .get("scale")
        .expect("missing scale output buffers")[0]
        .len(),
      TEST_BLOCK_SIZE
    );
  }
```

- [ ] **Step 2: Run the metadata test to verify it fails**

Run:

```bash
cargo test -p modular pending_runtime_metadata_ -- --nocapture
```

Expected: FAIL to compile because `PendingSchedulerRuntimeMetadata` does not yet have `module_order`, `block_region_module_ids`, or `region_output_buffers`.

- [ ] **Step 3: Add precomputed runtime fields and helper allocation function**

In `crates/modular/src/audio.rs`, replace `PendingSchedulerRuntimeMetadata` with:

```rust
#[derive(Debug, Clone, Default)]
pub(crate) struct PendingSchedulerRuntimeMetadata {
  scheduler_debug_snapshot: SchedulerDebugSnapshot,
  scheduler_analysis: Option<SchedulerAnalysis>,
  region_order: Vec<Vec<String>>,
  region_execution_plan: Vec<SchedulerRegionExecutionPlan>,
  callback_block_size: usize,
  block_module_specs: HashMap<String, BlockModuleSpec>,
  block_region_module_ids: HashSet<String>,
  module_order: Vec<String>,
  region_output_buffers: RegionOutputBuffer,
}
```

Add this new helper right above `allocate_region_output_buffers()`:

```rust
fn allocate_region_output_buffers_from_channels(
  output_channels: &HashMap<String, usize>,
  frames: usize,
) -> RegionOutputBuffer {
  let mut outputs = HashMap::new();

  for (module_id, channels) in output_channels {
    if is_reserved_module_id(module_id) {
      continue;
    }

    outputs.insert(module_id.clone(), vec![vec![0.0; frames]; (*channels).max(1)]);
  }

  RegionOutputBuffer { outputs }
}
```

- [ ] **Step 4: Populate metadata on main thread and consume it on audio thread**

In `AudioState::apply_patch()` inside `crates/modular/src/audio.rs`, make these exact additions:

1. Right before the module-construction loop, add:

```rust
    let mut output_channels: HashMap<String, usize> = HashMap::new();
```

2. Inside the module-construction loop, right after `let deserialized = ...?;`, add:

```rust
      output_channels.insert(id.clone(), deserialized.channel_count.max(1));
```

3. Right after `let block_module_specs = compile_block_module_specs(&desired_modules);`, add:

```rust
    let mut module_order: Vec<String> = desired_modules.keys().cloned().collect();
    module_order.sort();
    let block_region_module_ids: HashSet<String> = scheduler_analysis
      .regions
      .iter()
      .filter(|region| region.mode == SchedulerRegionMode::Block)
      .flat_map(|region| region.module_ids.iter().cloned())
      .collect();
    let region_output_buffers =
      allocate_region_output_buffers_from_channels(&output_channels, callback_block_size.max(1));
```

4. When building `PendingSchedulerRuntimeMetadata`, add these fields:

```rust
        block_region_module_ids,
        module_order,
        region_output_buffers,
```

In `apply_patch_update()`, replace the runtime rebuild assignments with:

```rust
    self.runtime.block_region_module_ids = pending_runtime_metadata.block_region_module_ids;
    self.runtime.host_block_size = pending_runtime_metadata.callback_block_size.max(1);
    self.runtime.block_module_specs = pending_runtime_metadata.block_module_specs;
    self.runtime.module_order = pending_runtime_metadata.module_order;
    self.runtime.region_output_buffers = pending_runtime_metadata.region_output_buffers;
```

Delete the old recompute blocks that rebuild `block_region_module_ids`, `module_order`, and `region_output_buffers` from live patch state during apply. Leave audio-thread `connect()` in place after state transfer.

Update the local test helpers `pending_scheduler_metadata(...)` and `pending_scheduler_metadata_for_patch(...)` near the bottom of `audio.rs` so they initialize the new fields with deterministic values.

- [ ] **Step 5: Run targeted metadata/apply tests**

Run:

```bash
cargo test -p modular pending_runtime_metadata_ -- --nocapture
cargo test -p modular apply_patch_update_ -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Run full `modular` test suite**

Run:

```bash
cargo test -p modular
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/modular/src/audio.rs
git commit -m "refactor(audio): precompute runtime metadata"
```

---

### Task 7: Final verification

**Files:**
- Modify: none
- Test: `cargo test -p modular_core`
- Test: `cargo test -p modular`
- Test: `yarn typecheck`

- [ ] **Step 1: Run full core verification**

Run:

```bash
cargo test -p modular_core
```

Expected: PASS.

- [ ] **Step 2: Run full audio/runtime verification**

Run:

```bash
cargo test -p modular
```

Expected: PASS.

- [ ] **Step 3: Run repo typecheck**

Run:

```bash
yarn typecheck
```

Expected: PASS.

- [ ] **Step 4: Commit verification-only follow-ups if needed**

If verification changed code, commit it with the narrowest matching message. If verification is already clean, skip this step.

```bash
git add <files-fixed-during-verification>
git commit -m "fix: address send-only runtime regressions"
```
