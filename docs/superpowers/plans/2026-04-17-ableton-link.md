# Ableton Link Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate Ableton Link into Operator for tempo/beat/phase/transport sync with other Link-enabled apps.

**Architecture:** `rusty_link` provides the Link session. `AudioProcessor` captures Link state each buffer and drives ROOT_CLOCK via a new `sync_external_clock()` trait method. A UI button toggles Link on/off, with peer count shown in the transport display.

**Tech Stack:** `rusty_link` 0.4.9 (Rust bindings for Ableton Link SDK), CMake 3.14+ (build dep)

---

### Task 1: Add `rusty_link` dependency

**Files:**

- Modify: `crates/modular/Cargo.toml`

- [ ] **Step 1: Add rusty_link to Cargo.toml**

Add after the `[dependencies.modular_core]` section in `crates/modular/Cargo.toml`:

```toml
[dependencies.rusty_link]
version = "0.4"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p modular`
Expected: success (requires CMake 3.14+ installed)

- [ ] **Step 3: Commit**

```bash
git add crates/modular/Cargo.toml Cargo.lock
git commit -m "feat(link): add rusty_link dependency"
```

---

### Task 2: Add `sync_external_clock` to `Sampleable` trait

**Files:**

- Modify: `crates/modular_core/src/types.rs:153-192`
- Test: `crates/modular_core/src/dsp/core/clock.rs` (existing tests still pass)

- [ ] **Step 1: Write failing test — Clock with external sync overrides free-running**

Add to the `#[cfg(test)] mod tests` block at the bottom of `crates/modular_core/src/dsp/core/clock.rs`:

```rust
#[test]
fn clock_external_sync_overrides_phase() {
    let mut c = Clock::default();
    let sr = 48_000.0;

    // Advance clock to some non-zero phase
    for _ in 0..12_000 {
        c.update(sr);
    }
    let free_phase = c.state.phase;
    assert!(free_phase > 0.0);

    // Now sync to a specific phase externally
    c.sync_external_clock(0.75, 140.0, true);
    c.update(sr);

    // Phase should be near 0.75 (the externally-set value), not free-running
    // Allow small increment from one sample advance
    assert!(
        (c.state.phase - 0.75).abs() < 0.01,
        "Phase should be near 0.75 after external sync, got {}",
        c.state.phase
    );
}

#[test]
fn clock_external_sync_clears_on_none() {
    let mut c = Clock::default();
    let sr = 48_000.0;

    // Sync externally
    c.sync_external_clock(0.5, 120.0, true);
    c.update(sr);

    // Clear external sync
    c.clear_external_sync();
    let phase_before = c.state.phase;
    c.update(sr);

    // Should advance freely from where it was
    assert!(
        c.state.phase > phase_before,
        "Clock should free-run after clearing external sync"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p modular_core clock_external_sync -- --nocapture`
Expected: FAIL — `sync_external_clock` and `clear_external_sync` methods don't exist

- [ ] **Step 3: Add `sync_external_clock` default no-op to Sampleable trait**

In `crates/modular_core/src/types.rs`, add after `fn on_patch_update(&self) {}` (line 163):

```rust
    /// Provide external clock synchronization data.
    /// Only ROOT_CLOCK overrides this. Default: no-op.
    fn sync_external_clock(&self, _bar_phase: f64, _bpm: f64, _playing: bool) {}

    /// Clear external clock synchronization, returning to free-running mode.
    fn clear_external_sync(&self) {}
```

- [ ] **Step 4: Add external sync state to Clock module**

In `crates/modular_core/src/dsp/core/clock.rs`, add a new field to `ClockState`:

```rust
/// External clock sync data (set by AudioProcessor when Link is active).
/// When Some, update() uses these values instead of self-advancing.
external_sync: Option<ExternalClockSync>,
```

Add the struct before `ClockState`:

```rust
#[derive(Clone, Copy)]
struct ExternalClockSync {
    bar_phase: f64,
    bpm: f64,
    playing: bool,
}
```

