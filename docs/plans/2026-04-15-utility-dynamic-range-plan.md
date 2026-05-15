# Utility Module Dynamic Range Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `dynamic_range` to 5 utility modules (`$remap`, `$scaleAndShift`, `$clamp`, `$wrap`, `$spread`) so `.range()` produces correct remappings downstream. Also add `Signal::get_range()` / `PolySignal::get_range()` so modules can read their input's range without extra params.

**Architecture:** Two groups of modules — Group A (self-contained: `$remap`, `$wrap`, `$spread`) where output range is fully determined by own params, and Group B (composed: `$scaleAndShift`, `$clamp`) where output range depends on input range via the new `get_range()` API. A dedicated `get_range(port, channel) -> Option<(f32, f32)>` method is added to the `OutputStruct` and `Sampleable` traits — the derive macro generates it from output annotations. This avoids any string concatenation or heap allocation: `Signal::get_range()` calls `module.get_range(port, channel)` directly. The virtual `.rangeMin`/`.rangeMax` string ports still exist for the TypeScript DSL (which constructs cables to them), but Rust-side range reads bypass them entirely. The derive macro is also updated to generate virtual range ports for ALL outputs with a static `range = (...)` attribute (not just `dynamic_range`), so the TypeScript DSL can read static ranges too.

**Tech Stack:** Rust (proc macros, DSP modules), TypeScript (no changes expected — DSL already handles `dynamicRange`)

---

### Task 1: Derive macro — generate virtual range ports for static-range outputs

The derive macro currently only generates `output.rangeMin` / `output.rangeMax` match arms for outputs marked `dynamic_range`. We need it to also generate them for outputs that have `range = (...)` but NOT `dynamic_range` — returning the static constants. This enables `get_range()` to read static ranges uniformly via virtual ports.

**Files:**

- Modify: `crates/modular_derive/src/outputs.rs:358-410`
- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Write a failing test**

Add to `crates/modular_core/tests/dsp_fresh_tests.rs`:

```rust
#[test]
fn static_range_output_exposes_virtual_range_ports() {
    // $sine has range = (-5.0, 5.0) but no dynamic_range
    // After this change, it should still expose output.rangeMin / output.rangeMax
    // returning the static values
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "osc".into(),
        serde_json::from_value(json!({
            "type": "$sine",
            "params": { "freq": 0.0 }
        }))
        .unwrap(),
    );
    patch.init();

    let osc = patch.sampleables.get("osc").unwrap();
    osc.get_sample("output", 0); // trigger update

    let range_min = osc.get_sample("output.rangeMin", 0);
    let range_max = osc.get_sample("output.rangeMax", 0);

    assert!(
        range_min.is_some(),
        "static-range output should expose output.rangeMin virtual port"
    );
    assert!(
        range_max.is_some(),
        "static-range output should expose output.rangeMax virtual port"
    );
    assert!(
        (range_min.unwrap() - (-5.0)).abs() < 0.01,
        "rangeMin should be -5.0, got {}",
        range_min.unwrap()
    );
    assert!(
        (range_max.unwrap() - 5.0).abs() < 0.01,
        "rangeMax should be 5.0, got {}",
        range_max.unwrap()
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p modular_core static_range_output_exposes -- --nocapture`
Expected: FAIL — `range_min.is_some()` assertion fails because `$sine` doesn't have `dynamic_range`

- [ ] **Step 3: Update the derive macro to generate static range port arms**

In `crates/modular_derive/src/outputs.rs`, add a new set of match arms for outputs that have `range` but NOT `dynamic_range`. These return the static constant values.

After the existing `virtual_range_arms` (line ~359), add:

