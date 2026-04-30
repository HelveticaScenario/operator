# Sampleable Sync Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `Sampleable` send-only by removing its `Sync` requirement and fixing the direct runtime fallout without changing runtime behavior.

**Architecture:** The runtime already has single-owner module semantics: modules are built on the main thread, transferred once to the audio thread, then accessed only there. This plan updates the trait and generated wrapper types to match that model, removes helper `Sync` impls that only existed to support the old fake-shared graph, and uses compiler-guided cleanup for any adjacent assumptions.

**Tech Stack:** Rust, Cargo tests, proc-macro codegen in `modular_derive`, runtime/tests in `modular_core` and `modular`

---

## File Map

- Modify: `crates/modular_core/src/types.rs`
  - Change `Sampleable` trait bound from `Send + Sync` to `Send`
  - Remove runtime-helper `unsafe impl Sync` blocks that were only justified by fake shared module access
- Modify: `crates/modular_derive/src/module_attr.rs`
  - Stop generating `unsafe impl Sync` for wrapper structs
- Modify: `crates/modular_core/tests/types_tests.rs`
  - Add or update focused tests for the send-only contract if needed
- Modify: `crates/modular_core/src/patch.rs`
  - Only if compiler fallout requires updating local test helpers or trait-object assumptions
- Modify: `crates/modular_core/tests/dsp_fresh_tests.rs`
  - Only if a runtime regression test is needed for fallout discovered during the change

### Task 1: Create the RED failure for the send-only contract

**Files:**
- Modify: `crates/modular_core/src/types.rs:155-210`
- Modify: `crates/modular_derive/src/module_attr.rs:673-676`
- Test: `cargo test -p modular_core`

- [ ] **Step 1: Write the failing change in the runtime trait**

```rust
pub trait Sampleable: MessageHandler + Send {
    fn get_id(&self) -> &str;
    fn tick(&self) -> ();
    fn update(&self) -> ();
    fn get_poly_sample(&self, port: &str) -> Result<PolyOutput>;
    fn get_module_type(&self) -> &str;
    fn connect(&self, patch: &Patch);
    fn on_patch_update(&self) {}
    fn sync_external_clock(&self, _bar_phase: f64, _bpm: f64, _playing: bool) {}
    fn clear_external_sync(&self) {}
    fn get_state(&self) -> Option<serde_json::Value> {
        None
    }
    fn get_buffer_output(&self, _port: &str) -> Option<&Arc<BufferData>> {
        None
    }
    fn prepare_resources(
        &self,
        _wav_data: &std::collections::HashMap<String, std::sync::Arc<WavData>>,
    ) {
    }
    fn as_any(&self) -> &dyn std::any::Any;
    fn transfer_state_from(&self, _old: &dyn Sampleable) {}
}
```

- [ ] **Step 2: Write the failing change in the proc-macro output**

Delete this generated impl from `crates/modular_derive/src/module_attr.rs`:

```rust
unsafe impl Sync for #struct_name {}
```

Keep:

```rust
unsafe impl Send for #struct_name {}
```

- [ ] **Step 3: Run the core suite to verify the RED state**

Run: `cargo test -p modular_core`
Expected: FAIL at compile time or test time with direct fallout from removing `Sync` from `Sampleable` or generated wrappers

- [ ] **Step 4: Record the actual fallout sites before fixing anything else**

Capture the exact errors from the failed command and map them to one of these categories:

```text
1. direct trait-object bound fallout (`dyn Sampleable: Sync` still assumed)
2. manual helper `unsafe impl Sync` no longer needed (`Signal`, `Buffer`)
3. test-only assumptions about send+sync behavior
4. unrelated code - do not change unless it directly depends on Sampleable Sync
```

- [ ] **Step 5: Commit checkpoint**

Do not commit in this session. Treat the failing command output as the checkpoint for this task.

### Task 2: Remove direct runtime `Sync` assumptions

**Files:**
- Modify: `crates/modular_core/src/types.rs:1219-1247`
- Modify: `crates/modular_core/src/types.rs:1790-1805`
- Test: `cargo test -p modular_core raw_pointer_buffer_ -- --nocapture`
- Test: `cargo test -p modular_core signal_cable_ -- --nocapture`

- [ ] **Step 1: Write the focused failing test or assertion only if RED fallout is test-invisible**

If compile errors already directly point to `Signal` or `Buffer`, skip adding a new test and use the compile failure as RED.

If a focused test is needed, add a small send-only assertion helper in `crates/modular_core/tests/types_tests.rs`:

```rust
fn assert_send<T: Send>() {}

#[test]
fn signal_and_buffer_are_send() {
    assert_send::<Signal>();
    assert_send::<Buffer>();
}
```

Do **not** add a `Sync` assertion here.

- [ ] **Step 2: Remove `Sync` from `Buffer` if its only justification is fake shared-module access**

Delete this block from `crates/modular_core/src/types.rs`:

```rust
unsafe impl Sync for Buffer {}
```

Keep:

```rust
unsafe impl Send for Buffer {}
```

