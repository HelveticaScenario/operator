# Wavetable Oscillator & Table Type Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `$wavetable` oscillator module and a `Table` param type for phase warping, enabling wavetable synthesis with signal-driven warp effects.

**Architecture:** Tables are a param type (enum) stored on the consuming module's params. Each variant holds PolySignal fields for dynamic parameters. The consumer resolves table signals in its own `connect()` and calls `table.evaluate(position, channel)` per-sample. The wavetable module loads WAV files via the existing `$wavs()` system, splits them into frames with metadata-based detection, generates FFT-based mipmaps for anti-aliasing, and reads frames with linear interpolation. Wavetable data is prepared on the main thread via a new `prepare_resources` trait method.

**Tech Stack:** Rust (modular_core DSP, modular N-API), TypeScript (DSL executor, type generation), rustfft crate, hound crate (existing)

**Spec:** `docs/superpowers/specs/2026-04-16-wavetable-oscillator-design.md`

---

### Task 1: Table Enum + Warp Functions

**Files:**

- Modify: `crates/modular_core/src/types.rs`
- Create: `crates/modular_core/src/dsp/tables/mod.rs`
- Create: `crates/modular_core/src/dsp/tables/warp.rs`
- Modify: `crates/modular_core/src/dsp/mod.rs`

The `Table` is an enum param type with variants per warp kind. Each variant holds PolySignal fields. The enum implements `evaluate()` (dispatches to warp functions) and `connect()` (resolves inner PolySignals). Warp functions are pure `fn(f32, f32) -> f32`.

- [ ] **Step 1: Create warp.rs with pure warp functions**

Create `crates/modular_core/src/dsp/tables/warp.rs`:

```rust
//! Pure warp functions for table phase distortion.
//! Each function: (x: f32, param: f32) -> f32
//! x ∈ [0,1], output ∈ [0,1], param range depends on the function.

/// Identity — pass through unchanged.
pub fn identity(x: f32, _param: f32) -> f32 {
    x
}

/// Mirror — reflects the waveform around the midpoint.
/// param=0: identity. param=1: full triangle (double-speed, mirrored).
pub fn mirror(x: f32, param: f32) -> f32 {
    let reflected = if x < 0.5 { x * 2.0 } else { 2.0 - x * 2.0 };
    let result = x + (reflected - x) * param.clamp(0.0, 1.0);
    result.clamp(0.0, 1.0)
}

/// Bend — asymmetric phase distortion (power curve).
/// param=0: identity. param=1: aggressive bend. param=-1: inverse bend.
pub fn bend(x: f32, param: f32) -> f32 {
    let param = param.clamp(-1.0, 1.0);
    if param.abs() < 1e-6 {
        return x;
    }
    let exponent = (2.0f32).powf(param * 2.0);
    x.powf(exponent).clamp(0.0, 1.0)
}

/// Sync — hard sync effect. Multiplies phase frequency.
/// param=0: 1x (identity). param=1: 16x frequency.
pub fn sync(x: f32, param: f32) -> f32 {
    let ratio = 1.0 + param.clamp(0.0, 1.0) * 15.0;
    (x * ratio).fract()
}

/// Fold — wave folding. Folds phase back at boundaries.
/// param=0: identity. param=1: 4x folding.
pub fn fold(x: f32, param: f32) -> f32 {
    let param = param.clamp(0.0, 1.0);
    let scaled = x * (1.0 + param * 3.0);
    let period = scaled % 2.0;
    if period <= 1.0 {
        period
    } else {
        2.0 - period
    }
}

/// PWM — pulse width modulation of phase.
/// param=0.5: identity-like. param→0: compress first half. param→1: compress second half.
pub fn pwm(x: f32, param: f32) -> f32 {
    let width = param.clamp(0.01, 0.99);
    if x < width {
        x / width * 0.5
    } else {
        0.5 + (x - width) / (1.0 - width) * 0.5
    }
}
```

Include tests for each function (identity at param=0, output range [0,1], endpoint preservation).

- [ ] **Step 2: Add Table enum to types.rs**

Add the `Table` enum after the existing `Wav`/`Buffer` types. It extends the pattern of `Vec<PolySignal>` as a connected container of signals — Table is a connected container of signals plus an evaluate function.