```rust
// Generate static range port match arms for outputs with range but no dynamic_range
let static_range_arms: Vec<_> = outputs
    .iter()
    .filter(|o| o.range.is_some() && !o.dynamic_range && o.precision == OutputPrecision::PolySignal)
    .flat_map(|o| {
        let output_name_str = o.output_name.value();
        let range_min_name = format!("{}.rangeMin", output_name_str);
        let range_max_name = format!("{}.rangeMax", output_name_str);
        let (min_val, max_val) = o.range.unwrap();
        let min_f32 = min_val as f32;
        let max_f32 = max_val as f32;
        vec![
            quote! {
                #range_min_name => {
                    let mut po = crate::poly::PolyOutput::default();
                    po.set_channels(self.#field_name.channels());
                    for ch in 0..self.#field_name.channels() {
                        po.set(ch, #min_f32);
                    }
                    Some(po)
                },
            },
            quote! {
                #range_max_name => {
                    let mut po = crate::poly::PolyOutput::default();
                    po.set_channels(self.#field_name.channels());
                    for ch in 0..self.#field_name.channels() {
                        po.set(ch, #max_f32);
                    }
                    Some(po)
                },
            },
        ]
    })
    .collect();

let static_range_sample_arms: Vec<_> = outputs
    .iter()
    .filter(|o| o.range.is_some() && !o.dynamic_range && o.precision == OutputPrecision::PolySignal)
    .flat_map(|o| {
        let output_name_str = o.output_name.value();
        let range_min_name = format!("{}.rangeMin", output_name_str);
        let range_max_name = format!("{}.rangeMax", output_name_str);
        let (min_val, max_val) = o.range.unwrap();
        let min_f32 = min_val as f32;
        let max_f32 = max_val as f32;
        vec![
            quote! {
                #range_min_name => Some(#min_f32),
            },
            quote! {
                #range_max_name => Some(#max_f32),
            },
        ]
    })
    .collect();
```

Then add `#(#static_range_arms)*` and `#(#static_range_sample_arms)*` to the match blocks in `get_poly_sample` and `get_sample` respectively, alongside the existing virtual range arms.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p modular_core static_range_output_exposes -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run full Rust test suite**

Run: `cargo test -p modular_core`
Expected: All tests pass (existing dynamic_range tests unaffected)

- [ ] **Step 6: Commit**

```bash
git add crates/modular_derive/src/outputs.rs crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "feat: derive macro generates virtual range ports for static-range outputs"
```

---

### Task 2: Add `get_range()` to `OutputStruct`, `Sampleable`, `Signal`, and `PolySignal`

Add a dedicated `get_range(port, channel) -> Option<(f32, f32)>` method to the trait hierarchy so modules can read their input's range without any string concatenation or heap allocation. The derive macro generates the `OutputStruct::get_range()` implementation from the same `range`/`dynamic_range` annotations. `Signal::get_range()` calls `module.get_range(port, channel)` directly.

**Files:**

- Modify: `crates/modular_core/src/types.rs` — add `get_range` to `OutputStruct` trait and `Sampleable` trait, add `Signal::get_range()`
- Modify: `crates/modular_core/src/poly.rs` — add `PolySignal::get_range()`
- Modify: `crates/modular_derive/src/outputs.rs` — generate `get_range()` impl
- Modify: `crates/modular_derive/src/module_attr.rs` — wire `get_range()` through Sampleable impl
- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Write failing tests**

Add to `crates/modular_core/tests/dsp_fresh_tests.rs`:

```rust
#[test]
fn signal_get_range_returns_none_for_volts() {
    let signal = Signal::Volts(3.0);
    assert!(signal.get_range().is_none(), "Volts should return None for get_range()");
}

#[test]
fn signal_get_range_reads_dynamic_range_from_cable() {
    // Create a pulse oscillator at 25% width, which has dynamic_range
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "pulse".into(),
        serde_json::from_value(json!({
            "type": "$pulse",
            "params": { "freq": 0.0, "width": 1.25 }
        }))
        .unwrap(),
    );
    patch.init();

    // Trigger an update so range is computed
    let pulse = patch.sampleables.get("pulse").unwrap();
    pulse.get_sample("output", 0);

    // Create a cable signal pointing to the pulse output
    let signal = Signal::Cable {
        module: "pulse".into(),
        module_ptr: std::sync::Arc::downgrade(pulse),
        port: "output".into(),
        channel: 0,
    };

    let range = signal.get_range();
    assert!(range.is_some(), "Cable to dynamic_range output should return Some");
    let (min, max) = range.unwrap();
    // At 25% width (1.25V / 5V = 0.25): min = -10*0.25 = -2.5, max = 10*0.75 = 7.5
    assert!((min - (-2.5)).abs() < 0.1, "expected min ~-2.5, got {min}");
    assert!((max - 7.5).abs() < 0.1, "expected max ~7.5, got {max}");
}

#[test]
fn signal_get_range_reads_static_range_from_cable() {
    // $sine has static range (-5, 5) but no dynamic_range
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "osc".into(),
        serde_json::from_value(json!({
            "type": "$sine",
            "params": { "freq": 0.0 }
        }))
        .unwrap(),
    );
    patch.init();

    let osc = patch.sampleables.get("osc").unwrap();
    osc.get_sample("output", 0);

    let signal = Signal::Cable {
        module: "osc".into(),
        module_ptr: std::sync::Arc::downgrade(osc),
        port: "output".into(),
        channel: 0,
    };

    let range = signal.get_range();
    assert!(range.is_some(), "Cable to static-range output should return Some");
    let (min, max) = range.unwrap();
    assert!((min - (-5.0)).abs() < 0.01, "expected min -5.0, got {min}");
    assert!((max - 5.0).abs() < 0.01, "expected max 5.0, got {max}");
}

#[test]
fn poly_signal_get_range_per_channel() {
    // Create a pulse oscillator at 25% width
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "pulse".into(),
        serde_json::from_value(json!({
            "type": "$pulse",
            "params": { "freq": 0.0, "width": 1.25 }
        }))
        .unwrap(),
    );
    patch.init();

    let pulse = patch.sampleables.get("pulse").unwrap();
    pulse.get_sample("output", 0);

    let poly = PolySignal::mono(Signal::Cable {
        module: "pulse".into(),
        module_ptr: std::sync::Arc::downgrade(pulse),
        port: "output".into(),
        channel: 0,
    });

    let range = poly.get_range(0);
    assert!(range.is_some(), "PolySignal cable should return range");
    let (min, max) = range.unwrap();
    assert!((min - (-2.5)).abs() < 0.1, "expected min ~-2.5, got {min}");
    assert!((max - 7.5).abs() < 0.1, "expected max ~7.5, got {max}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p modular_core signal_get_range -- --nocapture`
