# Wavetable Oscillator & Table Type Design

**Date:** 2026-04-16
**Status:** Approved

## Overview

Add a `$wavetable` DSP module and a new `Table` param type to the modular synth. The wavetable oscillator reads WAV files (loaded via the existing `$wavs()` system) as multi-frame wavetables with mipmap-based anti-aliasing. The Table type provides signal-driven phase warping, enabling classic wavetable warp effects (mirror, bend, sync, fold, PWM). Table composition (chaining) is deferred to a future iteration.

## DSL API

```javascript
// Basic: pitch + position
$wavetable($wavs().pad, $note('C4'), 0.5).out();

// Modulated position via LFO
$wavetable($wavs().strings.warm, note, lfo.sine(0.2).range(0, 5)).out();

// Phase warping with static amount
$wavetable($wavs().pad, note, position, { phase: $table.mirror(0.7) }).out();

// Phase warping with signal-driven amount
$wavetable($wavs().pad, note, position, { phase: $table.bend(lfo.out) }).out();
```

**Signature:** `$wavetable(wav, pitch, position, config?)`

| Param          | Type               | Description                                                 |
| -------------- | ------------------ | ----------------------------------------------------------- |
| `wav`          | `WavHandle`        | From `$wavs()`. Triggers WAV loading via existing pipeline. |
| `pitch`        | polysignal         | V/Oct pitch input (0V = C4).                                |
| `position`     | polysignal (0–5V)  | Frame morph position. 0V = first frame, 5V = last frame.    |
| `config.phase` | `Table` (optional) | Phase warp table. Default: identity (linear).               |

**Output:** Single polyphonic audio output. Channel count derived from max of `pitch` and `position` channel counts. Always reads channel 0 of wav data.

## Table Type

### Concept

A `Table` is an immutable function object — a param type (like `Wav` or `Buffer`) that the consuming module evaluates per-sample. It is NOT a DSP module and has no graph node identity. Tables map `f(x) → y` where `x ∈ [0,1]` and `y ∈ [0,1]`.

Tables can hold `PolySignal` fields for dynamic parameters (e.g. an LFO driving skew amount). These signals are resolved during the consuming module's `connect()`, and read by the table during `evaluate()`. This extends the existing concept of `Vec<PolySignal>` as a connected container of signals — a table is a connected container of signals plus an evaluate function.

### Rust Enum

```rust
#[derive(Deserialize, Deserr, JsonSchema)]
#[serde(tag = "type")]
pub enum Table {
    Identity,
    Mirror { amount: PolySignal },
    Bend { amount: PolySignal },
    Sync { ratio: PolySignal },
    Fold { amount: PolySignal },
    Pwm { width: PolySignal },
}

impl Table {
    /// Evaluate the table at position x for the given channel.
    /// Reads current signal values from embedded PolySignals.
    pub fn evaluate(&self, x: f32, channel: usize) -> f32 { ... }

    /// Resolve embedded PolySignal connections (called by consuming module's connect).
    pub fn connect(&mut self, patch: &Patch) { ... }
}
```

### Serialization

Tables serialize as tagged JSON objects within the consuming module's params:

```json
{ "type": "Identity" }
{ "type": "Mirror", "amount": 0.7 }
{ "type": "Bend", "amount": { "type": "mono_signal", "module": "lfo", "output": "out" } }
```

Signal params can be either literal numbers (constant value) or signal references (resolved during connect). This matches how `PolySignal` already serializes.

### DSL Helpers ($table namespace)

`$table.*` functions are DSL helpers defined in `executor.ts` (like `$hz`, `$note`). They return JSON objects — NOT graph nodes.

- `$table.mirror(0.7)` → `{ "type": "Mirror", "amount": 0.7 }`
- `$table.mirror(lfo.out)` → `{ "type": "Mirror", "amount": <signal_ref_for_lfo.out> }`
- `$table.bend(0.3)` → `{ "type": "Bend", "amount": 0.3 }`

### Table Variants

| Variant                 | Signal Params | Behavior                                                        |
| ----------------------- | ------------- | --------------------------------------------------------------- |
| `$table.mirror(amount)` | amount        | Reflects waveform at midpoint. Amount controls intensity.       |
| `$table.bend(amount)`   | amount        | Asymmetric phase distortion (skew).                             |
| `$table.sync(ratio)`    | ratio         | Hard sync effect. Resets phase at `ratio` times base frequency. |
| `$table.fold(amount)`   | amount        | Wave folding — folds phase back at boundaries.                  |
| `$table.pwm(width)`     | width         | Pulse-width-like phase shift.                                   |