```rust
use crate::dsp::tables::warp;

/// A table is an immutable function object used as a param type.
/// Each variant holds PolySignal fields for dynamic parameters.
/// The consuming module resolves signals via connect() and calls evaluate() per-sample.
#[derive(Clone, Deserialize, Deserr, JsonSchema)]
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
    /// Evaluate the table warp at position x for the given channel.
    #[inline]
    pub fn evaluate(&self, x: f32, channel: usize) -> f32 {
        match self {
            Table::Identity => x,
            Table::Mirror { amount } => warp::mirror(x, amount.get_value(channel)),
            Table::Bend { amount } => warp::bend(x, amount.get_value(channel)),
            Table::Sync { ratio } => warp::sync(x, ratio.get_value(channel)),
            Table::Fold { amount } => warp::fold(x, amount.get_value(channel)),
            Table::Pwm { width } => warp::pwm(x, width.get_value(channel)),
        }
    }
}

impl Connect for Table {
    fn connect(&mut self, patch: &Patch) {
        match self {
            Table::Identity => {}
            Table::Mirror { amount } => amount.connect(patch),
            Table::Bend { amount } => amount.connect(patch),
            Table::Sync { ratio } => ratio.connect(patch),
            Table::Fold { amount } => amount.connect(patch),
            Table::Pwm { width } => width.connect(patch),
        }
    }
}
```

Note: `PolySignal` already implements `Connect`, `Deserialize`, `Deserr`, and `JsonSchema`. The `Table` derives should work via the existing infrastructure.

Serialization: add `Serialize` impl that mirrors the deserialization format (tagged enum with signal refs).

- [ ] **Step 3: Create tables/mod.rs**

Create `crates/modular_core/src/dsp/tables/mod.rs`:

```rust
pub mod warp;
```

Register in `crates/modular_core/src/dsp/mod.rs`:

```rust
pub mod tables;
```

- [ ] **Step 4: Add unit tests for Table enum**

Test each variant: evaluate with constant signal, evaluate with varying channel values, connect resolves properly.

- [ ] **Step 5: Run tests**

Run: `cargo test -p modular_core`
Expected: all existing tests PASS, new table/warp tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/modular_core/src/types.rs crates/modular_core/src/dsp/tables/ crates/modular_core/src/dsp/mod.rs
git commit -m "feat: add Table param enum with warp functions for phase distortion"
```

---

### Task 2: Frame Detection in WavCache

**Files:**

- Modify: `crates/modular/src/lib.rs` (WavCache::load)
- Modify: `crates/modular_core/src/types.rs` (WavData — add detected_frame_size)

Parse raw WAV chunks (CLM, uhWT, srge) before hound decodes audio to detect wavetable frame boundaries.

- [ ] **Step 1: Add `detected_frame_size` to WavData**

In `crates/modular_core/src/types.rs`, add `detected_frame_size: Option<usize>` to `WavData`. Update `WavData::new` to accept it. Fix all existing callers to pass `None`.

- [ ] **Step 2: Add WAV chunk parser**

In `crates/modular/src/lib.rs`, add `detect_wavetable_frame_size(file_path: &Path) -> Option<usize>` that reads raw RIFF chunks:

- CLM chunk (Serum format) → parse ASCII frame size after `<!>` prefix (e.g. `<!>2048 10000000 wavetable Name`)
- uhWT chunk (Hive format) → assume 2048-sample frames (payload format undocumented)
- srge chunk (Surge format) → 8-byte payload: int32 version (must be 1) + int32 frame_size
- No match → return None (caller uses 2048 default)

- [ ] **Step 3: Call frame detection in WavCache::load**

Call `detect_wavetable_frame_size(&full_path)` and pass result to `WavData::new`.

- [ ] **Step 4: Write tests for frame detection**

Generate minimal WAV files with CLM/uhWT/srge chunks and verify detection.

Run: `cargo test -p modular detect_wavetable`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/modular_core/src/types.rs crates/modular/src/lib.rs
git commit -m "feat: detect wavetable frame size from WAV metadata (CLM, uhWT, srge)"
```

---

### Task 3: Mtime in wav_ref

**Files:**

- Modify: `crates/modular/src/lib.rs` (WavLoadInfo, WavCache::load)
- Modify: `crates/modular_core/src/types.rs` (Wav struct)
- Modify: `src/main/dsl/executor.ts`
- Modify: `src/main/dsl/typescriptLibGen.ts`

Add `mtime` to the wav_ref object so the params cache invalidates when a wav file changes.

- [ ] **Step 1: Add `mtime` to WavLoadInfo**

In `crates/modular/src/lib.rs`, add `pub mtime: f64` (epoch milliseconds) to `WavLoadInfo`. Populate from file metadata in `WavCache::load`.

- [ ] **Step 2: Add `mtime` to Wav type**

In `crates/modular_core/src/types.rs`, add `mtime: Option<f64>` to the `Wav` struct. Update `WavSerde`, `Serialize`/`Deserialize`/`Deserr` impls.