Expected: FAIL — `get_range()` method doesn't exist yet

- [ ] **Step 3: Add `get_range` to `OutputStruct` trait**

In `crates/modular_core/src/types.rs`, add to the `OutputStruct` trait (after `get_sample`):

```rust
/// Get the range (min, max) for a specific port and channel.
/// Returns None if the port has no range information.
/// For dynamic_range outputs, reads the live per-channel range.
/// For static range outputs, returns the declared constants.
/// For outputs with no range, returns None.
fn get_range(&self, port: &str, channel: usize) -> Option<(f32, f32)> {
    None
}
```

- [ ] **Step 4: Add `get_range` to `Sampleable` trait**

In `crates/modular_core/src/types.rs`, add to the `Sampleable` trait (after `get_sample`):

```rust
/// Get the output range for a specific port and channel.
/// Zero-allocation alternative to reading virtual .rangeMin/.rangeMax ports.
fn get_range(&self, port: &str, channel: usize) -> Option<(f32, f32)> {
    None
}
```

- [ ] **Step 5: Generate `get_range` in derive macro**

In `crates/modular_derive/src/outputs.rs`, generate `get_range` match arms. Add after the `virtual_range_sample_arms` block (~line 410):

```rust
// Generate get_range match arms — returns Option<(f32, f32)> directly
let get_range_arms: Vec<_> = outputs
    .iter()
    .filter(|o| o.precision == OutputPrecision::PolySignal)
    .filter_map(|o| {
        let field_name = &o.field_name;
        let output_name = &o.output_name;

        if o.dynamic_range {
            // Dynamic range: read live per-channel values from PolyOutput
            Some(quote! {
                #output_name => Some((
                    self.#field_name.range_min_value(channel),
                    self.#field_name.range_max_value(channel),
                )),
            })
        } else if let Some((min_val, max_val)) = o.range {
            // Static range: return declared constants
            let min_f32 = min_val as f32;
            let max_f32 = max_val as f32;
            Some(quote! {
                #output_name => Some((#min_f32, #max_f32)),
            })
        } else {
            None // No range info at all
        }
    })
    .collect();
```

Then add the method to the `impl OutputStruct for #name` block:

```rust
fn get_range(&self, port: &str, channel: usize) -> Option<(f32, f32)> {
    match port {
        #(#get_range_arms)*
        _ => None,
    }
}
```

- [ ] **Step 6: Wire `get_range` through `Sampleable` in module_attr macro**

In `crates/modular_derive/src/module_attr.rs`, add the `get_range` method to the generated `Sampleable` impl (after `get_sample`, ~line 678):

```rust
fn get_range(&self, port: &str, channel: usize) -> Option<(f32, f32)> {
    self.update();
    let outputs = unsafe { &*self.outputs.get() };
    crate::types::OutputStruct::get_range(outputs, port, channel)
}
```

