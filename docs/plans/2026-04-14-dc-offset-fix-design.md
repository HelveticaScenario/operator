# Design: DC Offset Fix with Dynamic Range Annotations

Date: 2026-04-14

## Problem

The `$pulse` oscillator produces DC offset at non-50% duty cycles. The DC component
is `(2w - 1) * 5V` where `w` is the normalized pulse width (0..1). At 25% width this
is -2.5V; at 75% it's +2.5V. This DC propagates unfiltered through `$lpf`, `$mix`,
`$stereoMix`, `$scaleAndShift`, and into `ROOT_OUTPUT`, causing audible speaker
excursion and headroom loss during performances.

Secondary DC sources (`$cheby`, `$fold`, `$segment`) exist but are out of scope for
this change.

## Constraints

- No DC blocking filters at the output stage.
- No user-facing toggle for DC enable/disable.
- Intentional DC (CV to external devices) must still work.
- `.range()` must continue to produce correct remappings.
- No heap allocation on the audio thread.

## Approach

Three changes:

1. **Analytic DC subtraction in `$pulse`** — subtract the known DC component each sample.
2. **Per-channel dynamic range on `PolyOutput`** — make output range a core part of
   the signal system so any module can declare its actual output bounds at runtime,
   and `.range()` uses them automatically.
3. **`get_sample()` fast path** — avoid copying the now-larger `PolyOutput` on every
   cable read.

---

## Part 1: DC Subtraction in `$pulse`

**File:** `crates/modular_core/src/dsp/oscillators/pulse.rs`

After computing `naive_pulse` (with PolyBLEP), subtract the DC component:

```
dc_offset = (2.0 * pulse_width - 1.0) * 5.0
output = naive_pulse * 5.0 - dc_offset
```

This is the same technique used by Surge and Befaco EvenVCO. Zero cost (one multiply,
one subtract). Works correctly at LFO rates — no filter lag.

**After DC subtraction, the output bounds become width-dependent:**

| Width (w) | DC offset | Output min | Output max |
| --------- | --------- | ---------- | ---------- |
| 0.50      | 0.0V      | -5.0V      | +5.0V      |
| 0.25      | -2.5V     | -2.5V      | +7.5V      |
| 0.75      | +2.5V     | -7.5V      | +2.5V      |
| 0.01      | -4.9V     | -0.1V      | +9.9V      |

General formula per channel:

- `min = -10 * w`
- `max = 10 * (1 - w)`

The peak-to-peak amplitude remains 10V at all widths.

---

## Part 2: Per-Channel Dynamic Range on `PolyOutput`

### The Problem with Static Ranges

The current `range = (-5.0, 5.0)` annotation on `$pulse`'s output is baked into
`OutputSchema` at compile time. The `.range()` DSL method reads `minValue`/`maxValue`
from this schema and passes them as literal numbers to `$remap`'s `inMin`/`inMax`
params. After DC subtraction, these static values are wrong — the actual bounds
depend on the runtime pulse width, which can vary per polyphony channel.

### Solution: Per-Channel Range Arrays on `PolyOutput`

Add two `[f32; 16]` arrays to `PolyOutput` — `range_min` and `range_max`. Each
channel has its own range bounds, updated each sample by any module that knows its
range. This makes dynamic range a core property of the signal system.

**File:** `crates/modular_core/src/poly.rs`

```rust
#[derive(Clone, Copy, Debug)]
pub struct PolyOutput {
    voltages: [f32; PORT_MAX_CHANNELS],
    channels: usize,
    range_min: [f32; PORT_MAX_CHANNELS],   // NaN = unknown
    range_max: [f32; PORT_MAX_CHANNELS],   // NaN = unknown
}
```

Cost: +128 bytes per `PolyOutput` (68 → 196 bytes). `PolyOutput` remains `Copy`.
All range values default to `NaN` (unknown).

**API additions on `PolyOutput`:**

