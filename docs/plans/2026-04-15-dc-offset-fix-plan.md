# DC Offset Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate unintentional DC offset accumulation from the `$pulse` oscillator by applying analytic DC subtraction, making output range annotations runtime-dynamic (per-channel), and adding a `get_sample()` fast path to avoid copying the now-larger `PolyOutput` on every cable read.

**Architecture:** Three coupled changes: (1) DC subtraction in `pulse.rs` with per-channel `set_range()` calls, (2) per-channel `range_min`/`range_max` arrays on `PolyOutput` + `dynamic_range` derive macro annotation + virtual range ports + TypeScript `.range()` dynamic wiring, (3) `get_sample(port, channel) -> f32` fast path on `Sampleable`/`OutputStruct` traits to avoid full `PolyOutput` copy on every cable read.

**Tech Stack:** Rust (edition 2024), proc macros (syn/quote), TypeScript, N-API (napi-rs), Vitest, cargo test

**Design spec:** `docs/plans/2026-04-14-dc-offset-fix-design.md`

---

### Task 1: Add per-channel range arrays to `PolyOutput`

**Files:**

- Modify: `crates/modular_core/src/poly.rs:26-40` (PolyOutput struct + Default impl)
- Test: `crates/modular_core/src/poly.rs` (inline #[cfg(test)] module)

- [ ] **Step 1: Write failing tests for the range API**

Add these tests to the existing `#[cfg(test)] mod tests` block at the bottom of `crates/modular_core/src/poly.rs` (after the `test_mono_signal_ext_some` test at line 728):

```rust
#[test]
fn test_poly_output_range_defaults_to_nan() {
    let po = PolyOutput::default();
    // All range values should be NaN by default (unknown)
    assert!(po.range_min_value(0).is_nan());
    assert!(po.range_max_value(0).is_nan());
    assert!(!po.has_range());
    assert!(po.channel_range(0).is_none());
}

#[test]
fn test_poly_output_set_range() {
    let mut po = PolyOutput::default();
    po.set_channels(2);
    po.set(0, 1.0);
    po.set(1, 2.0);
    po.set_range(0, -2.5, 7.5);
    po.set_range(1, -7.5, 2.5);

    assert!(po.has_range());
    assert_eq!(po.channel_range(0), Some((-2.5, 7.5)));
    assert_eq!(po.channel_range(1), Some((-7.5, 2.5)));
    // Channel without range set should still be NaN
    assert!(po.channel_range(2).is_none());
}

#[test]
fn test_poly_output_range_survives_copy() {
    let mut po = PolyOutput::default();
    po.set_channels(1);
    po.set(0, 3.0);
    po.set_range(0, -2.5, 7.5);

    let copy = po;
    assert_eq!(copy.channel_range(0), Some((-2.5, 7.5)));
    assert_eq!(copy.get(0), 3.0);
}

#[test]
fn test_poly_output_mono_has_no_range() {
    let po = PolyOutput::mono(5.0);
    assert!(!po.has_range());
    assert!(po.channel_range(0).is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p modular_core poly::tests::test_poly_output_range -- --no-capture 2>&1 | head -30`
Expected: Compilation errors — `set_range`, `channel_range`, `has_range`, `range_min_value`, `range_max_value` don't exist yet.

- [ ] **Step 3: Add range fields to `PolyOutput` struct and Default impl**

In `crates/modular_core/src/poly.rs`, replace the struct definition (lines 26-31) and Default impl (lines 33-40):

```rust
#[derive(Clone, Copy, Debug)]
pub struct PolyOutput {
    /// Voltage values for each channel (always allocated, not all may be active)
    voltages: [f32; PORT_MAX_CHANNELS],
    /// Number of active channels: 0 = disconnected, 1 = mono, 2-16 = poly
    channels: usize,
    /// Per-channel minimum output range. NaN = unknown (no range metadata).
    range_min: [f32; PORT_MAX_CHANNELS],
    /// Per-channel maximum output range. NaN = unknown (no range metadata).
    range_max: [f32; PORT_MAX_CHANNELS],
}

impl Default for PolyOutput {
    fn default() -> Self {
        Self {
            voltages: [0.0; PORT_MAX_CHANNELS],
            channels: 0, // Disconnected
            range_min: [f32::NAN; PORT_MAX_CHANNELS],
            range_max: [f32::NAN; PORT_MAX_CHANNELS],
        }
    }
}
```

- [ ] **Step 4: Add range methods to `PolyOutput` impl block**

In the `impl PolyOutput` block (after the `set_all` method at line 111), add:

```rust
    // === Range metadata ===

    /// Set the output range for a specific channel.
    pub fn set_range(&mut self, channel: usize, min: f32, max: f32) {
        if channel < PORT_MAX_CHANNELS {
            self.range_min[channel] = min;
            self.range_max[channel] = max;
        }
    }

    /// Get the output range for a specific channel.
    /// Returns None if the range is unknown (NaN sentinel).
    pub fn channel_range(&self, channel: usize) -> Option<(f32, f32)> {
        if channel < PORT_MAX_CHANNELS {
            let min = self.range_min[channel];
            let max = self.range_max[channel];
            if min.is_nan() || max.is_nan() {
                None
            } else {
                Some((min, max))
            }
        } else {
            None
        }
    }

    /// Check if any channel has range metadata set.
    pub fn has_range(&self) -> bool {
        self.range_min.iter().any(|v| !v.is_nan())
    }

    /// Raw range_min value for a channel (may be NaN). Used by virtual range ports.
    pub fn range_min_value(&self, channel: usize) -> f32 {
        if channel < PORT_MAX_CHANNELS {
            self.range_min[channel]
        } else {
            f32::NAN
        }
    }

    /// Raw range_max value for a channel (may be NaN). Used by virtual range ports.
    pub fn range_max_value(&self, channel: usize) -> f32 {
        if channel < PORT_MAX_CHANNELS {
            self.range_max[channel]
        } else {
            f32::NAN
        }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p modular_core poly::tests::test_poly_output_range -- --no-capture`
Expected: All 4 new tests PASS. Existing poly tests also pass.

- [ ] **Step 6: Run full modular_core test suite**

Run: `cargo test -p modular_core`
Expected: All tests pass (the 3 DC offset tests still fail — that's expected, they'll be fixed in Task 5).

- [ ] **Step 7: Commit**

```bash
git add crates/modular_core/src/poly.rs
git commit -m "feat(poly): add per-channel range_min/range_max arrays to PolyOutput"
```

---

### Task 2: Add `get_sample()` to `OutputStruct` trait and derive macro

**Files:**

- Modify: `crates/modular_core/src/types.rs:1454-1470` (OutputStruct trait)
- Modify: `crates/modular_derive/src/outputs.rs:302-401` (derive macro — get_poly_sample arms, generated impl)
- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Write failing test for `get_sample` via Sampleable**

Add to `crates/modular_core/tests/dsp_fresh_tests.rs` (after the `sine_has_zero_dc` test at the end of file):

```rust
// ─── get_sample tests ────────────────────────────────────────────────────────

#[test]
fn get_sample_matches_get_poly_sample_for_pulse() {
    let osc = make_module("$pulse", "pulse-gs", json!({ "freq": 0.0, "width": 2.5 }));
    for _ in 0..100 {
        step(&**osc);
        let poly = osc.get_poly_sample(DEFAULT_PORT).unwrap();
        let via_get_sample = osc.get_sample(DEFAULT_PORT, 0).unwrap();
        assert_eq!(
            poly.get_cycling(0),
            via_get_sample,
            "get_sample should match get_poly_sample().get_cycling()"
        );
    }
}

#[test]
fn get_sample_matches_get_poly_sample_for_clock() {
    // Clock has f32 outputs — tests that path too
    let clk = make_module("$clock", "clk-gs", json!({ "bpm": 120.0 }));
    for _ in 0..100 {
        step(&**clk);
        let poly = clk.get_poly_sample("beatTrigger").unwrap();
        let via_get_sample = clk.get_sample("beatTrigger", 0).unwrap();
        assert_eq!(
            poly.get_cycling(0),
            via_get_sample,
            "get_sample on f32 output should match get_poly_sample"
        );
    }
}

#[test]
fn get_sample_returns_error_for_invalid_port() {
    let osc = make_module("$sine", "sine-gs", json!({ "freq": 0.0 }));
    step(&**osc);
    let result = osc.get_sample("nonexistent", 0);
    assert!(result.is_err(), "get_sample on invalid port should return Err");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p modular_core get_sample -- --no-capture 2>&1 | head -20`
Expected: Compilation error — `get_sample` doesn't exist on `Sampleable`.

- [ ] **Step 3: Add `get_sample` to `OutputStruct` trait**

In `crates/modular_core/src/types.rs`, add a new method to the `OutputStruct` trait (after `get_poly_sample` at line 1457):

```rust
pub trait OutputStruct: Default + Send + Sync + 'static {
    fn copy_from(&mut self, other: &Self);
    /// Get polyphonic sample output for a port.
    fn get_poly_sample(&self, port: &str) -> Option<PolyOutput>;
    /// Get a single sample value for a specific port and channel.
    /// This avoids copying the full PolyOutput — used by Signal::Cable::get_value().
    fn get_sample(&self, port: &str, channel: usize) -> Option<f32>;
    /// Set the channel count on all PolyOutput fields.
    fn set_all_channels(&mut self, channels: usize);
    fn schemas() -> Vec<OutputSchema>
    where
        Self: Sized;
    /// Transfer buffer data from old outputs to new outputs during always-reconstruct.
    /// Default: no-op. Modules with buffer outputs override this.
    fn transfer_buffers_from(&mut self, _old: &mut Self) {}
    /// Get a buffer output by port name. Default: no buffer outputs.
    fn get_buffer_output(&self, _port: &str) -> Option<&Arc<BufferData>> {
        None
    }
}
```

- [ ] **Step 4: Add `get_sample` to `Sampleable` trait**

In `crates/modular_core/src/types.rs`, add to the `Sampleable` trait (after `get_poly_sample` at line 158):

```rust
    /// Get a single sample value for a specific port and channel.
    /// Fast path that avoids copying the full PolyOutput.
    fn get_sample(&self, port: &str, channel: usize) -> Result<f32>;
```

- [ ] **Step 5: Generate `get_sample` match arms in derive macro**

In `crates/modular_derive/src/outputs.rs`, after the `poly_sample_match_arms` generation (line 317), add a new match arm generator:

```rust
    // Generate get_sample match arms (returns single f32, no PolyOutput copy)
    let sample_match_arms: Vec<_> = outputs
        .iter()
        .map(|o| {
            let output_name = &o.output_name;
            let field_name = &o.field_name;
            match o.precision {
                OutputPrecision::F32 => quote! {
                    #output_name => Some(self.#field_name),
                },
                OutputPrecision::PolySignal => quote! {
                    #output_name => Some(self.#field_name.get_cycling(channel)),
                },
            }
        })
        .collect();
```

Then add the method implementation to the generated `impl OutputStruct for #name` block (after the `get_poly_sample` method, before `set_all_channels`):

```rust
            fn get_sample(&self, port: &str, channel: usize) -> Option<f32> {
                match port {
                    #(#sample_match_arms)*
                    _ => None,
                }
            }
```

- [ ] **Step 6: Generate `get_sample` wrapper on Sampleable impl in module_attr.rs**

In `crates/modular_derive/src/module_attr.rs`, inside the `impl crate::types::Sampleable for #struct_name` block (after `get_poly_sample` at around line 663), add:

```rust
            fn get_sample(&self, port: &str, channel: usize) -> napi::Result<f32> {
                self.update();
                let outputs = unsafe { &*self.outputs.get() };
                crate::types::OutputStruct::get_sample(outputs, port, channel).ok_or_else(|| {
                    napi::Error::from_reason(
                        format!(
                            "{} with id {} does not have port {}",
                            #module_name,
                            &self.id,
                            port
                        )
                    )
                })
            }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p modular_core get_sample -- --no-capture`
Expected: All 3 new tests PASS.

- [ ] **Step 8: Run full test suite**

Run: `cargo test -p modular_core`
Expected: All tests pass (3 DC offset tests still fail — expected).

- [ ] **Step 9: Commit**

```bash
git add crates/modular_core/src/types.rs crates/modular_derive/src/outputs.rs crates/modular_derive/src/module_attr.rs crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "feat(types): add get_sample() fast path to OutputStruct and Sampleable"
```

---

### Task 3: Wire `Signal::Cable::get_value()` to use `get_sample()`

**Files:**

- Modify: `crates/modular_core/src/types.rs:1266-1282` (Signal::get_value)

- [ ] **Step 1: Write a test that exercises the Signal::Cable path**

This is already covered by the `get_sample_matches_get_poly_sample_*` tests from Task 2 and by all existing module tests that use cable connections. The change here is a performance optimization that preserves identical behavior. We verify by running the full suite.

- [ ] **Step 2: Update `Signal::get_value()` to use `get_sample()`**

In `crates/modular_core/src/types.rs`, replace the `get_value` method (lines 1266-1282):

```rust
    pub fn get_value(&self) -> f32 {
        match self {
            Signal::Volts(v) => *v,
            Signal::Cable {
                module_ptr,
                port,
                channel,
                ..
            } => match module_ptr.upgrade() {
                Some(module_ptr) => module_ptr
                    .get_sample(port, *channel)
                    .unwrap_or(0.0),
                None => 0.0,
            },
        }
    }
```

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p modular_core`
Expected: All tests pass. The behavior is identical — just faster because we avoid the PolyOutput copy.

- [ ] **Step 4: Commit**

```bash
git add crates/modular_core/src/types.rs
git commit -m "perf: use get_sample() fast path in Signal::Cable::get_value()"
```

---

### Task 4: Add `dynamic_range` annotation to derive macro + virtual range ports + `OutputSchema.dynamic_range`

**Files:**

- Modify: `crates/modular_derive/src/outputs.rs:16-21,46-143,302-345` (OutputAttr parsing, poly_sample arms, schema generation)
- Modify: `crates/modular_core/src/types.rs:1434-1452` (OutputSchema struct)
- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Add `dynamic_range` to `OutputSchema`**

In `crates/modular_core/src/types.rs`, add the field to `OutputSchema` (after `max_value` at line 1451):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[napi(object)]
pub struct OutputSchema {
    pub name: String,
    pub description: String,
    /// Whether this output is polyphonic (PolyOutput) or monophonic (f32/f64)
    #[serde(default)]
    pub polyphonic: bool,
    /// Whether this is the default output for the module
    #[serde(default)]
    pub default: bool,
    /// The minimum value of the raw output range (before any remapping)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_value: Option<f64>,
    /// The maximum value of the raw output range (before any remapping)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_value: Option<f64>,
    /// Whether this output provides dynamic per-channel range metadata at runtime.
    /// When true, virtual `.rangeMin`/`.rangeMax` ports are available.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic_range: bool,
}
```

- [ ] **Step 2: Add `dynamic_range` to `OutputAttr` parsing**

In `crates/modular_derive/src/outputs.rs`, add `dynamic_range` to the `OutputAttr` struct (line 16):

```rust
struct OutputAttr {
    name: LitStr,
    description: Option<LitStr>,
    is_default: bool,
    range: Option<(f64, f64)>,
    dynamic_range: bool,
}
```

Add `dynamic_range` field to `OutputAttrParser` struct (line 50):

```rust
    struct OutputAttrParser {
        name: LitStr,
        description: Option<LitStr>,
        is_default: bool,
        range: Option<(f64, f64)>,
        dynamic_range: bool,
    }
```

In the parsing loop (after the `range` branch at line 113), add handling for the `dynamic_range` keyword:

```rust
                    } else if ident == "dynamic_range" {
                        dynamic_range = true;
```

Initialize `dynamic_range` to `false` at the top of `parse()` (alongside `is_default` and `range`):

```rust
            let mut dynamic_range = false;
```

Include it in the `OutputAttrParser` return (line 126):

```rust
            Ok(OutputAttrParser {
                name,
                description: Some(description),
                is_default,
                range,
                dynamic_range,
            })
```

And in the `Ok(OutputAttr { ... })` conversion (line 137):

```rust
    Ok(OutputAttr {
        name: parsed.name,
        description: parsed.description,
        is_default: parsed.is_default,
        range: parsed.range,
        dynamic_range: parsed.dynamic_range,
    })
```

- [ ] **Step 3: Add `dynamic_range` to `OutputField` and wire it through**

Add the field to `OutputField` (line 31):

```rust
struct OutputField {
    field_name: Ident,
    output_name: LitStr,
    precision: OutputPrecision,
    description: TokenStream2,
    is_default: bool,
    range: Option<(f64, f64)>,
    dynamic_range: bool,
}
```

In the field construction (around line 209):

```rust
                    out.push(OutputField {
                        field_name,
                        output_name,
                        precision,
                        description,
                        is_default: output_attr.is_default,
                        range: output_attr.range,
                        dynamic_range: output_attr.dynamic_range,
                    });
```

- [ ] **Step 4: Generate virtual range port match arms in `get_poly_sample`**

In `crates/modular_derive/src/outputs.rs`, after the `poly_sample_match_arms` generator (around line 317), add virtual range port arms for outputs with `dynamic_range`:

```rust
    // Generate virtual range port match arms for dynamic_range outputs
    let virtual_range_arms: Vec<_> = outputs
        .iter()
        .filter(|o| o.dynamic_range && o.precision == OutputPrecision::PolySignal)
        .flat_map(|o| {
            let field_name = &o.field_name;
            let output_name_str = o.output_name.value();
            let range_min_name = format!("{}.rangeMin", output_name_str);
            let range_max_name = format!("{}.rangeMax", output_name_str);
            vec![
                quote! {
                    #range_min_name => {
                        let mut po = crate::poly::PolyOutput::default();
                        po.set_channels(self.#field_name.channels());
                        for ch in 0..self.#field_name.channels() {
                            po.set(ch, self.#field_name.range_min_value(ch));
                        }
                        Some(po)
                    },
                },
                quote! {
                    #range_max_name => {
                        let mut po = crate::poly::PolyOutput::default();
                        po.set_channels(self.#field_name.channels());
                        for ch in 0..self.#field_name.channels() {
                            po.set(ch, self.#field_name.range_max_value(ch));
                        }
                        Some(po)
                    },
                },
            ]
        })
        .collect();
```

Then update the generated `get_poly_sample` match block to include these arms (in the `generated` quote block):

```rust
            fn get_poly_sample(&self, port: &str) -> Option<crate::poly::PolyOutput> {
                match port {
                    #(#poly_sample_match_arms)*
                    #(#virtual_range_arms)*
                    _ => None,
                }
            }
```

- [ ] **Step 5: Add `dynamic_range` to schema generation**

In `crates/modular_derive/src/outputs.rs`, update the `schema_exprs` generator (around line 319) to include `dynamic_range`:

```rust
    let schema_exprs: Vec<_> = outputs
        .iter()
        .map(|o| {
            let output_name = &o.output_name;
            let description = &o.description;
            let is_polyphonic = o.precision == OutputPrecision::PolySignal;
            let is_default = o.is_default;
            let min_value = match o.range {
                Some((min, _)) => quote! { Some(#min) },
                None => quote! { None },
            };
            let max_value = match o.range {
                Some((_, max)) => quote! { Some(#max) },
                None => quote! { None },
            };
            let dynamic_range = o.dynamic_range;
            quote! {
                crate::types::OutputSchema {
                    name: #output_name.to_string(),
                    description: #description,
                    polyphonic: #is_polyphonic,
                    default: #is_default,
                    min_value: #min_value,
                    max_value: #max_value,
                    dynamic_range: #dynamic_range,
                }
            }
        })
        .collect();
```

- [ ] **Step 6: Run the full Rust test suite**

Run: `cargo test -p modular_core`
Expected: All tests pass. The `dynamic_range` field defaults to `false` on all existing modules, so behavior is unchanged. No module uses `dynamic_range` annotation yet (that comes in Task 5).

- [ ] **Step 7: Commit**

```bash
git add crates/modular_core/src/types.rs crates/modular_derive/src/outputs.rs
git commit -m "feat(derive): add dynamic_range annotation with virtual range ports"
```

---

### Task 5: Apply DC subtraction to `$pulse` + set per-channel range

**Files:**

- Modify: `crates/modular_core/src/dsp/oscillators/pulse.rs:35-40,76-118` (output annotation + update method)
- Test: `crates/modular_core/tests/dsp_fresh_tests.rs` (existing DC tests + new range tests)

- [ ] **Step 1: Write tests for virtual range ports on `$pulse`**

Add to `crates/modular_core/tests/dsp_fresh_tests.rs` (after the `get_sample_returns_error_for_invalid_port` test):

```rust
// ─── Virtual range port tests ────────────────────────────────────────────────

#[test]
fn pulse_virtual_range_ports_at_50_percent() {
    let osc = make_module("$pulse", "pulse-rp", json!({ "freq": 0.0, "width": 2.5 }));
    // Run a few samples to let Clickless settle
    for _ in 0..1000 {
        step(&**osc);
    }
    let range_min = osc.get_poly_sample("output.rangeMin").unwrap();
    let range_max = osc.get_poly_sample("output.rangeMax").unwrap();
    assert!(
        (range_min.get(0) - (-5.0)).abs() < 0.01,
        "rangeMin at 50% width should be -5.0, got {}",
        range_min.get(0)
    );
    assert!(
        (range_max.get(0) - 5.0).abs() < 0.01,
        "rangeMax at 50% width should be 5.0, got {}",
        range_max.get(0)
    );
}

#[test]
fn pulse_virtual_range_ports_at_25_percent() {
    // Width 1.25 = 25% duty cycle => min = -10*0.25 = -2.5, max = 10*0.75 = 7.5
    let osc = make_module("$pulse", "pulse-rp2", json!({ "freq": 0.0, "width": 1.25 }));
    for _ in 0..1000 {
        step(&**osc);
    }
    let range_min = osc.get_poly_sample("output.rangeMin").unwrap();
    let range_max = osc.get_poly_sample("output.rangeMax").unwrap();
    assert!(
        (range_min.get(0) - (-2.5)).abs() < 0.1,
        "rangeMin at 25% width should be ~-2.5, got {}",
        range_min.get(0)
    );
    assert!(
        (range_max.get(0) - 7.5).abs() < 0.1,
        "rangeMax at 25% width should be ~7.5, got {}",
        range_max.get(0)
    );
}

#[test]
fn pulse_output_stays_within_dynamic_range() {
    // Verify the actual signal stays within the declared range bounds
    for width in [1.25, 2.5, 3.75] {
        let osc = make_module("$pulse", "pulse-rng", json!({ "freq": 0.0, "width": width }));
        // Let Clickless settle
        for _ in 0..1000 {
            step(&**osc);
        }
        let range_min = osc.get_poly_sample("output.rangeMin").unwrap().get(0);
        let range_max = osc.get_poly_sample("output.rangeMax").unwrap().get(0);

        // Collect samples and verify bounds
        let samples = collect_samples(&**osc, 5000);
        let (mn, mx) = min_max(&samples);
        assert!(
            mn >= range_min - 0.1,
            "width={width}: sample min {mn} below rangeMin {range_min}"
        );
        assert!(
            mx <= range_max + 0.1,
            "width={width}: sample max {mx} above rangeMax {range_max}"
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p modular_core pulse_virtual_range -- --no-capture 2>&1 | head -20`
Expected: Fail — `get_poly_sample("output.rangeMin")` returns error because no virtual ports exist yet (no `dynamic_range` annotation on `$pulse`).

- [ ] **Step 3: Add `dynamic_range` annotation to `$pulse` outputs**

In `crates/modular_core/src/dsp/oscillators/pulse.rs`, update the output annotation (line 38):

```rust
#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PulseOscillatorOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0), dynamic_range)]
    sample: PolyOutput,
}
```

- [ ] **Step 4: Apply DC subtraction and set per-channel range in the update loop**

In `crates/modular_core/src/dsp/oscillators/pulse.rs`, replace line 116 (`self.outputs.sample.set(ch, naive_pulse * 5.0);`):

```rust
            // DC offset for this pulse width: (2w - 1) * 5V
            let dc = (2.0 * pulse_width - 1.0) * 5.0;
            self.outputs.sample.set(ch, naive_pulse * 5.0 - dc);

            // Per-channel range after DC subtraction:
            // min = -10 * pulse_width, max = 10 * (1 - pulse_width)
            self.outputs
                .sample
                .set_range(ch, -10.0 * pulse_width, 10.0 * (1.0 - pulse_width));
```

- [ ] **Step 5: Run the DC offset tests that were previously failing**

Run: `cargo test -p modular_core pulse_ -- --no-capture`
Expected: ALL pulse tests pass now, including:

- `pulse_square_wave_has_zero_dc` — PASS
- `pulse_narrow_width_has_zero_dc` — PASS (was FAIL)
- `pulse_wide_width_has_zero_dc` — PASS (was FAIL)
- `pulse_extreme_widths_have_zero_dc` — PASS (was FAIL)
- `pulse_preserves_amplitude_after_dc_fix` — PASS
- `pulse_virtual_range_ports_at_50_percent` — PASS
- `pulse_virtual_range_ports_at_25_percent` — PASS
- `pulse_output_stays_within_dynamic_range` — PASS

- [ ] **Step 6: Run the full Rust test suite**

Run: `cargo test -p modular_core`
Expected: ALL tests pass. Zero failures.

- [ ] **Step 7: Commit**

```bash
git add crates/modular_core/src/dsp/oscillators/pulse.rs crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "fix(pulse): apply analytic DC subtraction with per-channel dynamic range"
```

---

### Task 6: Update TypeScript DSL for dynamic range

**Files:**

- Modify: `src/main/dsl/GraphBuilder.ts:29-35,1052-1109,1277-1307` (OutputSchemaWithRange, \_output(), ModuleOutputWithRange)
- Modify: `src/main/dsl/typescriptLibGen.ts:525-540,1019-1030` (interface + getOutputType)
- Modify: `src/main/dsl/paramsSchema.ts` (passthrough)

- [ ] **Step 1: Add `dynamicRange` to `OutputSchemaWithRange` interface**

In `src/main/dsl/GraphBuilder.ts`, update the interface (lines 29-35):

```typescript
export interface OutputSchemaWithRange {
    name: string;
    description: string;
    polyphonic?: boolean;
    minValue?: number;
    maxValue?: number;
    dynamicRange?: boolean;
}
```

- [ ] **Step 2: Add `dynamicRange` to `ModuleOutputWithRange` class**

In `src/main/dsl/GraphBuilder.ts`, update the class (lines 1277-1307):

```typescript
export class ModuleOutputWithRange extends ModuleOutput {
    readonly minValue: number;
    readonly maxValue: number;
    readonly dynamicRange: boolean;

    constructor(
        builder: GraphBuilder,
        moduleId: string,
        portName: string,
        channel: number = 0,
        minValue: number,
        maxValue: number,
        dynamicRange: boolean = false,
    ) {
        super(builder, moduleId, portName, channel);
        this.minValue = minValue;
        this.maxValue = maxValue;
        this.dynamicRange = dynamicRange;
    }

    /**
     * Remap this output from its known range to a new range.
     * For dynamic range outputs, wires virtual range ports as cable inputs.
     * For static range outputs, passes literal min/max numbers.
     */
    range(outMin: PolySignal, outMax: PolySignal): Collection {
        const factory = this.builder.getFactory('$remap');
        if (this.dynamicRange) {
            // Wire virtual range ports as cable inputs to $remap's inMin/inMax
            const rangeMin = new ModuleOutput(
                this.builder,
                this.moduleId,
                `${this.portName}.rangeMin`,
                this.channel,
            );
            const rangeMax = new ModuleOutput(
                this.builder,
                this.moduleId,
                `${this.portName}.rangeMax`,
                this.channel,
            );
            return factory(
                this,
                outMin,
                outMax,
                rangeMin,
                rangeMax,
            ) as Collection;
        }
        return factory(
            this,
            outMin,
            outMax,
            this.minValue,
            this.maxValue,
        ) as Collection;
    }
}
```

- [ ] **Step 3: Pass `dynamicRange` through `_output()` method**

In `src/main/dsl/GraphBuilder.ts`, update the `_output` method (around lines 1073-1106) to pass `dynamicRange`:

In the polyphonic path (line 1077):

```typescript
outputs.push(
    new ModuleOutputWithRange(
        this.builder,
        this.id,
        portName,
        i,
        outputSchema.minValue!,
        outputSchema.maxValue!,
        outputSchema.dynamicRange ?? false,
    ),
);
```

In the monophonic path (line 1099):

```typescript
return new ModuleOutputWithRange(
    this.builder,
    this.id,
    portName,
    0,
    outputSchema.minValue!,
    outputSchema.maxValue!,
    outputSchema.dynamicRange ?? false,
);
```

- [ ] **Step 4: Update `typescriptLibGen.ts` — `ModuleOutputWithRange` interface**

In `src/main/dsl/typescriptLibGen.ts`, update the `ModuleOutputWithRange` interface in `BASE_LIB_SOURCE` (around line 525):

```typescript
interface ModuleOutputWithRange extends ModuleOutput {
    /** The minimum value this output produces (static fallback) */
    readonly minValue: number;
    /** The maximum value this output produces (static fallback) */
    readonly maxValue: number;
    /** Whether this output provides dynamic per-channel range at runtime */
    readonly dynamicRange: boolean;

    /**
     * Remap the output from its native range to a new range.
     * For dynamic range outputs, uses runtime per-channel bounds.
     * For static range outputs, uses the stored minValue/maxValue.
     * @param outMin - New minimum as {@link Poly<Signal>}
     * @param outMax - New maximum as {@link Poly<Signal>}
     * @returns A {@link ModuleOutput} with the remapped signal
     * @example lfo.range(note("C3"), note("C5"))
     */
    range(outMin: Poly<Signal>, outMax: Poly<Signal>): ModuleOutput;
}
```

- [ ] **Step 5: Update `paramsSchema.ts` to pass through `dynamicRange`**

Check if `paramsSchema.ts` explicitly maps `OutputSchema` fields. Based on the code analysis, it does a passthrough via spread — the `dynamicRange` field from Rust will flow through automatically via the N-API `#[napi(object)]` derive. No changes needed here if it uses spread. Verify this is the case.

If explicit field mapping exists, add `dynamicRange` to it.

- [ ] **Step 6: Build and verify TypeScript compiles**

Run: `yarn typecheck`
Expected: No type errors. The new `dynamicRange` field is optional with a default, so all existing code is compatible.

- [ ] **Step 7: Commit**

```bash
git add src/main/dsl/GraphBuilder.ts src/main/dsl/typescriptLibGen.ts
git commit -m "feat(dsl): wire dynamic range through TypeScript .range() method"
```

---

### Task 7: Build native module, regenerate types, and run full test suite

**Files:**

- Regenerate: `crates/modular/index.d.ts` (N-API type definitions — auto-generated)

- [ ] **Step 1: Build the native Rust module**

Run: `yarn build-native`
Expected: Build succeeds. The updated `OutputSchema` with `dynamic_range` field is reflected in the generated `.d.ts`.

- [ ] **Step 2: Regenerate TypeScript types**

Run: `yarn generate-lib`
Expected: Type generation succeeds.

- [ ] **Step 3: Run TypeScript type checking**

Run: `yarn typecheck`
Expected: No type errors.

- [ ] **Step 4: Run Rust tests**

Run: `cargo test -p modular_core`
Expected: ALL tests pass. Zero failures — including the 3 previously-failing DC offset tests.

- [ ] **Step 5: Run JS/TS unit tests**

Run: `yarn test:unit`
Expected: All tests pass.

- [ ] **Step 6: Verify the `OutputSchema` includes `dynamicRange` for `$pulse`**

Run a quick check that the schema is correct. In the test file or via a quick script, verify that `$pulse`'s output schema has `dynamic_range: true`.

Add this test to `crates/modular_core/tests/dsp_fresh_tests.rs`:

```rust
#[test]
fn pulse_output_schema_has_dynamic_range() {
    use modular_core::dsp::get_schemas;
    let schemas = get_schemas();
    let pulse_schema = schemas.iter().find(|s| s.name == "$pulse").expect("$pulse schema not found");
    let output_schema = pulse_schema
        .outputs
        .iter()
        .find(|o| o.name == "output")
        .expect("output not found in $pulse schema");
    assert!(
        output_schema.dynamic_range,
        "$pulse output should have dynamic_range = true"
    );
}
```

Run: `cargo test -p modular_core pulse_output_schema -- --no-capture`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "test: verify $pulse schema has dynamic_range flag"
```

---

### Task 8: End-to-end `.range()` test with dynamic bounds

**Files:**

- Test: `crates/modular_core/tests/dsp_fresh_tests.rs`

- [ ] **Step 1: Write end-to-end test for `.range()` accuracy at non-50% widths**

This test constructs a `$pulse` → `$remap` chain manually (without the DSL), verifying that when `$remap` reads the virtual range ports, the output is correctly remapped to [0, 1].

Add to `crates/modular_core/tests/dsp_fresh_tests.rs`:

```rust
#[test]
fn pulse_remap_via_virtual_range_ports() {
    // Simulate what .range(0, 1) does with dynamic range:
    // $pulse(width=1.25) → $remap(input, outMin=0, outMax=1, inMin=<virtual rangeMin>, inMax=<virtual rangeMax>)
    //
    // At 25% width: rangeMin=-2.5, rangeMax=7.5
    // So remap maps [-2.5, 7.5] → [0, 1]

    let pulse = make_module("$pulse", "pulse-e2e", json!({ "freq": 0.0, "width": 1.25 }));

    // Build a patch with the pulse module and a remap module that reads virtual ports
    let pulse_id = "pulse-e2e".to_string();
    let remap_id = "remap-e2e".to_string();

    let remap = make_module("$remap", &remap_id, json!({
        "input": { "type": "cable", "module": "pulse-e2e", "port": "output", "channel": 0 },
        "outMin": 0.0,
        "outMax": 1.0,
        "inMin": { "type": "cable", "module": "pulse-e2e", "port": "output.rangeMin", "channel": 0 },
        "inMax": { "type": "cable", "module": "pulse-e2e", "port": "output.rangeMax", "channel": 0 }
    }));

    // Connect both modules via a patch
    let graph = PatchGraph {
        modules: vec![
            ModuleState {
                id: pulse_id.clone(),
                module_type: "$pulse".to_string(),
                params: json!({ "freq": 0.0, "width": 1.25 }),
            },
            ModuleState {
                id: remap_id.clone(),
                module_type: "$remap".to_string(),
                params: json!({
                    "input": { "type": "cable", "module": "pulse-e2e", "port": "output", "channel": 0 },
                    "outMin": 0.0,
                    "outMax": 1.0,
                    "inMin": { "type": "cable", "module": "pulse-e2e", "port": "output.rangeMin", "channel": 0 },
                    "inMax": { "type": "cable", "module": "pulse-e2e", "port": "output.rangeMax", "channel": 0 }
                }),
            },
        ],
    };

    let patch = Patch::from_graph(&graph, SAMPLE_RATE).expect("patch construction failed");

    // Get the remap module from the patch
    let remap_module = patch.get_module(&remap_id).expect("remap module not found");

    // Let Clickless settle (remap uses Clickless on inMin/inMax)
    for _ in 0..2000 {
        for m in patch.modules_in_order() {
            m.tick();
        }
        for m in patch.modules_in_order() {
            m.update();
        }
    }

    // Collect samples from remap output
    let mut samples = Vec::new();
    for _ in 0..5000 {
        for m in patch.modules_in_order() {
            m.tick();
        }
        for m in patch.modules_in_order() {
            m.update();
        }
        let s = remap_module
            .get_poly_sample("output")
            .expect("remap get_poly_sample failed")
            .get(0);
        samples.push(s);
    }

    let (mn, mx) = min_max(&samples);
    // After remap to [0, 1], values should be in approximately [0, 1]
    assert!(
        mn > -0.05,
        "remapped pulse min should be near 0, got {mn}"
    );
    assert!(
        mx < 1.05,
        "remapped pulse max should be near 1, got {mx}"
    );
    assert!(
        mx - mn > 0.9,
        "remapped pulse should span most of [0,1], got range {}", mx - mn
    );
}
```

**Note:** This test may need adjustments based on the exact Patch API (whether `from_graph`, `get_module`, `modules_in_order` exist). The implementer should check the `Patch` struct's public API in `crates/modular_core/src/patch.rs` and adapt the test accordingly. The key verification is: pulse → remap with virtual range port cables → output in [0, 1].

If the Patch API doesn't support this kind of direct construction, an alternative is to verify just the virtual port values and trust that `$remap` works (it has its own test coverage):

```rust
#[test]
fn pulse_remap_inputs_correct_at_various_widths() {
    // Verify that virtual range ports provide correct inMin/inMax values
    // that would make $remap produce correct [0,1] output
    for (width_param, expected_min, expected_max) in [
        (1.25, -2.5, 7.5),   // 25% duty
        (2.5, -5.0, 5.0),    // 50% duty (square)
        (3.75, -7.5, 2.5),   // 75% duty
    ] {
        let osc = make_module("$pulse", "pulse-ri", json!({ "freq": 0.0, "width": width_param }));
        // Settle Clickless
        for _ in 0..1000 {
            step(&**osc);
        }
        let range_min = osc.get_poly_sample("output.rangeMin").unwrap().get(0);
        let range_max = osc.get_poly_sample("output.rangeMax").unwrap().get(0);
        assert!(
            (range_min - expected_min as f32).abs() < 0.1,
            "width={width_param}: rangeMin={range_min}, expected {expected_min}"
        );
        assert!(
            (range_max - expected_max as f32).abs() < 0.1,
            "width={width_param}: rangeMax={range_max}, expected {expected_max}"
        );
    }
}
```

Use the simpler alternative if the Patch API doesn't easily support multi-module test construction.

- [ ] **Step 2: Run the test**

Run: `cargo test -p modular_core pulse_remap -- --no-capture`
Expected: PASS.

- [ ] **Step 3: Run the complete test suite one final time**

Run: `cargo test -p modular_core && yarn test:unit`
Expected: ALL tests pass across both Rust and TypeScript.

- [ ] **Step 4: Commit**

```bash
git add crates/modular_core/tests/dsp_fresh_tests.rs
git commit -m "test: add end-to-end .range() test with dynamic bounds"
```

---

## Summary of Changes

| File                                               | Change                                                                                                                                                       |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `crates/modular_core/src/poly.rs`                  | Add `range_min`/`range_max` arrays to `PolyOutput` (+128 bytes), range API methods                                                                           |
| `crates/modular_core/src/types.rs`                 | Add `get_sample()` to `Sampleable` + `OutputStruct` traits; add `dynamic_range` to `OutputSchema`; update `Signal::get_value()` to use `get_sample()`        |
| `crates/modular_derive/src/outputs.rs`             | Parse `dynamic_range` annotation; generate `get_sample` match arms; generate virtual range port arms in `get_poly_sample`; include `dynamic_range` in schema |
| `crates/modular_derive/src/module_attr.rs`         | Generate `get_sample()` wrapper on `Sampleable` impl                                                                                                         |
| `crates/modular_core/src/dsp/oscillators/pulse.rs` | Add `dynamic_range` annotation; apply DC subtraction; call `set_range()` per channel                                                                         |
| `src/main/dsl/GraphBuilder.ts`                     | Add `dynamicRange` to schema interface; add `dynamicRange` to `ModuleOutputWithRange`; branch `.range()` for dynamic vs static                               |
| `src/main/dsl/typescriptLibGen.ts`                 | Update `ModuleOutputWithRange` interface with `dynamicRange`                                                                                                 |
| `crates/modular_core/tests/dsp_fresh_tests.rs`     | New tests: `get_sample` correctness, virtual range ports, DC offset (existing), schema flag, end-to-end `.range()`                                           |