Initialize `external_sync: None` in `ClockState::default()`.

- [ ] **Step 5: Implement `sync_external_clock` and `clear_external_sync` on Clock**

Add methods to `impl Clock`:

```rust
fn sync_external_clock(&mut self, bar_phase: f64, bpm: f64, playing: bool) {
    self.state.external_sync = Some(ExternalClockSync {
        bar_phase,
        bpm,
        playing,
    });
}

fn clear_external_sync(&mut self) {
    self.state.external_sync = None;
}
```

- [ ] **Step 6: Modify Clock::update() to use external sync when present**

At the top of `Clock::update()`, before the existing code, add:

```rust
if let Some(sync) = self.state.external_sync.take() {
    if !sync.playing {
        if self.state.running {
            // Transition to stopped
            self.state.running = false;
            self.outputs.bar_trigger = 0.0;
            self.outputs.beat_trigger = 0.0;
            self.outputs.ppq_trigger = 0.0;
        }
        return;
    }

    if !self.state.running {
        self.state.running = true;
    }

    // Override tempo
    self.params.tempo = sync.bpm;

    // Compute time signature values
    let numerator = self.params.numerator.max(1) as f64;
    let denominator = self.params.denominator.max(1) as f64;

    // Set phase directly from Link
    let old_phase = self.state.phase;
    self.state.phase = sync.bar_phase.rem_euclid(1.0);

    // Detect bar boundary crossing (phase wrapped)
    if self.state.phase < old_phase && old_phase > 0.5 {
        self.state.loop_index += 1;
    }

    // Derive beat and PPQ phases from bar phase
    let beat_period = 1.0 / numerator;
    self.state.beat_phase = self.state.phase.rem_euclid(beat_period);

    let quarter_notes_per_bar = numerator * 4.0 / denominator;
    let ppq_period = 1.0 / (12.0 * quarter_notes_per_bar);
    self.state.ppq_phase = self.state.phase.rem_euclid(ppq_period);

    // Update outputs
    self.outputs.beat_in_bar = (self.state.phase * numerator).floor() as f32;
    self.outputs.playhead.set(0, self.state.phase as f32);
    self.outputs.playhead.set(1, self.state.loop_index as f32);
    self.outputs.ramp = self.state.phase as f32 * 5.0;

    // Generate triggers using the existing SchmittTrigger + TempGate mechanism
    let bpm = sync.bpm.max(1.0);
    let frequency_hz = bpm / 60.0;
    let bar_frequency = frequency_hz / quarter_notes_per_bar;
    let phase_increment = bar_frequency / self.params.tempo.max(1.0) * (sync.bpm / 60.0);
    // Use a simpler approach: detect wrap-around for triggers
    let hold = min_gate_samples(48000.0); // TODO: pass sample_rate properly

    // Bar trigger: detect when phase wraps (old > 0.9, new < 0.1)
    if self.state.phase < 0.1 && old_phase > 0.9 {
        self.state
            .bar_gate
            .set_state(TempGateState::High, TempGateState::Low, hold);
    }
    self.outputs.bar_trigger = self.state.bar_gate.process();

    // Beat trigger: detect beat boundary crossing
    let old_beat = (old_phase * numerator).floor();
    let new_beat = (self.state.phase * numerator).floor();
    if new_beat != old_beat || (self.state.phase < 0.1 && old_phase > 0.9) {
        self.state
            .beat_gate
            .set_state(TempGateState::High, TempGateState::Low, hold);
    }
    self.outputs.beat_trigger = self.state.beat_gate.process();

    // PPQ trigger: detect PPQ boundary crossing
    let ppq_per_bar = 12.0 * quarter_notes_per_bar;
    let old_ppq = (old_phase * ppq_per_bar).floor();
    let new_ppq = (self.state.phase * ppq_per_bar).floor();
    if new_ppq != old_ppq || (self.state.phase < 0.1 && old_phase > 0.9) {
        self.state
            .ppq_gate
            .set_state(TempGateState::High, TempGateState::Low, hold);
    }
    self.outputs.ppq_trigger = self.state.ppq_gate.process();

    return;
}
```

