# Ableton Link Integration Design

## Overview

Integrate Ableton Link into Operator so that it can sync tempo, beat phase, and transport (start/stop) with other Link-enabled applications — including Ableton Live and other Operator instances on the local network.

**Scope:** Core Link only (tempo/beat/phase/start-stop sync). Link Audio (audio streaming between peers, new in Link 4.0 beta) is deferred until the API stabilizes and Rust bindings add support.

## Approach

**Link drives ROOT_CLOCK directly from the audio thread.** The `AblLink` instance lives in the N-API layer. `AudioProcessor` captures the Link audio session state each buffer callback and writes tempo, phase, and transport state into ROOT_CLOCK via a new `sync_external_clock()` method on the `Sampleable` trait. All downstream modules (sequencers, patterns, etc.) continue reading ROOT_CLOCK as they do now — they are unaware of Link.

### Why This Approach

- Maximum timing accuracy — Link state read and applied at sample level
- Follows Link's documented best practice (audio thread captures/commits)
- ROOT_CLOCK remains the single source of truth for all modules
- Clean separation: Link is a clock source, not a new subsystem

## Architecture

### External Clock Sync Mechanism

The `Sampleable` trait gains a new default-no-op method:

```rust
fn sync_external_clock(&self, _beat_time: f64, _bpm: f64, _playing: bool) {}
```

ROOT_CLOCK overrides this to store the values in its `UnsafeCell` state. When present, `update()` uses these values instead of self-advancing its phase. This is zero-allocation, called per-frame from `AudioProcessor`, and follows the existing `update()` pattern.

### Data Flow — Inbound (Link -> Operator)

Each audio buffer callback:

1. `AudioProcessor` captures Link audio session state (realtime-safe)
2. Uses `HostTimeFilter` to convert sample count to accurate host time
3. Per frame: calls `root_clock.sync_external_clock(beat_time, bpm, playing)` then `root_clock.update()`
4. ROOT_CLOCK derives bar phase, triggers, ramp, beat_in_bar from the Link-provided values

### Data Flow — Outbound (Operator -> Link Session)

- **Tempo change:** When ROOT_CLOCK's tempo param changes (via patch update), `AudioProcessor` detects the change and commits new tempo to Link session state.
- **Transport start/stop:** Start/Stop commands are intercepted by `AudioProcessor`, which commits playing state to Link with `request_beat_at_start_playing_time()` for quantized launch.

### Host Time Filter

Operator uses cpal, which does not provide host timestamps in the audio callback. `rusty_link` includes a `HostTimeFilter` that performs linear regression between system time and sample time to produce accurate host timestamps. Called once per buffer.

### Phase Alignment

Quantum is bar-aligned: `quantum = numerator * 4 / denominator` (beats per bar from time signature). Phase synchronization happens at bar boundaries.

## User Interface

### Link Toggle Button

A button in the transport UI area (alongside play/stop/tempo) to enable/disable Link. When enabled:

- Visual indicator shows Link is active
- Peer count is displayed
- Clicking toggles Link off, returning ROOT_CLOCK to free-running mode

Link is a system-level setting, not a patch concern. It persists across patch changes.

### TransportMeter Updates

The existing `TransportMeter` gains `link_enabled: bool` and `link_peers: u32` fields, exposed to the renderer via `TransportSnapshot`.

## Implementation Details

### Dependency

`rusty_link` v0.4.9 — Rust bindings wrapping the official `abl_link` C extension. Build requires CMake 3.14+. License: GPL-2.0+.

### Command Queue

New `GraphCommand::EnableLink(bool)` variant sent from main thread when UI toggle is clicked.

### Error Handling & Edge Cases

- **Enable transition:** Create/enable `AblLink` on main thread, send command to audio thread. Audio thread begins calling `sync_external_clock()`.
- **Disable transition:** Audio thread stops calling `sync_external_clock()`. ROOT_CLOCK resumes free-running, preserving current tempo and phase.
- **No peers:** Operator is sole peer. Link reflects its own values. No special handling.
- **Quantized launch:** Starting transport with Link active uses `request_beat_at_start_playing_time()`.
- **Sample rate changes:** `HostTimeFilter` is reset. Link session survives.

## Testing

### Unit Tests (Rust)

- ROOT_CLOCK with `sync_external_clock`: verify outputs match externally-provided phase/tempo
- Mode transitions: free-running -> synced and back, no phase discontinuity
- Trigger generation from externally-driven phase

### Integration Tests

- Real `AblLink` instance with `enable(false)` to verify API surface

### E2E Tests

- Link button appears in transport UI
- Clicking toggles Link on/off
- Peer count displays

## Future Work

- **Link Audio:** Add audio streaming once Link 4.0 stabilizes and `rusty_link` adds support
- **Configurable quantum:** Allow users to override quantum independently of time signature