Table composition (chaining tables) is deferred to a future iteration. Each table is standalone for v1.

## Wavetable Module — Rust DSP

### Module Definition

```rust
#[module(
    name = "$wavetable",
    channels_derive = wavetable_derive_channel_count,
    args(wav, pitch, position)
)]
pub struct WavetableOsc {
    params: WavetableParams,
    outputs: WavetableOutputs,  // { out: PolyOutput }
    state: WavetableState,
}
```

### Params

```rust
struct WavetableParams {
    wav: Wav,
    pitch: MonoSignal,
    position: MonoSignal,
    phase: Option<Table>,

    #[serde(skip)] #[schemars(skip)]
    prepared: Option<PreparedWavetable>,
}
```

`prepared` is populated by `prepare_resources()` on the main thread after construction.

### State

```rust
struct WavetableState {
    phases: [f64; MAX_CHANNELS],
}
```

Phases preserved across patch updates via `transfer_state_from()`.

### PreparedWavetable

```rust
struct PreparedWavetable {
    levels: Vec<Vec<f32>>,  // [mipmap_level][frame * frame_size + sample]
    frame_size: usize,
    frame_count: usize,
    mipmap_count: usize,
    base_frequency: f32,    // sr / frame_size
}
```

### Processing (per sample, per channel)

1. Read `pitch` → V/Oct to frequency
2. Read `position` → scale 0–5V to 0.0–1.0 → frame float index
3. Phase increment: `freq / sample_rate`
4. Warp phase: `warped = self.params.phase.evaluate(raw_phase, ch)` (or just `raw_phase` if None)
5. Mipmap level: `log2(freq / base_frequency)`, clamped
6. Linear interpolation within frame at warped phase
7. Linear crossfade between adjacent frames at fractional position
8. Advance phase, wrap at 1.0

### Channel Count

`max(pitch.channels, position.channels)`.

## Resource Preparation

### `prepare_resources` Trait Method

New method on `Sampleable` trait (default no-op):

```rust
fn prepare_resources(&mut self, wav_data: &HashMap<String, Arc<WavData>>) {}
```

Called in `AudioState::apply_patch()` on main thread after module construction, before queueing to audio thread.

### Frame Size Detection

Happens during `WavCache::load()` — parse raw WAV chunks before `hound` decodes audio:

1. CLM chunk (Serum format) → parse ASCII frame size after `<!>` prefix (e.g. `<!>2048 ...`)
2. uhWT chunk (Hive format) → assume 2048-sample frames (payload format undocumented)
3. srge chunk (Surge format) → 8 bytes: int32 version (1) + int32 frame_size
4. Fallback: 2048

Store `detected_frame_size: Option<usize>` on `WavData`.

### Mipmap Generation

During `prepare_resources()` on main thread:

1. Split wav data into frames using detected frame size
2. For each frame: FFT → for each octave: zero bins above Nyquist → IFFT → store
3. ~`log2(frame_size)` mipmap levels

Uses `rustfft` crate.

## Cache Invalidation

Add `mtime` (epoch milliseconds) to wav_ref JSON:

```json
{ "type": "wav_ref", "path": "pad", "channels": 1, "mtime": 1713200000000 }
```

When a wav file changes, mtime changes → wav_ref JSON changes → params cache key changes → fresh deserialization + wavetable preparation.

## Testing

| Test                       | What it verifies                                          |
| -------------------------- | --------------------------------------------------------- |
| Frame detection unit tests | CLM/uhWT/srge chunk parsing, fallback                     |
| Mipmap generation tests    | FFT band-limiting correctness, level count                |
| Table enum tests           | Each variant produces expected output for given inputs    |
| Table connect test         | PolySignal params resolve correctly in consumer connect() |
| $table DSL test            | $table.mirror(0.5) etc. serialize correctly               |
| WavetableOsc process test  | Pitch tracking, position morphing, mipmap selection       |
| Polyphony test             | Multi-channel independent outputs                         |
| Cache invalidation test    | Mtime change triggers re-preparation                      |
| DSL integration test       | $wavetable produces valid PatchGraph JSON                 |