```rust
/// Set range for a specific channel
pub fn set_range(&mut self, channel: usize, min: f32, max: f32);

/// Get range for a specific channel, None if unknown
pub fn channel_range(&self, channel: usize) -> Option<(f32, f32)>;

/// Check if any channel has range metadata
pub fn has_range(&self) -> bool;

/// Raw access for virtual port generation
pub fn range_min_value(&self, channel: usize) -> f32;
pub fn range_max_value(&self, channel: usize) -> f32;
```

**Usage in `$pulse`:**

```rust
// In update(), per channel:
let dc = (2.0 * pulse_width - 1.0) * 5.0;
self.outputs.sample.set(ch, naive_pulse * 5.0 - dc);
self.outputs.sample.set_range(ch, -10.0 * pulse_width, 10.0 * (1.0 - pulse_width));
```

Per-channel range means polyphonic `$pulse` with different widths per voice gets
exact bounds per voice. `.range()` remaps each channel precisely.

### Virtual Range Ports

Every `PolyOutput` field with `dynamic_range` annotation automatically exposes two
virtual output ports via the derive macro. No explicit companion fields needed.

The generated `get_poly_sample()` gains two synthetic port names per output:

```rust
// Auto-generated for output named "output":
"output" => Some(self.sample),
"output.rangeMin" => {
    let mut po = PolyOutput::default();
    po.set_channels(self.sample.channels());
    for ch in 0..self.sample.channels() {
        po.set(ch, self.sample.range_min_value(ch));
    }
    Some(po)
},
"output.rangeMax" => { /* same pattern with range_max */ },
```

These virtual ports:

- Are NOT included in `schemas()` — invisible to the user
- Are only used internally by `.range()` wiring
- Are only called when `.range()` is used on a dynamic-range output

### How `.range()` Uses Dynamic Range

In TypeScript, when `dynamicRange: true`:

```typescript
range(outMin, outMax) {
    if (this.dynamicRange) {
        const rangeMin = new ModuleOutput(
            this.builder, this.moduleId,
            `${this.portName}.rangeMin`, this.channel
        );
        const rangeMax = new ModuleOutput(
            this.builder, this.moduleId,
            `${this.portName}.rangeMax`, this.channel
        );
        return $remap(this, outMin, outMax, rangeMin, rangeMax);
    }
    return $remap(this, outMin, outMax, this.minValue, this.maxValue);
}
```

`$remap` needs no changes — it receives range values as normal cable inputs to
`inMin`/`inMax`, with `Clickless` smoothing for free.

### Which Modules Set Range

**This PR:** `$pulse` only.

**Future (same pattern, trivial to add):**

- `$remap` — always knows its range: `(outMin, outMax)`
- `$scaleAndShift` — computable from input range + scale/shift params
- `$cheby`, `$fold`, `$segment` — if/when DC subtraction is added

Modules without `set_range()` calls → NaN → no `dynamicRange` in schema → static
path → zero behavior change.

---

## Part 3: `get_sample()` Fast Path

### The Problem

`Signal::Cable::get_value()` is called on every cable connection on every sample.
Today it copies a full `PolyOutput` (72 bytes) just to extract one `f32`. With
per-channel range arrays, `PolyOutput` grows to 196 bytes, making this worse.

### Solution: `get_sample(port, channel) -> f32`

Add a new method to `Sampleable` and `OutputStruct` that reads a single channel
value directly, avoiding the full `PolyOutput` copy.

**File:** `crates/modular_core/src/types.rs` — `Sampleable` trait:

```rust
fn get_sample(&self, port: &str, channel: usize) -> Result<f32>;
```

**File:** `crates/modular_core/src/types.rs` — `OutputStruct` trait:

```rust
fn get_sample(&self, port: &str, channel: usize) -> Option<f32>;
```

**Generated implementation** (derive macro):

```rust
fn get_sample(&self, port: &str, channel: usize) -> Option<f32> {
    match port {
        "output" => Some(self.sample.get_cycling(channel)),
        // f32 fields:
        "beatTrigger" => Some(self.beat_trigger),
        _ => None,
    }
}
```