- [ ] **Step 3: Add `mtime` to TypeScript wav_ref**

In `src/main/dsl/executor.ts`, include `mtime: info.mtime` in the wav_ref object returned by `$wavs()`. Update `DSLExecutionOptions` loadWav return type.

- [ ] **Step 4: Update WavHandle type in typescriptLibGen.ts**

Add `readonly mtime: number;` to `WavHandle`.

- [ ] **Step 5: Run tests**

Run: `yarn test:unit`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/modular/src/lib.rs crates/modular_core/src/types.rs src/main/dsl/executor.ts src/main/dsl/typescriptLibGen.ts
git commit -m "feat: add mtime to wav_ref for params cache invalidation"
```

---

### Task 4: Mipmap Generation

**Files:**

- Create: `crates/modular_core/src/dsp/oscillators/wavetable_prep.rs`
- Modify: `crates/modular_core/Cargo.toml`

FFT-based mipmap generation for anti-aliased wavetable playback. Each mipmap level has progressively fewer harmonics.

- [ ] **Step 1: Add `rustfft` dependency**

In `crates/modular_core/Cargo.toml`, add `rustfft = "6.2"`.

- [ ] **Step 2: Create PreparedWavetable and mipmap generation**

Create `crates/modular_core/src/dsp/oscillators/wavetable_prep.rs` with:

- `PreparedWavetable` struct: `levels: Vec<Vec<f32>>`, `frame_size`, `frame_count`, `mipmap_count`, `base_frequency`
- `from_wav_data(wav_data: &WavData, sample_rate: f32) -> Self` — splits wav into frames, generates FFT-based mipmaps
- `read_sample(level, frame, phase) -> f32` — linear interpolation within frame + crossfade between frames
- `mipmap_level_for_freq(freq) -> usize` — select appropriate level

Mipmap generation: for each frame, FFT → for each octave level, zero bins above Nyquist → IFFT → store. ~log2(frame_size) levels.

- [ ] **Step 3: Run tests**

Run: `cargo test -p modular_core wavetable_prep`
Expected: PASS (single-frame sine, multi-frame, mipmap selection, frame crossfade tests)

- [ ] **Step 4: Commit**

```bash
git add crates/modular_core/src/dsp/oscillators/wavetable_prep.rs crates/modular_core/Cargo.toml
git commit -m "feat: add PreparedWavetable with FFT-based mipmap generation"
```

---

### Task 5: prepare_resources + WavetableOsc Module

**Files:**

- Modify: `crates/modular_core/src/types.rs` (Sampleable trait — add prepare_resources)
- Modify: `crates/modular_derive/src/module_attr.rs` (add has_prepare_resources flag)
- Modify: `crates/modular/src/audio.rs` (call prepare_resources in apply_patch)
- Create: `crates/modular_core/src/dsp/oscillators/wavetable.rs`
- Modify: `crates/modular_core/src/dsp/oscillators/mod.rs`

- [ ] **Step 1: Add `prepare_resources` to Sampleable trait**

In `crates/modular_core/src/types.rs`, add default no-op:

```rust
fn prepare_resources(&self, _wav_data: &std::collections::HashMap<String, Arc<WavData>>) {}
```

- [ ] **Step 2: Add `has_prepare_resources` to #[module] macro**

In `crates/modular_derive/src/module_attr.rs`, parse the `has_prepare_resources` flag and generate delegation:

```rust
fn prepare_resources(&self, wav_data: &HashMap<String, Arc<WavData>>) {
    let module = unsafe { &mut *self.module.get() };
    module.prepare_resources_impl(wav_data, self.sample_rate);
}
```

- [ ] **Step 3: Call prepare_resources in apply_patch**

In `crates/modular/src/audio.rs`, after constructing all modules but before queueing PatchUpdate:

```rust
for module in update.inserts.values() {
    module.prepare_resources(&wav_data);
}
```

- [ ] **Step 4: Create wavetable.rs**

Create `crates/modular_core/src/dsp/oscillators/wavetable.rs`:

```rust
#[module(
    name = "$wavetable",
    channels_derive = wavetable_derive_channel_count,
    args(wav, pitch, position),
    has_prepare_resources
)]
pub struct WavetableOsc {
    params: WavetableParams,
    outputs: WavetableOutputs,
    state: WavetableState,
}
```

Params:

- `wav: Wav` — wav file reference
- `pitch: MonoSignal` — V/Oct pitch (signal type = pitch)
- `position: MonoSignal` — frame position 0–5V (signal type = generic)
- `phase: Option<Table>` — phase warp table (the new Table enum)
- `prepared: Option<PreparedWavetable>` — `#[serde(skip)]`, filled by prepare_resources