- [ ] **Step 7: Implement `Signal::get_range()`**

In `crates/modular_core/src/types.rs`, add to the `impl Signal` block (after `get_value`):

```rust
/// Get the range of this signal, if available.
/// - Volts: returns None (a constant has no meaningful range for remapping)
/// - Cable: calls get_range() on the connected module — zero allocation
pub fn get_range(&self) -> Option<(f32, f32)> {
    match self {
        Signal::Volts(_) => None,
        Signal::Cable {
            module_ptr,
            port,
            channel,
            ..
        } => {
            let module = module_ptr.upgrade()?;
            module.get_range(port, *channel)
        }
    }
}
```

- [ ] **Step 8: Implement `PolySignal::get_range()`**

In `crates/modular_core/src/poly.rs`, add to the `impl PolySignal` block:

```rust
/// Get the range of a specific channel's signal, if available.
/// Delegates to Signal::get_range() with channel cycling.
pub fn get_range(&self, channel: usize) -> Option<(f32, f32)> {
    self.channels[channel % self.channels.len()].get_range()
}
```

- [ ] **Step 9: Run tests to verify they pass**

Run: `cargo test -p modular_core signal_get_range poly_signal_get_range -- --nocapture`
Expected: PASS

- [ ] **Step 10: Run full test suite**

Run: `cargo test -p modular_core`
Expected: All pass

- [ ] **Step 11: Commit**

```bash
git add crates/modular_core/src/types.rs crates/modular_core/src/poly.rs crates/modular_derive/src/outputs.rs crates/modular_derive/src/module_attr.rs crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "feat: add get_range() to OutputStruct/Sampleable/Signal/PolySignal — zero-allocation range reads"
```

---

### Task 3: Add `dynamic_range` to `$remap`

`$remap` output range is `[outMin, outMax]`. Use the smoothed values from `ChannelState`.

**Files:**

- Modify: `crates/modular_core/src/dsp/utilities/remap.rs:41-46,68-93`
- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Write a failing test**

Add to `crates/modular_core/tests/dsp_fresh_tests.rs`:

```rust
#[test]
fn remap_has_dynamic_range() {
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "remap".into(),
        serde_json::from_value(json!({
            "type": "$remap",
            "params": { "input": 0.0, "outMin": -3.0, "outMax": 7.0 }
        }))
        .unwrap(),
    );
    patch.init();

    let remap = patch.sampleables.get("remap").unwrap();
    // Run enough samples for Clickless smoothing to converge
    for _ in 0..1000 {
        remap.get_sample("output", 0);
    }

    let range_min = remap.get_sample("output.rangeMin", 0);
    let range_max = remap.get_sample("output.rangeMax", 0);

    assert!(range_min.is_some(), "remap should expose rangeMin");
    assert!(range_max.is_some(), "remap should expose rangeMax");
    assert!(
        (range_min.unwrap() - (-3.0)).abs() < 0.1,
        "rangeMin should be ~-3.0, got {}",
        range_min.unwrap()
    );
    assert!(
        (range_max.unwrap() - 7.0).abs() < 0.1,
        "rangeMax should be ~7.0, got {}",
        range_max.unwrap()
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p modular_core remap_has_dynamic_range -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement dynamic_range on $remap**

In `crates/modular_core/src/dsp/utilities/remap.rs`:

Change the output annotation from:

```rust
#[output("output", "remapped signal output", default)]
```

to:

```rust
#[output("output", "remapped signal output", default, range = (-5.0, 5.0), dynamic_range)]
```

Add `set_range` call at the end of the per-channel loop (after `self.outputs.sample.set(i, output)`):

```rust
self.outputs.sample.set_range(i, *state.out_min, *state.out_max);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p modular_core remap_has_dynamic_range -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/modular_core/src/dsp/utilities/remap.rs crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "feat: add dynamic_range to \$remap module"
```

---

### Task 4: Add `dynamic_range` to `$wrap`

`$wrap` output range is `[min, max]` (the wrap bounds).

**Files:**

- Modify: `crates/modular_core/src/dsp/utilities/wrap.rs:23-27,46-65`
- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Write a failing test**

Add to `crates/modular_core/tests/dsp_fresh_tests.rs`:

```rust
#[test]
fn wrap_has_dynamic_range() {
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "wrap".into(),
        serde_json::from_value(json!({
            "type": "$wrap",
            "params": { "input": 0.0, "min": -2.0, "max": 3.0 }
        }))
        .unwrap(),
    );
    patch.init();

    let wrap = patch.sampleables.get("wrap").unwrap();
    wrap.get_sample("output", 0);

    let range_min = wrap.get_sample("output.rangeMin", 0);
    let range_max = wrap.get_sample("output.rangeMax", 0);

    assert!(range_min.is_some(), "wrap should expose rangeMin");
    assert!(range_max.is_some(), "wrap should expose rangeMax");
    assert!(
        (range_min.unwrap() - (-2.0)).abs() < 0.01,
        "rangeMin should be -2.0, got {}",
        range_min.unwrap()
    );
    assert!(
        (range_max.unwrap() - 3.0).abs() < 0.01,
        "rangeMax should be 3.0, got {}",
        range_max.unwrap()
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p modular_core wrap_has_dynamic_range -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement dynamic_range on $wrap**

In `crates/modular_core/src/dsp/utilities/wrap.rs`:

Change output annotation from:

```rust
#[output("output", "wrapped signal output", default)]
```

to:

```rust
#[output("output", "wrapped signal output", default, range = (0.0, 5.0), dynamic_range)]
```

Add `set_range` at the end of the per-channel loop (after `self.outputs.sample.set(i, output)`):

```rust
self.outputs.sample.set_range(i, min, max);
```

Note: `min` and `max` are already computed with swap handling in the loop body.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p modular_core wrap_has_dynamic_range -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/modular_core/src/dsp/utilities/wrap.rs crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "feat: add dynamic_range to \$wrap module"
```

---

### Task 5: Add `dynamic_range` to `$spread`

`$spread` output range is always `[min, max]` (the spread bounds), regardless of bias.

**Files:**

- Modify: `crates/modular_core/src/dsp/utilities/spread.rs:26-31,60-86`
- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Write a failing test**

Add to `crates/modular_core/tests/dsp_fresh_tests.rs`:

```rust
#[test]
fn spread_has_dynamic_range() {
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "spread".into(),
        serde_json::from_value(json!({
            "type": "$spread",
            "params": { "min": -3.0, "max": 7.0, "count": 4 }
        }))
        .unwrap(),
    );
    patch.init();

    let spread = patch.sampleables.get("spread").unwrap();
    spread.get_sample("output", 0);

    let range_min = spread.get_sample("output.rangeMin", 0);
    let range_max = spread.get_sample("output.rangeMax", 0);

    assert!(range_min.is_some(), "spread should expose rangeMin");
    assert!(range_max.is_some(), "spread should expose rangeMax");
    assert!(
        (range_min.unwrap() - (-3.0)).abs() < 0.01,
        "rangeMin should be -3.0, got {}",
        range_min.unwrap()
    );
    assert!(
        (range_max.unwrap() - 7.0).abs() < 0.01,
        "rangeMax should be 7.0, got {}",
        range_max.unwrap()
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p modular_core spread_has_dynamic_range -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement dynamic_range on $spread**

In `crates/modular_core/src/dsp/utilities/spread.rs`:

Change output annotation from:

```rust
#[output("output", "spread signal output", default)]
```

to:

```rust
#[output("output", "spread signal output", default, range = (-5.0, 5.0), dynamic_range)]
```

Add `set_range` calls inside the per-channel loop (after `self.outputs.sample.set(i, value)`):

```rust
self.outputs.sample.set_range(i, min_val, max_val);
```

Note: `min_val` and `max_val` are already computed at the top of `update()`. Every channel gets the same range bounds since spread interpolates between min and max.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p modular_core spread_has_dynamic_range -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/modular_core/src/dsp/utilities/spread.rs crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "feat: add dynamic_range to \$spread module"
```

---

### Task 6: Add `dynamic_range` to `$scaleAndShift` (composed range)

`$scaleAndShift` output = `input * (scale/5) + shift`. Output range depends on input range. Uses the new `get_range()` API.

If input has range `[inMin, inMax]` and scale `s` (after `/5.0`), shift `sh`:

- If `s >= 0`: `outMin = inMin * s + sh`, `outMax = inMax * s + sh`
- If `s < 0`: `outMin = inMax * s + sh`, `outMax = inMin * s + sh`

If input has no range, fall back to the static range annotation.

**Files:**

- Modify: `crates/modular_core/src/dsp/utilities/scale_and_shift.rs`
- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Write failing tests**

Add to `crates/modular_core/tests/dsp_fresh_tests.rs`:

```rust
#[test]
fn scale_and_shift_has_dynamic_range_from_input() {
    // $pulse at 25% width → range [-2.5, 7.5]
    // $scaleAndShift with scale=5 (unity), shift=1 → range [-1.5, 8.5]
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "pulse".into(),
        serde_json::from_value(json!({
            "type": "$pulse",
            "params": { "freq": 0.0, "width": 1.25 }
        }))
        .unwrap(),
    );
    patch.modules.insert(
        "ss".into(),
        serde_json::from_value(json!({
            "type": "$scaleAndShift",
            "params": {
                "input": { "type": "cable", "module": "pulse", "port": "output", "channel": 0 },
                "scale": 5.0,
                "shift": 1.0
            }
        }))
        .unwrap(),
    );
    patch.init();

    // Run enough samples for smoothing
    for _ in 0..1000 {
        for name in ["pulse", "ss"] {
            patch.sampleables.get(name).unwrap().get_sample("output", 0);
        }
    }

    let ss = patch.sampleables.get("ss").unwrap();
    let range_min = ss.get_sample("output.rangeMin", 0);
    let range_max = ss.get_sample("output.rangeMax", 0);

    assert!(range_min.is_some(), "scaleAndShift should expose rangeMin");
    assert!(range_max.is_some(), "scaleAndShift should expose rangeMax");
    assert!(
        (range_min.unwrap() - (-1.5)).abs() < 0.2,
        "rangeMin should be ~-1.5, got {}",
        range_min.unwrap()
    );
    assert!(
        (range_max.unwrap() - 8.5).abs() < 0.2,
        "rangeMax should be ~8.5, got {}",
        range_max.unwrap()
    );
}

#[test]
fn scale_and_shift_negative_scale_swaps_range() {
    // $pulse at 50% → range [-5, 5]
    // $scaleAndShift with scale=-5 (invert), shift=0 → range [-5, 5] (swapped)
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "pulse".into(),
        serde_json::from_value(json!({
            "type": "$pulse",
            "params": { "freq": 0.0 }
        }))
        .unwrap(),
    );
    patch.modules.insert(
        "ss".into(),
        serde_json::from_value(json!({
            "type": "$scaleAndShift",
            "params": {
                "input": { "type": "cable", "module": "pulse", "port": "output", "channel": 0 },
                "scale": -5.0,
                "shift": 0.0
            }
        }))
        .unwrap(),
    );
    patch.init();

    for _ in 0..1000 {
        for name in ["pulse", "ss"] {
            patch.sampleables.get(name).unwrap().get_sample("output", 0);
        }
    }

    let ss = patch.sampleables.get("ss").unwrap();
    let range_min = ss.get_sample("output.rangeMin", 0).unwrap();
    let range_max = ss.get_sample("output.rangeMax", 0).unwrap();

    // scale = -5/5 = -1, so: outMin = 5 * -1 + 0 = -5, outMax = -5 * -1 + 0 = 5
    assert!(
        (range_min - (-5.0)).abs() < 0.2,
        "rangeMin should be ~-5.0, got {range_min}"
    );
    assert!(
        (range_max - 5.0).abs() < 0.2,
        "rangeMax should be ~5.0, got {range_max}"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p modular_core scale_and_shift -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement dynamic_range on $scaleAndShift**

In `crates/modular_core/src/dsp/utilities/scale_and_shift.rs`:

Change output annotation from:

```rust
#[output("output", "signal output", default)]
```

to:

```rust
#[output("output", "signal output", default, range = (-5.0, 5.0), dynamic_range)]
```

Update the `update` method to compute and set range:

```rust
fn update(&mut self, _sample_rate: f32) {
    let channels = self.channel_count();

    for i in 0..channels as usize {
        let input_val = self.params.input.get_value(i);
        let scale_val = self.params.scale.value_or(i, 5.0);
        let shift_val = self.params.shift.value_or(i, 0.0);

        let s = scale_val / 5.0;
        self.outputs.sample.set(i, input_val * s + shift_val);

        // Compose range from input
        if let Some((in_min, in_max)) = self.params.input.get_range(i) {
            let a = in_min * s + shift_val;
            let b = in_max * s + shift_val;
            if s >= 0.0 {
                self.outputs.sample.set_range(i, a, b);
            } else {
                self.outputs.sample.set_range(i, b, a);
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p modular_core scale_and_shift -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/modular_core/src/dsp/utilities/scale_and_shift.rs crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "feat: add dynamic_range to \$scaleAndShift with composed range from input"
```

---

### Task 7: Add `dynamic_range` to `$clamp` (composed range)

`$clamp` constrains input to `[min, max]`. Output range is the intersection of the input range and the clamp bounds:

- `outMin = max(inMin, clampMin)`, `outMax = min(inMax, clampMax)`
- When min/max are disconnected, use the input's range for that bound.
- When input has no range, fall back to static range.

**Files:**

- Modify: `crates/modular_core/src/dsp/utilities/clamp.rs`
- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Write failing tests**

Add to `crates/modular_core/tests/dsp_fresh_tests.rs`:

```rust
#[test]
fn clamp_has_dynamic_range_both_bounds() {
    // $pulse at 50% → range [-5, 5]
    // $clamp with min=-2, max=3 → range [-2, 3]
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "pulse".into(),
        serde_json::from_value(json!({
            "type": "$pulse",
            "params": { "freq": 0.0 }
        }))
        .unwrap(),
    );
    patch.modules.insert(
        "clamp".into(),
        serde_json::from_value(json!({
            "type": "$clamp",
            "params": {
                "input": { "type": "cable", "module": "pulse", "port": "output", "channel": 0 },
                "min": -2.0,
                "max": 3.0
            }
        }))
        .unwrap(),
    );
    patch.init();

    for _ in 0..100 {
        for name in ["pulse", "clamp"] {
            patch.sampleables.get(name).unwrap().get_sample("output", 0);
        }
    }

    let clamp = patch.sampleables.get("clamp").unwrap();
    let range_min = clamp.get_sample("output.rangeMin", 0).unwrap();
    let range_max = clamp.get_sample("output.rangeMax", 0).unwrap();

    assert!(
        (range_min - (-2.0)).abs() < 0.1,
        "rangeMin should be ~-2.0, got {range_min}"
    );
    assert!(
        (range_max - 3.0).abs() < 0.1,
        "rangeMax should be ~3.0, got {range_max}"
    );
}

#[test]
fn clamp_dynamic_range_one_sided() {
    // $pulse at 50% → range [-5, 5]
    // $clamp with only min=0 → range [0, 5]
    let mut patch = Patch::default();
    patch.sample_rate = 44100.0;
    patch.modules.insert(
        "pulse".into(),
        serde_json::from_value(json!({
            "type": "$pulse",
            "params": { "freq": 0.0 }
        }))
        .unwrap(),
    );
    patch.modules.insert(
        "clamp".into(),
        serde_json::from_value(json!({
            "type": "$clamp",
            "params": {
                "input": { "type": "cable", "module": "pulse", "port": "output", "channel": 0 },
                "min": 0.0
            }
        }))
        .unwrap(),
    );
    patch.init();

    for _ in 0..100 {
        for name in ["pulse", "clamp"] {
            patch.sampleables.get(name).unwrap().get_sample("output", 0);
        }
    }

    let clamp = patch.sampleables.get("clamp").unwrap();
    let range_min = clamp.get_sample("output.rangeMin", 0).unwrap();
    let range_max = clamp.get_sample("output.rangeMax", 0).unwrap();

    assert!(
        (range_min - 0.0).abs() < 0.1,
        "rangeMin should be ~0.0, got {range_min}"
    );
    assert!(
        (range_max - 5.0).abs() < 0.1,
        "rangeMax should be ~5.0, got {range_max}"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p modular_core clamp_has_dynamic_range clamp_dynamic_range_one -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement dynamic_range on $clamp**

In `crates/modular_core/src/dsp/utilities/clamp.rs`:

Change output annotation from:

```rust
#[output("output", "clamped signal output", default)]
```

to:

```rust
#[output("output", "clamped signal output", default, range = (-5.0, 5.0), dynamic_range)]
```

Update the `update` method to compute and set range after the existing value clamping logic:

```rust
fn update(&mut self, _sample_rate: f32) {
    let channels = self.channel_count();
    let has_min = !self.params.min.is_disconnected();
    let has_max = !self.params.max.is_disconnected();

    for i in 0..channels as usize {
        let mut val = self.params.input.get_value(i);

        match (has_min, has_max) {
            (true, true) => {
                let a = self.params.min.value_or_zero(i);
                let b = self.params.max.value_or_zero(i);
                let (lo, hi) = if b < a { (b, a) } else { (a, b) };
                val = val.clamp(lo, hi);
            }
            (true, false) => {
                let min_val = self.params.min.value_or_zero(i);
                if val < min_val {
                    val = min_val;
                }
            }
            (false, true) => {
                let max_val = self.params.max.value_or_zero(i);
                if val > max_val {
                    val = max_val;
                }
            }
            (false, false) => {}
        }

        self.outputs.sample.set(i, val);

        // Compose range from input
        if let Some((in_min, in_max)) = self.params.input.get_range(i) {
            let out_min = if has_min {
                let a = self.params.min.value_or_zero(i);
                let b = if has_max { self.params.max.value_or_zero(i) } else { a };
                let lo = if has_min && has_max && b < a { b } else { a };
                in_min.max(lo)
            } else {
                in_min
            };
            let out_max = if has_max {
                let a = self.params.min.value_or_zero(i);
                let b = self.params.max.value_or_zero(i);
                let hi = if has_min && b < a { a } else { b };
                in_max.min(hi)
            } else {
                in_max
            };
            self.outputs.sample.set_range(i, out_min, out_max);
        }
    }
}
```

Wait — this range logic is getting complex with the swap handling. Let me simplify. The clamp bounds `lo`/`hi` are already computed in the match block above. Let me restructure to reuse them:

```rust
fn update(&mut self, _sample_rate: f32) {
    let channels = self.channel_count();
    let has_min = !self.params.min.is_disconnected();
    let has_max = !self.params.max.is_disconnected();

    for i in 0..channels as usize {
        let mut val = self.params.input.get_value(i);

        // Compute effective bounds
        let clamp_lo = if has_min { Some(self.params.min.value_or_zero(i)) } else { None };
        let clamp_hi = if has_max { Some(self.params.max.value_or_zero(i)) } else { None };

        // Swap if both present and inverted
        let (clamp_lo, clamp_hi) = match (clamp_lo, clamp_hi) {
            (Some(a), Some(b)) if b < a => (Some(b), Some(a)),
            other => other,
        };

        // Apply clamp
        if let Some(lo) = clamp_lo {
            val = val.max(lo);
        }
        if let Some(hi) = clamp_hi {
            val = val.min(hi);
        }

        self.outputs.sample.set(i, val);

        // Compose range from input
        if let Some((in_min, in_max)) = self.params.input.get_range(i) {
            let out_min = match clamp_lo {
                Some(lo) => in_min.max(lo),
                None => in_min,
            };
            let out_max = match clamp_hi {
                Some(hi) => in_max.min(hi),
                None => in_max,
            };
            self.outputs.sample.set_range(i, out_min, out_max);
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p modular_core clamp_has_dynamic_range clamp_dynamic_range_one -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test -p modular_core`
Expected: All pass

- [ ] **Step 6: Commit**

```bash
git add crates/modular_core/src/dsp/utilities/clamp.rs crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "feat: add dynamic_range to \$clamp with composed range from input"
```

---

### Task 8: Schema verification and full integration test

Verify all 5 modules expose `dynamicRange: true` in their schemas, and test an end-to-end chain.

**Files:**

- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`
- Test: `src/main/dsl/__tests__/executor.test.ts`

- [ ] **Step 1: Write schema verification test**

Add to `crates/modular_core/tests/dsp_fresh_tests.rs`:

```rust
#[test]
fn utility_modules_have_dynamic_range_in_schema() {
    let schemas = modular_core::get_module_schemas();
    for module_name in ["$remap", "$wrap", "$spread", "$scaleAndShift", "$clamp"] {
        let schema = schemas.get(module_name).unwrap_or_else(|| panic!("missing schema for {module_name}"));
        let output = schema.outputs.iter().find(|o| o.default).unwrap();
        assert!(
            output.dynamic_range,
            "{module_name} default output should have dynamic_range = true"
        );
    }
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p modular_core utility_modules_have_dynamic_range -- --nocapture`
Expected: PASS (if all previous tasks completed)

- [ ] **Step 3: Build native module and run TypeScript tests**

Run:

```bash
yarn build-native
yarn test:unit
```

Expected: All pass. The TypeScript DSL should automatically pick up the new `dynamicRange: true` flags from schemas.json.

- [ ] **Step 4: Run full test suite**

Run: `cargo test -p modular_core && yarn test:unit`
Expected: All pass

- [ ] **Step 5: Commit**

```bash
git add crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "test: verify dynamic_range schema for all 5 utility modules"
```