**Sampleable wrapper** (module_attr.rs):

```rust
fn get_sample(&self, port: &str, channel: usize) -> napi::Result<f32> {
    self.update();
    let outputs = unsafe { &*self.outputs.get() };
    OutputStruct::get_sample(outputs, port, channel)
        .ok_or_else(|| napi::Error::from_reason(...))
}
```

**Signal::Cable::get_value()** updated:

```rust
Signal::Cable { module_ptr, port, channel, .. } => match module_ptr.upgrade() {
    Some(module_ptr) => module_ptr
        .get_sample(port, *channel)        // fast path: no PolyOutput copy
        .unwrap_or(0.0),
    None => 0.0,
},
```

`get_poly_sample` remains for callers that need the full `PolyOutput` (ROOT_OUTPUT
read, scope capture, virtual range ports). These are ~10 calls per frame, not
thousands.

---

## Part 4: Derive Macro Changes

**File:** `crates/modular_derive/src/outputs.rs`

### New `dynamic_range` annotation

```rust
#[output("output", "signal output", default, range = (-5.0, 5.0), dynamic_range)]
sample: PolyOutput,
```

When `dynamic_range` is present on a `PolyOutput` field:

- `OutputSchema` emits `dynamic_range: true`
- Static `range = (...)` serves as fallback default
- Two virtual port match arms are auto-generated in `get_poly_sample()`

### New `get_sample()` generation

For every output field, generate a `get_sample` match arm that returns a single
`f32` value without constructing a full `PolyOutput`.

---

## Part 5: `OutputSchema` + TypeScript

**File:** `crates/modular_core/src/types.rs`

Add to `OutputSchema`:

```rust
#[serde(default, skip_serializing_if = "std::ops::Not::not")]
pub dynamic_range: bool,
```

**File:** `src/main/dsl/GraphBuilder.ts`

- `OutputSchemaWithRange` gains `dynamicRange?: boolean`
- `ModuleOutputWithRange` gains `dynamicRange: boolean` constructor param
- `.range()` branches on `dynamicRange` (wires cables vs static numbers)
- `_output()` in `ModuleNode` passes `dynamicRange` from schema

**File:** `src/main/dsl/typescriptLibGen.ts`

Include `dynamicRange` in generated type definitions.

**File:** `src/main/dsl/paramsSchema.ts`

Pass through `dynamicRange` from Rust schema.

---

## Part 6: Reserved Output Names

No changes needed — `.rangeMin`/`.rangeMax` are virtual port names used internally,
not DSL methods on `ModuleOutput`/`Collection`.

---

## Scope

### In scope

- Per-channel range arrays on `PolyOutput` (+128 bytes)
- `get_sample()` fast path on Sampleable/OutputStruct
- DC subtraction for `$pulse`
- `dynamic_range` annotation + virtual range ports in derive macro
- `OutputSchema.dynamic_range` flag
- TypeScript `.range()` dynamic wiring
- Tests

### Out of scope

- DC fixes for `$cheby`, `$fold`, `$segment` (future — same pattern)
- Making `$remap`/`$scaleAndShift` set range on their outputs (future — trivial)
- Changing `get_poly_sample` to return by reference (future optimization)
- Any output-stage DC blocking
- User-facing controls

## Testing

1. **Existing DC offset tests** — the 3 failing tests should pass after the fix.
2. **New: `PolyOutput` range API** — unit tests for `set_range`, `channel_range()`,
   `has_range()`, NaN sentinel behavior, per-channel independence.
3. **New: `get_sample()` correctness** — verify `get_sample` returns same values
   as `get_poly_sample().get_cycling()` for all output types.
4. **New: virtual range ports** — verify `get_poly_sample("output.rangeMin")`
   returns correct per-channel values for `$pulse` at various widths.
5. **New: `.range()` with dynamic bounds** — end-to-end: `$pulse` → `.range(0, 1)`
   produces values in [0, 1] at non-50% widths.
6. **All 528 existing tests** must continue to pass.