State:

- `phases: [f64; MAX_CHANNELS]` — per-channel phase accumulators, preserved via transfer_state_from

Processing per sample per channel:

1. Read pitch → V/Oct to frequency (0V = C4 = 261.63 Hz)
2. Read position → scale 0–5V to 0.0–1.0 → frame float index
3. Phase increment: freq / sample_rate
4. Warp phase: `phase.evaluate(raw_phase, ch)` if Some, else raw_phase
5. Mipmap level: `prepared.mipmap_level_for_freq(freq)`
6. `prepared.read_sample(level, frame, warped_phase)`
7. Advance phase, wrap at 1.0

Channel count: `max(pitch.channels, position.channels)`

The consumer's connect() must also connect the Table's inner PolySignals. In the params Connect derive, `Option<Table>` should auto-connect if it implements Connect (which it does from Task 1). Verify the derive handles `Option<T: Connect>`.

- [ ] **Step 5: Register in oscillators/mod.rs**

Add `pub mod wavetable;` and `pub mod wavetable_prep;`. Register constructor, params deserializer, and schema.

- [ ] **Step 6: Run tests**

Run: `cargo test -p modular_core`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/modular_core/src/types.rs crates/modular_core/src/dsp/oscillators/wavetable.rs crates/modular_core/src/dsp/oscillators/wavetable_prep.rs crates/modular_core/src/dsp/oscillators/mod.rs crates/modular/src/audio.rs crates/modular_derive/src/module_attr.rs
git commit -m "feat: add $wavetable oscillator with mipmap-based anti-aliasing"
```

---

### Task 6: $table DSL Helpers + TypeScript Types

**Files:**

- Modify: `src/main/dsl/executor.ts`
- Modify: `src/main/dsl/typescriptLibGen.ts`

`$table.*` functions are DSL helpers (like `$hz`, `$note`) that return JSON objects. They are NOT graph-node factories — they produce inline param values.

- [ ] **Step 1: Add $table namespace to dslGlobals**

In `src/main/dsl/executor.ts`, add a `$table` object to `dslGlobals`:

```typescript
const $table = {
    mirror: (amount: any) => ({
        type: 'mirror',
        amount: serializeSignalParam(amount),
    }),
    bend: (amount: any) => ({
        type: 'bend',
        amount: serializeSignalParam(amount),
    }),
    sync: (ratio: any) => ({
        type: 'sync',
        ratio: serializeSignalParam(ratio),
    }),
    fold: (amount: any) => ({
        type: 'fold',
        amount: serializeSignalParam(amount),
    }),
    pwm: (width: any) => ({ type: 'pwm', width: serializeSignalParam(width) }),
};
```

Note: the Rust `Table` enum deserializes with camelCase tags (`"mirror"`, `"bend"`, etc.), matching codebase convention (`Signal::Cable` → `"cable"`, etc.). Identity is `{type: "identity"}` if ever emitted by the DSL.

The `serializeSignalParam` helper needs to handle both literal numbers and signal references (ModuleOutput objects). Check how signal params are currently serialized when passed to module factories — use the same mechanism.

- [ ] **Step 2: Add TypeScript types**

In `src/main/dsl/typescriptLibGen.ts`, add:

```typescript
type Table = { readonly type: string };

declare const $table: {
    mirror(amount: Signal): Table;
    bend(amount: Signal): Table;
    sync(ratio: Signal): Table;
    fold(amount: Signal): Table;
    pwm(width: Signal): Table;
};
```

Add `Table` as an accepted type for the `phase` config param on `$wavetable`.

- [ ] **Step 3: Run tests**

Run: `yarn test:unit`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/main/dsl/executor.ts src/main/dsl/typescriptLibGen.ts
git commit -m "feat: add $table DSL helpers and TypeScript types"
```

---

### Task 7: Build and Smoke Test

**Files:** None new — integration verification.

- [ ] **Step 1: Rebuild native module**

Run: `yarn build-native`
Expected: compiles without errors

- [ ] **Step 2: Regenerate types**

Run: `yarn generate-lib`
Expected: generates updated `crates/modular/index.d.ts`

- [ ] **Step 3: TypeScript typecheck**

Run: `yarn typecheck`
Expected: no type errors

- [ ] **Step 4: Run all unit tests**

Run: `yarn test:unit`
Expected: PASS

- [ ] **Step 5: Run Rust tests**

Run: `cargo test -p modular_core && cargo test -p modular`
Expected: PASS

- [ ] **Step 6: Commit any generated file updates**

```bash
git add -A
git commit -m "chore: regenerate types and schemas after wavetable + table additions"
```