- [ ] **Step 3: Remove `Sync` from `Signal` if its only justification is fake shared-module access**

Delete this block from `crates/modular_core/src/types.rs`:

```rust
unsafe impl Sync for Signal {}
```

Keep the send-only semantics intact.

- [ ] **Step 4: Run the focused runtime regression tests**

Run: `cargo test -p modular_core raw_pointer_buffer_ -- --nocapture`
Expected: PASS

Run: `cargo test -p modular_core signal_cable_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit checkpoint**

Do not commit in this session. Use the passing focused test output as the checkpoint.

### Task 3: Fix compile-proven adjacent fallout only

**Files:**
- Modify: `crates/modular_core/src/types.rs`
- Modify: `crates/modular_core/src/patch.rs` if needed
- Modify: `crates/modular_core/tests/types_tests.rs` if needed
- Modify: `crates/modular_core/tests/dsp_fresh_tests.rs` if needed
- Test: `cargo test -p modular_core`

- [ ] **Step 1: Re-run the full core suite after Tasks 1-2**

Run: `cargo test -p modular_core`
Expected: either PASS or FAIL only at sites that still assume `Sampleable: Sync`

- [ ] **Step 2: Fix only direct `Sampleable Sync` fallout**

Allowed fixes:

```text
- remove now-invalid `Sync` constraints on `dyn Sampleable` users
- adjust tests that asserted old Sync behavior for runtime objects
- update helper signatures only where the compiler proves they are coupled to the old bound
```

Forbidden fixes:

```text
- proactive cleanup of unrelated generic `Send + Sync` APIs
- refactors not required to restore the send-only runtime model
- behavior changes in DSP or patch-update logic
```

- [ ] **Step 3: If a regression test is needed, add the smallest real one**

Use a targeted runtime test only if a discovered fallout bug is behavioral rather than purely type-level. Example shape:

```rust
#[test]
fn sampleable_runtime_still_reconnects_after_sync_removal() {
    let patch = Patch::new();
    assert!(patch.sampleables.contains_key("HIDDEN_AUDIO_IN"));
}
```

Prefer existing verification coverage over inventing a redundant test.

- [ ] **Step 4: Run the full core suite to verify GREEN**

Run: `cargo test -p modular_core`
Expected: PASS with 0 failures

- [ ] **Step 5: Commit checkpoint**

Do not commit in this session. Use the passing full suite as the checkpoint.

### Task 4: Verify modular runtime integration still works

**Files:**
- Modify: only if verification exposes a real breakage in `crates/modular/src/audio.rs` or `crates/modular/src/commands.rs`
- Test: `cargo test -p modular test_single_module_update_re_registers_message_listeners -- --nocapture`
- Test: `cargo test -p modular test_patch_update_remap_re_registers_message_listeners -- --nocapture`
- Test: `cargo test -p modular --no-run`

- [ ] **Step 1: Run the single-module update integration test**

Run: `cargo test -p modular test_single_module_update_re_registers_message_listeners -- --nocapture`
Expected: PASS

- [ ] **Step 2: Run the remap integration test**

Run: `cargo test -p modular test_patch_update_remap_re_registers_message_listeners -- --nocapture`
Expected: PASS

- [ ] **Step 3: Build all modular test targets**

Run: `cargo test -p modular --no-run`
Expected: PASS

- [ ] **Step 4: If one of these fails, fix only the direct ownership/trait fallout**

Allowed edits:

```text
- trait-object bounds in modular runtime code
- send-only handoff types that still assume Sync
- test helpers or wrappers directly coupled to Sampleable Sync
```

Then re-run the exact failing command before continuing.

- [ ] **Step 5: Commit checkpoint**

Do not commit in this session. Use the passing integration outputs as the checkpoint.

### Task 5: Final review and evidence capture

**Files:**
- Modify: `docs/superpowers/specs/2026-04-24-sampleable-sync-removal-design.md` only if implementation reveals a real mismatch
- Test: all commands from Tasks 3-4

- [ ] **Step 1: Compare implementation against the spec**

Check these statements explicitly:

```text
- Sampleable is Send but not Sync
- generated wrappers no longer claim Sync
- runtime helper Sync impls were only kept for truly shared data
- owned-box patch/runtime behavior is unchanged
```

- [ ] **Step 2: Re-run the fresh verification slice**

Run:

```bash
cargo test -p modular_core
cargo test -p modular test_single_module_update_re_registers_message_listeners -- --nocapture
cargo test -p modular test_patch_update_remap_re_registers_message_listeners -- --nocapture
cargo test -p modular --no-run
```

Expected: all commands PASS

- [ ] **Step 3: Update the spec only if reality differs**

If implementation required a broader cleanup than the spec describes, update the spec with the exact discovered coupling. Otherwise leave it unchanged.

- [ ] **Step 4: Summarize the completed slice**

Include:

```text
- files changed
- whether Signal/Buffer lost Sync
- whether any broader cleanup was actually required
- exact verification commands run
```

- [ ] **Step 5: Commit checkpoint**

Do not commit in this session. Final checkpoint is the passing verification slice.