- [ ] **Step 7: Wire `sync_external_clock` and `clear_external_sync` through the Sampleable proc macro**

The proc macro generates the `Sampleable` impl for the wrapper type. The `sync_external_clock` and `clear_external_sync` methods need to be forwarded from the wrapper's `Sampleable` impl to the inner `Clock` methods.

Check `crates/modular_derive/src/module_attr.rs` — the proc macro generates default trait methods. Since `Sampleable` has default no-op implementations, only ROOT_CLOCK needs to override. Add a new optional attribute or hardcode for `_clock` module type.

The simplest approach: add `sync_external_clock` and `clear_external_sync` to the generated `Sampleable` impl for ALL modules (calling the inner module's method if it exists, otherwise the default no-op). Since the trait has a default, the generated code just needs to forward for Clock.

In the generated wrapper's `Sampleable` impl, add:

```rust
fn sync_external_clock(&self, bar_phase: f64, bpm: f64, playing: bool) {
    let module = unsafe { &mut *self.module.get() };
    module.sync_external_clock(bar_phase, bpm, playing);
}

fn clear_external_sync(&self) {
    let module = unsafe { &mut *self.module.get() };
    module.clear_external_sync();
}
```

This requires adding default no-op methods to ALL inner module types. The easiest way: define the methods on a trait that all modules get via a blanket impl, or just add default methods. Since the proc macro already generates code, add default no-ops in the macro for modules that don't define these methods.

**Alternative (simpler):** Don't forward through the proc macro. Instead, override the `Sampleable` trait method only for `_clock`. In the proc macro, check if the inner module has `sync_external_clock` defined and forward it; otherwise use the default no-op from the trait. Since the trait already has a default, simply don't generate an override for most modules — only generate the override for modules that implement the method.

**Simplest approach:** Add a `clock_sync` flag to the `#[module(...)]` attribute, similar to `patch_update`. Only `_clock` sets this flag. The proc macro generates the forwarding methods only when the flag is set.

- [ ] **Step 8: Run tests**

Run: `cargo test -p modular_core clock_external_sync -- --nocapture`
Expected: PASS

- [ ] **Step 9: Run all existing clock tests to verify no regression**

Run: `cargo test -p modular_core clock -- --nocapture`
Expected: all existing tests PASS

- [ ] **Step 10: Commit**

```bash
git add crates/modular_core/src/types.rs crates/modular_core/src/dsp/core/clock.rs crates/modular_derive/src/module_attr.rs
git commit -m "feat(link): add sync_external_clock to Sampleable trait and Clock module"
```

---

### Task 3: Add Link state to AudioProcessor

**Files:**

- Modify: `crates/modular/src/audio.rs:1173-1194` (AudioProcessor struct)
- Modify: `crates/modular/src/audio.rs:1436-1449` (process_frame)
- Modify: `crates/modular/src/audio.rs:1630-1640` (audio callback)
- Modify: `crates/modular/src/commands.rs:83-111` (GraphCommand)

- [ ] **Step 1: Add `EnableLink` variant to GraphCommand**

In `crates/modular/src/commands.rs`, add to the `GraphCommand` enum after `ClearPatch`:

```rust
  /// Enable or disable Ableton Link synchronization
  EnableLink(bool),
```

- [ ] **Step 2: Add Link fields to AudioProcessor**

In `crates/modular/src/audio.rs`, add fields to `AudioProcessor` struct:

```rust
  /// Ableton Link instance (None when not enabled)
  link: Option<rusty_link::AblLink>,
  /// Host time filter for accurate timestamps with cpal
  host_time_filter: Option<rusty_link::HostTimeFilter>,
  /// Running sample count for HostTimeFilter
  sample_count: f64,
  /// Sample rate for Link time calculations
  sample_rate: f32,
  /// Last known tempo from ROOT_CLOCK (for detecting changes to propagate to Link)
  last_link_tempo: f64,
```

Initialize all to `None`/`0.0`/`120.0` in `AudioProcessor::new()`. Pass `sample_rate` from the audio stream config.

- [ ] **Step 3: Handle `EnableLink` command in process_commands**

In `AudioProcessor::process_commands()`, add a match arm:

```rust
GraphCommand::EnableLink(enabled) => {
    if enabled {
        let bpm = self.last_link_tempo;
        let link = rusty_link::AblLink::new(bpm);
        link.enable(true);
        link.enable_start_stop_sync(true);
        self.host_time_filter = Some(rusty_link::HostTimeFilter::new());
        self.sample_count = 0.0;
        self.link = Some(link);
    } else {
        // Disable: clear external sync on ROOT_CLOCK
        if let Some(root_clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) {
            root_clock.clear_external_sync();
        }
        self.link = None;
        self.host_time_filter = None;
    }
}
```

- [ ] **Step 4: Add per-buffer Link state capture**

In the audio callback, after `process_commands()` and before the frame loop, add Link state capture. This needs to happen in `AudioProcessor` — add a method `capture_link_state()`:

```rust
/// Capture Link session state for this buffer. Called once per audio callback.
/// Returns (beat_time_at_buffer_start, bpm, playing, quantum, micros_per_sample)
/// or None if Link is not active.
fn capture_link_state(
    &mut self,
    num_frames: usize,
) -> Option<LinkBufferState> {
    let link = self.link.as_ref()?;
    let htf = self.host_time_filter.as_mut()?;

    // Get host time for this buffer using HostTimeFilter
    let host_time_micros = htf.sample_time_to_host_time(self.sample_count);

    // Capture realtime-safe audio session state
    let mut session_state = link.capture_audio_session_state();

    // Read quantum from ROOT_CLOCK's time signature
    let quantum = if let Some(clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) {
        // We can't read params directly, but we can compute from transport_meter
        // which has numerator/denominator. For now, use 4.0 as default.
        // TODO: Read from transport_meter or pass via command
        4.0
    } else {
        4.0
    };

    let tempo = session_state.tempo();
    let playing = session_state.is_playing();

    // Detect local tempo changes and propagate to Link
    if (tempo - self.last_link_tempo).abs() > 0.01 {
        // Link tempo changed from a peer — we'll follow it
    }

    Some(LinkBufferState {
        session_state,
        host_time_micros,
        quantum,
        tempo,
        playing,
        micros_per_sample: 1_000_000.0 / self.sample_rate as f64,
    })
}
```

Define `LinkBufferState` struct:

```rust
struct LinkBufferState {
    session_state: rusty_link::SessionState,
    host_time_micros: f64,
    quantum: f64,
    tempo: f64,
    playing: bool,
    micros_per_sample: f64,
}
```

- [ ] **Step 5: Drive ROOT_CLOCK from Link in process_frame**

Modify `process_frame()` — before the ROOT_CLOCK update (line 1447), if Link is active, call `sync_external_clock`:

```rust
// 1. Update ROOT_CLOCK — either from Link or free-running
if let Some(root_clock) = self.patch.sampleables.get(&*ROOT_CLOCK_ID) {
    if let Some(ref link_state) = self.current_link_state {
        // Calculate host time for this specific frame
        let frame_host_time = link_state.host_time_micros
            + (self.frame_in_buffer as f64 * link_state.micros_per_sample);

        let beat_time = link_state
            .session_state
            .beat_at_time(frame_host_time as i64, link_state.quantum);
        let phase = link_state
            .session_state
            .phase_at_time(frame_host_time as i64, link_state.quantum);

        // Convert Link phase (0..quantum in beats) to bar phase (0..1)
        let bar_phase = phase / link_state.quantum;

        root_clock.sync_external_clock(bar_phase, link_state.tempo, link_state.playing);
    }
    root_clock.update();
}
```

Add `current_link_state: Option<LinkBufferState>` and `frame_in_buffer: usize` fields to `AudioProcessor`. Set `current_link_state` from `capture_link_state()` at the start of each buffer callback. Increment `frame_in_buffer` each frame, reset to 0 at buffer start.

- [ ] **Step 6: Update sample count after each buffer**

At the end of the audio callback, after all frames are processed:

```rust
if self.link.is_some() {
    self.sample_count += num_frames as f64;
}
```

Where `num_frames = output.len() / num_channels`.

- [ ] **Step 7: Propagate tempo changes from Operator to Link**

In `process_frame()`, after ROOT_CLOCK updates, detect if tempo was changed locally (via patch update) and commit to Link:

```rust
// After ROOT_CLOCK update, propagate tempo changes to Link
if let Some(ref link) = self.link {
    if let Some(ref link_state) = self.current_link_state {
        let current_tempo = self.transport_meter_bpm(); // Read from transport meter or ROOT_CLOCK output
        if (current_tempo - self.last_link_tempo).abs() > 0.01 {
            let mut ss = link.capture_audio_session_state();
            let frame_time = link_state.host_time_micros
                + (self.frame_in_buffer as f64 * link_state.micros_per_sample);
            ss.set_tempo(current_tempo, frame_time as i64);
            link.commit_audio_session_state(ss);
            self.last_link_tempo = current_tempo;
        }
    }
}
```

- [ ] **Step 8: Handle Start/Stop with Link**

In `process_commands()`, modify the `Start` and `Stop` handling:

```rust
GraphCommand::Start => {
    if let Some(ref link) = self.link {
        let mut ss = link.capture_audio_session_state();
        let time = link.clock_micros();
        ss.set_is_playing_and_request_beat_at_time(true, time, 0.0, self.current_quantum());
        link.commit_audio_session_state(ss);
    }
    // Still dispatch ClockMessages::Start to ROOT_CLOCK
    let msg = Message::Clock(modular_core::types::ClockMessages::Start);
    let _ = self.patch.dispatch_message(&msg);
}
GraphCommand::Stop => {
    if let Some(ref link) = self.link {
        let mut ss = link.capture_audio_session_state();
        let time = link.clock_micros();
        ss.set_is_playing(false, time);
        link.commit_audio_session_state(ss);
    }
}
```

- [ ] **Step 9: Run cargo check**

Run: `cargo check -p modular`
Expected: success

- [ ] **Step 10: Commit**

```bash
git add crates/modular/src/audio.rs crates/modular/src/commands.rs
git commit -m "feat(link): integrate Link session state capture in AudioProcessor"
```

---

### Task 4: Add Link N-API methods and TransportMeter fields

**Files:**

- Modify: `crates/modular/src/audio.rs:1965-2084` (TransportMeter, TransportSnapshot)
- Modify: `crates/modular/src/lib.rs` (Synthesizer N-API methods)

- [ ] **Step 1: Add Link fields to TransportMeter**

In `TransportMeter` struct, add:

```rust
  /// Whether Ableton Link is currently enabled
  link_enabled: AtomicBool,
  /// Number of Link peers in the session
  link_peers: AtomicU32,
```

Initialize in `Default`: `link_enabled: AtomicBool::new(false)`, `link_peers: AtomicU32::new(0)`.

Add writer methods:

```rust
#[inline]
pub fn write_link_state(&self, enabled: bool, peers: u32) {
    self.link_enabled.store(enabled, Ordering::Relaxed);
    self.link_peers.store(peers, Ordering::Relaxed);
}
```

- [ ] **Step 2: Add Link fields to TransportSnapshot**

```rust
  /// Whether Ableton Link is enabled
  pub link_enabled: bool,
  /// Number of Link peers
  pub link_peers: u32,
```

Update `snapshot()` to read these fields.

- [ ] **Step 3: Add `enable_link` N-API method to Synthesizer**

In `crates/modular/src/lib.rs`, add inside `#[napi] impl Synthesizer`:

```rust
#[napi]
pub fn enable_link(&self, enabled: bool) -> napi::Result<()> {
    self.state.send_command(GraphCommand::EnableLink(enabled))?;
    self.state.transport_meter.write_link_state(enabled, 0);
    Ok(())
}
```

- [ ] **Step 4: Update AudioProcessor to write Link peer count to TransportMeter**

In the audio callback, after `capture_link_state()`, write peer count:

```rust
if let Some(ref link) = self.link {
    self.transport_meter.write_link_state(true, link.num_peers() as u32);
}
```

- [ ] **Step 5: Rebuild N-API types**

Run: `yarn build-native && yarn generate-lib`

This refreshes `crates/modular/index.d.ts` with the new `enableLink` method and updated `TransportSnapshot`.

- [ ] **Step 6: Commit**

```bash
git add crates/modular/src/audio.rs crates/modular/src/lib.rs
git commit -m "feat(link): add Link enable/disable N-API method and TransportMeter fields"
```

---

### Task 5: Add IPC channel for Link toggle

**Files:**

- Modify: `src/shared/ipcTypes.ts`
- Modify: `src/main/main.ts`
- Modify: `src/preload/preload.ts`

- [ ] **Step 1: Add IPC channel constant**

In `src/shared/ipcTypes.ts`, add to `IPC_CHANNELS` (after `SYNTH_GET_TRANSPORT_STATE`):

```typescript
    SYNTH_ENABLE_LINK: 'modular:synth:enable-link',
```

Add to `IPCHandlers` type map:

```typescript
    [IPC_CHANNELS.SYNTH_ENABLE_LINK]: typeof Synthesizer.prototype.enableLink;
```

- [ ] **Step 2: Register IPC handler in main process**

In `src/main/main.ts`, add after the `SYNTH_GET_TRANSPORT_STATE` handler:

```typescript
registerIPCHandler('SYNTH_ENABLE_LINK', (enabled: boolean) => {
    synth.enableLink(enabled);
});
```

- [ ] **Step 3: Expose in preload bridge**

In `src/preload/preload.ts`, add to the `synth` object in the `contextBridge.exposeInMainWorld` call:

```typescript
enableLink: Promisify<IPCHandlers[typeof IPC_CHANNELS.SYNTH_ENABLE_LINK]>;
```

And in the implementation:

```typescript
        enableLink: (...args) => invokeIPC('SYNTH_ENABLE_LINK', ...args),
```

- [ ] **Step 4: Commit**

```bash
git add src/shared/ipcTypes.ts src/main/main.ts src/preload/preload.ts
git commit -m "feat(link): add IPC channel for Link enable/disable"
```

---

### Task 6: Add Link toggle button to TransportDisplay

**Files:**

- Modify: `src/renderer/components/TransportDisplay.tsx`

- [ ] **Step 1: Add Link toggle button**

In `TransportDisplay.tsx`, add a Link toggle button after the queued update indicator. The component needs access to the `enableLink` IPC call. Update the props interface:

```typescript
interface TransportDisplayProps {
    transport: TransportSnapshot | null;
    onToggleLink?: (enabled: boolean) => void;
}
```

Add the Link UI element inside the return JSX, before the closing `</div>`:

```tsx
{
    /* Link toggle */
}
<button
    className={`transport-link${transport.linkEnabled ? ' active' : ''}`}
    onClick={() => onToggleLink?.(!transport.linkEnabled)}
    title={
        transport.linkEnabled
            ? `Link active (${transport.linkPeers} peer${transport.linkPeers !== 1 ? 's' : ''})`
            : 'Enable Ableton Link'
    }
>
    Link
    {transport.linkEnabled && transport.linkPeers > 0 && (
        <span className="transport-link-peers">{transport.linkPeers}</span>
    )}
</button>;
```

- [ ] **Step 2: Wire up in App.tsx**

In `src/renderer/App.tsx`, pass the callback to `TransportDisplay`:

```tsx
<TransportDisplay
    transport={transportState}
    onToggleLink={(enabled) => window.modular.synth.enableLink(enabled)}
/>
```

- [ ] **Step 3: Add CSS for Link button**

Find the CSS file that styles `.transport-display` (likely in `src/renderer/` styles). Add:

```css
.transport-link {
    font-size: 10px;
    padding: 2px 6px;
    border: 1px solid var(--border-color, #555);
    border-radius: 3px;
    background: transparent;
    color: var(--text-dim, #888);
    cursor: pointer;
    margin-left: 8px;
}

.transport-link.active {
    background: var(--accent-color, #ff764d);
    color: var(--text-bright, #fff);
    border-color: var(--accent-color, #ff764d);
}

.transport-link-peers {
    margin-left: 4px;
    font-size: 9px;
    opacity: 0.8;
}
```

- [ ] **Step 4: Commit**

```bash
git add src/renderer/components/TransportDisplay.tsx src/renderer/App.tsx
git commit -m "feat(link): add Link toggle button to transport UI"
```

---

### Task 7: Integration testing and verification

**Files:**

- Modify: `crates/modular_core/src/dsp/core/clock.rs` (additional edge case tests)

- [ ] **Step 1: Add edge case tests for external clock sync**

Add to `clock.rs` tests:

```rust
#[test]
fn clock_external_sync_generates_beat_triggers() {
    let mut c = Clock::default();
    let sr = 48_000.0;

    // Simulate Link driving the clock through one full bar
    // 120 BPM, 4/4 time = 96000 samples per bar
    let samples_per_bar = 96_000;
    let mut beat_triggers = 0;
    let mut was_high = false;

    for i in 0..samples_per_bar {
        let bar_phase = i as f64 / samples_per_bar as f64;
        c.sync_external_clock(bar_phase, 120.0, true);
        c.update(sr);

        let is_high = c.outputs.beat_trigger == 5.0;
        if is_high && !was_high {
            beat_triggers += 1;
        }
        was_high = is_high;
    }

    assert_eq!(
        beat_triggers, 4,
        "External sync should generate 4 beat triggers per bar in 4/4, got {}",
        beat_triggers
    );
}

#[test]
fn clock_external_sync_transport_stop_clears_triggers() {
    let mut c = Clock::default();
    let sr = 48_000.0;

    // Run a few frames synced
    for i in 0..100 {
        c.sync_external_clock(i as f64 / 96000.0, 120.0, true);
        c.update(sr);
    }

    // Stop transport
    c.sync_external_clock(0.0, 120.0, false);
    c.update(sr);

    assert_eq!(c.outputs.bar_trigger, 0.0, "Triggers should be 0 when stopped");
    assert_eq!(c.outputs.beat_trigger, 0.0, "Beat trigger should be 0 when stopped");
}
```

- [ ] **Step 2: Run all clock tests**

Run: `cargo test -p modular_core clock -- --nocapture`
Expected: all PASS

- [ ] **Step 3: Run full Rust test suite**

Run: `cargo test -p modular_core`
Expected: all PASS

- [ ] **Step 4: Build native and verify TypeScript**

Run: `yarn build-native && yarn typecheck`
Expected: success

- [ ] **Step 5: Commit any test additions**

```bash
git add crates/modular_core/src/dsp/core/clock.rs
git commit -m "test(link): add edge case tests for external clock sync"
```

---

### Task 8: Manual testing with Ableton Live or another Link app

This task is for manual verification — not automated.

- [ ] **Step 1: Launch Operator**

Run: `yarn start`

- [ ] **Step 2: Click the Link button in the transport bar**

Verify: Button activates, shows "Link" with active styling.

- [ ] **Step 3: Open Ableton Live (or LinkHut example app) on the same network**

Verify: Peer count increments in Operator's transport display.

- [ ] **Step 4: Change tempo in Live**

Verify: Operator's tempo follows.

- [ ] **Step 5: Change tempo in Operator (via $setTempo)**

Verify: Live's tempo follows.

- [ ] **Step 6: Start/stop transport in both directions**

Verify: Start/stop state syncs bidirectionally with quantized launch.

- [ ] **Step 7: Disable Link**

Verify: Operator returns to free-running mode, peer count disappears.
