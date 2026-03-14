# Constructor Refactor & Required Params Enforcement — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor the `#[module]` proc macro constructor to accept typed params at creation time, eliminating the need for `Default` on param structs. Then make per-module signal fields required (bare `PolySignal`) or optional (`Option<PolySignal>`) based on semantic intent, and propagate that distinction through JSON schema, TypeScript types, and `deriveChannelCount` error reporting.

**Architecture:** Constructor changes from `fn(id, sample_rate) -> Sampleable` to `fn(id, sample_rate, DeserializedParams) -> Sampleable`. The `*Sampleable` wrapper no longer needs `Default` on the inner module. Deserialization happens before construction (already does today — params are deserialized on the main thread and applied via `apply_deserialized_params` on the audio thread). The key insight is to merge construction + initial param application into a single step.

**Tech Stack:** Rust (proc macros, serde, schemars, napi), TypeScript (typescriptLibGen.ts, factories.ts)

**Worktree:** `~/.config/superpowers/worktrees/modular/required-params` (branch `feature/required-params`)

**Baseline:** 477 Rust tests, 135 TS tests passing (commit `f62eee3`)

---

## Phase 1: Constructor Takes DeserializedParams

The current flow:

1. Main thread: `constructor(&id, sample_rate)` creates module with `Default::default()` params
2. Main thread: `deserialize_params()` deserializes JSON -> `DeserializedParams`
3. Both sent to audio thread via `PatchUpdate`
4. Audio thread: `apply_deserialized_params(deserialized)` swaps params

The new flow:

1. Main thread: `deserialize_params()` deserializes JSON -> `DeserializedParams`
2. Main thread: `constructor(&id, sample_rate, deserialized)` creates module with typed params
3. Constructed module sent to audio thread via `PatchUpdate`
4. Audio thread: no initial param application needed (already applied at construction)

### Task 1.1: Change SampleableConstructor signature

**Files:**

- Modify: `crates/modular_core/src/types.rs:1036`

**Step 1: Update the SampleableConstructor type alias**

The current type alias:

```rust
pub type SampleableConstructor = Box<dyn Fn(&String, f32) -> Result<Arc<Box<dyn Sampleable>>>>;
```

Change to accept `DeserializedParams`:

```rust
pub type SampleableConstructor = Box<dyn Fn(&String, f32, DeserializedParams) -> Result<Arc<Box<dyn Sampleable>>>>;
```

Add `use crate::params::DeserializedParams;` at the top if not already imported.

**Step 2: Verify it compiles (it won't — expected failures downstream)**

Run: `cargo check -p modular_core 2>&1 | head -30`
Expected: Compile errors in places that use `SampleableConstructor` (proc macro generated code, audio.rs)

---

### Task 1.2: Update proc macro constructor generation

**Files:**

- Modify: `crates/modular_derive/src/module_attr.rs` (the `#constructor_name` function and `*Sampleable` struct)

**Step 1: Change the generated constructor to accept DeserializedParams**

In `module_attr.rs`, find the generated constructor function (around line 630):

```rust
fn #constructor_name(id: &String, sample_rate: f32) -> napi::Result<std::sync::Arc<Box<dyn crate::types::Sampleable>>> {
    let sampleable = #struct_name {
        id: id.clone(),
        sample_rate,
        ..#struct_name::default()
    };
    #has_init_call
    Ok(std::sync::Arc::new(Box::new(sampleable)))
}
```

Change to:

```rust
fn #constructor_name(id: &String, sample_rate: f32, deserialized: crate::params::DeserializedParams) -> napi::Result<std::sync::Arc<Box<dyn crate::types::Sampleable>>> {
    let concrete_params = deserialized.params.into_any()
        .downcast::<#params_struct_name>()
        .map_err(|_| napi::Error::from_reason(
            format!("Failed to downcast params for module type {}", #module_name)
        ))?;

    let mut inner = #name {
        params: *concrete_params,
        outputs: Default::default(),
        _channel_count: deserialized.channel_count,
        ..Default::default()
    };

    // Set output channel counts
    crate::types::OutputStruct::set_all_channels(&mut inner.outputs, deserialized.channel_count);

    let sampleable = #struct_name {
        id: id.clone(),
        sample_rate,
        outputs: std::cell::UnsafeCell::new(Default::default()),
        module: std::cell::UnsafeCell::new(inner),
        processed: core::sync::atomic::AtomicBool::new(false),
        argument_spans: std::cell::UnsafeCell::new(deserialized.argument_spans),
    };
    #has_init_call
    Ok(std::sync::Arc::new(Box::new(sampleable)))
}
```

**IMPORTANT:** This approach constructs the inner module struct field-by-field, with `params` coming from the deserialized data and other fields from `Default`. This means we still need `Default` on the **module struct** itself (e.g., `SineOscillator`), but NOT on the **params struct** (e.g., `SineOscillatorParams`).

**Problem:** The `..Default::default()` on the inner module struct won't work because we're already setting `params`, `outputs`, and `_channel_count` — but there may be other fields (like `channels: [ChannelState; 16]`). We need a different approach.

**Better approach — construct the Sampleable wrapper directly without Default on the inner module:**

Instead of using `..Default::default()` on the inner struct, we need to initialize the inner module with the params and let `init()` handle the rest. But the inner module struct has arbitrary fields (phase accumulators, lookup tables, etc.) that need initialization.

**The cleanest path:** Keep `Default` on the **module struct** (not the params struct). The module struct's `Default` impl will use `Default` for everything EXCEPT params. Then we overwrite `params` with the real data.

Actually, looking more carefully at the current code, the `*Sampleable` wrapper struct already has its own `Default` impl (generated by the proc macro). The constructor currently uses `..#struct_name::default()` (the wrapper's Default), not the inner module's Default. The wrapper's Default creates a default inner module.

**Revised approach:**

```rust
fn #constructor_name(id: &String, sample_rate: f32, deserialized: crate::params::DeserializedParams) -> napi::Result<std::sync::Arc<Box<dyn crate::types::Sampleable>>> {
    let concrete_params = deserialized.params.into_any()
        .downcast::<#params_struct_name>()
        .map_err(|_| napi::Error::from_reason(
            format!("Failed to downcast params for module type {}", #module_name)
        ))?;

    let sampleable = #struct_name {
        id: id.clone(),
        sample_rate,
        outputs: std::cell::UnsafeCell::new(Default::default()),
        module: std::cell::UnsafeCell::new(Default::default()),
        processed: core::sync::atomic::AtomicBool::new(false),
        argument_spans: std::cell::UnsafeCell::new(deserialized.argument_spans),
    };

    // Apply params to the just-created module
    // SAFETY: We just created sampleable, no one else has access yet.
    unsafe {
        let module = &mut *sampleable.module.get();
        module.params = *concrete_params;
        module._channel_count = deserialized.channel_count;
        crate::types::OutputStruct::set_all_channels(&mut module.outputs, deserialized.channel_count);
    }

    #has_init_call
    Ok(std::sync::Arc::new(Box::new(sampleable)))
}
```

This approach:

- Still uses `Default::default()` for the inner module (so module structs still derive Default)
- Immediately overwrites `params`, `_channel_count`, and output channel counts
- Then calls `init()` (which reads from `self.params`)
- The params struct no longer needs `Default` — it's never default-constructed

**Wait — the inner module's Default still calls Default on the params struct.** So we can't remove `Default` from params yet without a bigger change.

**The real solution: Remove the `Default` impl from the `*Sampleable` wrapper and construct all fields explicitly.** The wrapper has known fields — we control its struct definition. The issue is the inner module struct, which has user-defined fields.

**Final approach — two-phase construction:**

We keep using `Default` on the inner module struct for now. But we make the params struct NOT need Default by doing this:

1. The inner module struct derives `Default` — but its `Default` impl needs all fields to impl `Default`
2. The params field won't impl `Default` anymore
3. So we need to use `MaybeUninit` or a wrapper for the params field in the Default impl

Actually, this is getting complicated. Let's use a simpler approach:

**Pragmatic approach: Add a `ModuleDefault` trait that constructs a module with dummy params, then immediately overwrite params.**

No — even simpler. Since the module struct needs Default for the wrapper, and the params struct is a field of the module struct, we need one of:

1. Keep `Default` on params (current state — all params optional)
2. Change how the module struct's Default works (complex proc macro change)
3. Skip Default on the wrapper entirely

**Option 3 is the right path.** The wrapper struct `*Sampleable` is fully generated by the proc macro. We know all its fields. We can construct it directly without needing `Default`:

```rust
fn #constructor_name(id: &String, sample_rate: f32, deserialized: crate::params::DeserializedParams) -> napi::Result<std::sync::Arc<Box<dyn crate::types::Sampleable>>> {
    let concrete_params = deserialized.params.into_any()
        .downcast::<#params_struct_name>()
        .map_err(|_| napi::Error::from_reason(
            format!("Failed to downcast params for module type {}", #module_name)
        ))?;

    // Construct the inner module. We can't use Default because params may not impl Default.
    // Instead, we need to construct it with all fields specified.
    // PROBLEM: We don't know what other fields the user struct has!
```

We DO know the fields — the proc macro has access to the struct definition via `ast.data`. We can iterate over fields and generate default initialization for everything except `params`, `outputs`, and `_channel_count`.

**This is the correct approach but requires significant proc macro work.** Let's plan it properly.

---

**REVISED PLAN — Phased approach:**

### Phase 1A: Merge construction + initial param application (no Default removal yet)

Change the constructor signature to accept `DeserializedParams`. The constructor still uses `Default::default()` internally but immediately overwrites params. This is a safe refactoring step that changes the API contract without changing behavior.

### Phase 1B: Remove `Default` from `*Sampleable` wrapper

The wrapper struct is fully generated — we know all its fields. Remove the `impl Default for #struct_name` and construct all wrapper fields explicitly in the constructor.

### Phase 1C: Remove `Default` from inner module structs + params structs

Update the proc macro to iterate over the inner module struct's fields and generate per-field initialization. For `params`, use the deserialized value. For `outputs`, use `Default::default()`. For `_channel_count`, use the deserialized value. For all other fields (phase accumulators, lookup tables, etc.), use `Default::default()` on a per-field basis.

This means each user-defined field on the module struct must impl `Default` individually, but the struct itself doesn't need `#[derive(Default)]`. The params field explicitly does NOT need Default.

### Phase 2: Per-module required/optional annotations

With the constructor no longer needing `Default` on params, we can now make some signal fields bare (required) and remove `#[serde(default)]` from them.

### Phase 3: TypeScript + validation changes

Use the schema `required` array. Structured error reporting from `deriveChannelCount`.

---

## Phase 1A: Merge construction + initial param application

### Task 1A.1: Change SampleableConstructor type alias

**Files:**

- Modify: `crates/modular_core/src/types.rs`

**Step 1: Update the type alias**

Find (around line 1036):

```rust
pub type SampleableConstructor = Box<dyn Fn(&String, f32) -> Result<Arc<Box<dyn Sampleable>>>>;
```

Replace with:

```rust
pub type SampleableConstructor = Box<dyn Fn(&String, f32, crate::params::DeserializedParams) -> Result<Arc<Box<dyn Sampleable>>>>;
```

**Step 2: Run cargo check**

Run: `cargo check -p modular_core 2>&1 | head -50`
Expected: Errors in generated code (proc macro) and `audio.rs`

---

### Task 1A.2: Update proc macro constructor

**Files:**

- Modify: `crates/modular_derive/src/module_attr.rs`

**Step 1: Update the constructor function signature and body**

Find the constructor function generation (around line 630). Replace:

```rust
fn #constructor_name(id: &String, sample_rate: f32) -> napi::Result<std::sync::Arc<Box<dyn crate::types::Sampleable>>> {
    let sampleable = #struct_name {
        id: id.clone(),
        sample_rate,
        ..#struct_name::default()
    };
    #has_init_call
    Ok(std::sync::Arc::new(Box::new(sampleable)))
}
```

With:

```rust
fn #constructor_name(id: &String, sample_rate: f32, deserialized: crate::params::DeserializedParams) -> napi::Result<std::sync::Arc<Box<dyn crate::types::Sampleable>>> {
    let concrete_params = deserialized.params.into_any()
        .downcast::<#params_struct_name>()
        .map_err(|_| napi::Error::from_reason(
            format!("Failed to downcast params for module type {}", #module_name)
        ))?;

    let sampleable = #struct_name {
        id: id.clone(),
        sample_rate,
        ..#struct_name::default()
    };

    // Apply typed params immediately (before init).
    // SAFETY: We just created sampleable, no one else has access yet.
    unsafe {
        let module = &mut *sampleable.module.get();
        module.params = *concrete_params;
        module._channel_count = deserialized.channel_count;
        crate::types::OutputStruct::set_all_channels(&mut module.outputs, deserialized.channel_count);
        let argument_spans = &mut *sampleable.argument_spans.get();
        *argument_spans = deserialized.argument_spans;
    }

    #has_init_call
    Ok(std::sync::Arc::new(Box::new(sampleable)))
}
```

**Step 2: Run cargo check**

Run: `cargo check -p modular_core 2>&1 | head -50`
Expected: modular_core should compile (the type alias change + constructor change should be consistent). Errors may appear in `modular` crate.

---

### Task 1A.3: Update audio.rs apply_patch

**Files:**

- Modify: `crates/modular/src/audio.rs`

**Step 1: Merge construction + param deserialization in apply_patch**

In `apply_patch` (around line 1060-1090), the current code:

1. Calls `constructor(&id, sample_rate)` to create module
2. Calls `deserialize_params(...)` to deserialize params
3. Pushes both to separate vectors in `PatchUpdate`

Change to:

1. Call `deserialize_params(...)` FIRST
2. Call `constructor(&id, sample_rate, deserialized)` to create module with params
3. Push only the module to `inserts` — no separate `param_updates` entry for new modules

Find the loop body in `apply_patch`:

```rust
for (id, module_state) in desired_modules {
    if let Some(constructor) = constructors.get(&module_state.module_type) {
        match constructor(&id, sample_rate) {
            Ok(module) => {
                update.inserts.push((id.clone(), module));
            }
            Err(err) => {
                return Err(napi::Error::from_reason(format!(
                    "Failed to create module {}: {}",
                    id, err
                )));
            }
        }
    } else {
        return Err(napi::Error::from_reason(format!(
            "{} is not a valid module type",
            module_state.module_type
        )));
    }

    // Also add param update with pre-deserialized params (cache-aware)
    let deserialized =
        crate::deserialize_params(&module_state.module_type, module_state.params, true)
            .map_err(|e| {
                napi::Error::from_reason(format!(
                    "Failed to deserialize params for {}: {}",
                    id, e
                ))
            })?;
    update.param_updates.push((id.clone(), deserialized));
}
```

Replace with:

```rust
for (id, module_state) in desired_modules {
    // Deserialize params FIRST (before construction)
    let deserialized =
        crate::deserialize_params(&module_state.module_type, module_state.params, true)
            .map_err(|e| {
                napi::Error::from_reason(format!(
                    "Failed to deserialize params for {}: {}",
                    id, e
                ))
            })?;

    if let Some(constructor) = constructors.get(&module_state.module_type) {
        match constructor(&id, sample_rate, deserialized) {
            Ok(module) => {
                update.inserts.push((id.clone(), module));
            }
            Err(err) => {
                return Err(napi::Error::from_reason(format!(
                    "Failed to create module {}: {}",
                    id, err
                )));
            }
        }
    } else {
        return Err(napi::Error::from_reason(format!(
            "{} is not a valid module type",
            module_state.module_type
        )));
    }
}
```

**Step 2: Update the audio thread's apply_patch_update**

In `apply_patch_update` (around line 1305), the audio thread currently:

1. Inserts new modules
2. Removes stale modules
3. Applies `param_updates` for ALL modules (new and existing)

After this change, newly inserted modules already have their params applied. But `param_updates` is still needed for modules that persist across patch updates (params changed but module is reused). We need to keep `param_updates` for the reuse case.

**Actually, looking at the current code more carefully:** `apply_patch` in audio.rs currently constructs ALL modules fresh on every patch update (there's a comment: "For now, we send all modules as param_updates and inserts"). Every module in the desired graph gets a new constructor call. So `param_updates` is always applying params to freshly constructed modules.

This means after our change, we can remove the `param_updates` loop for the initial apply case. But we must keep it for `SingleParamUpdate` (slider interactions).

**Simplest approach:** Since all modules are freshly constructed with params, remove the `param_updates` entries from `apply_patch`. The `param_updates` vector in `PatchUpdate` stays (used by future optimizations and slider path), but `apply_patch` no longer pushes to it.

The `apply_patch_update` on the audio thread still processes `param_updates` if any exist — this is fine, it's just a no-op now for the full patch update path.

**Step 3: Run cargo check**

Run: `cargo check -p modular 2>&1 | head -50`
Expected: Should compile. May have warnings about unused `param_updates`.

---

### Task 1A.4: Update set_module_param path

**Files:**

- Modify: `crates/modular/src/lib.rs` (the `set_module_param` N-API function)

The `set_module_param` path (for slider interactions) does NOT construct new modules — it only deserializes params and sends a `SingleParamUpdate` command. This path is unaffected by the constructor change and needs no modification.

Verify: Search for all uses of constructors in `lib.rs` and `audio.rs` to ensure they all pass `DeserializedParams`.

Run: `cargo check -p modular`
Expected: Clean compile.

---

### Task 1A.5: Run all tests

**Step 1: Run Rust tests**

Run: `cargo test -p modular_core && cargo test -p modular`
Expected: All 477+ tests pass.

**Step 2: Run TS tests**

Run: `yarn test:unit`
Expected: All 135 tests pass.

**Step 3: Commit**

```bash
git add -A && git commit -m "refactor: constructor takes DeserializedParams, merging construction with initial param application"
```

---

## Phase 1B: Remove Default from Sampleable wrapper

### Task 1B.1: Remove Default impl from generated wrapper

**Files:**

- Modify: `crates/modular_derive/src/module_attr.rs`

**Step 1: Remove the `impl Default for #struct_name` block**

Find (around line 470):

```rust
impl Default for #struct_name {
    fn default() -> Self {
        Self {
            id: String::new(),
            outputs: std::cell::UnsafeCell::new(Default::default()),
            module: std::cell::UnsafeCell::new(Default::default()),
            processed: core::sync::atomic::AtomicBool::new(false),
            sample_rate: 0.0,
            argument_spans: std::cell::UnsafeCell::new(std::collections::HashMap::new()),
        }
    }
}
```

Remove this entire block.

**Step 2: Update constructor to not use `..#struct_name::default()`**

The constructor from Task 1A.2 should already construct all fields explicitly. Verify it doesn't reference `#struct_name::default()`. If it still does, replace with explicit field construction:

```rust
fn #constructor_name(id: &String, sample_rate: f32, deserialized: crate::params::DeserializedParams) -> napi::Result<std::sync::Arc<Box<dyn crate::types::Sampleable>>> {
    let concrete_params = deserialized.params.into_any()
        .downcast::<#params_struct_name>()
        .map_err(|_| napi::Error::from_reason(
            format!("Failed to downcast params for module type {}", #module_name)
        ))?;

    // Construct inner module with default fields, then overwrite params
    let mut inner: #name #static_ty_generics = Default::default();
    inner.params = *concrete_params;
    inner._channel_count = deserialized.channel_count;
    crate::types::OutputStruct::set_all_channels(&mut inner.outputs, deserialized.channel_count);

    let sampleable = #struct_name {
        id: id.clone(),
        sample_rate,
        outputs: std::cell::UnsafeCell::new(Default::default()),
        module: std::cell::UnsafeCell::new(inner),
        processed: core::sync::atomic::AtomicBool::new(false),
        argument_spans: std::cell::UnsafeCell::new(deserialized.argument_spans),
    };

    #has_init_call
    Ok(std::sync::Arc::new(Box::new(sampleable)))
}
```

Note: This still requires `Default` on the inner module struct (e.g., `SineOscillator`) because of `Default::default()`. But the wrapper struct no longer needs Default.

**Step 3: Run cargo check and tests**

Run: `cargo check -p modular_core && cargo test -p modular_core && cargo test -p modular`
Expected: All tests pass.

**Step 4: Commit**

```bash
git add -A && git commit -m "refactor: remove Default from Sampleable wrapper, construct all fields explicitly"
```

---

## Phase 1C: Remove Default from inner module structs

### Task 1C.1: Generate per-field initialization in proc macro

**Files:**

- Modify: `crates/modular_derive/src/module_attr.rs`

**Step 1: Update constructor to initialize inner module fields individually**

Instead of `let mut inner: #name = Default::default()`, iterate over the struct fields in the proc macro and generate initialization for each:

- `params` field: use `*concrete_params`
- `outputs` field: use `Default::default()`
- `_channel_count` field: use `deserialized.channel_count`
- All other fields: use `Default::default()` (field type must impl Default individually)

```rust
// In the proc macro, extract field names and types from the struct:
let fields = match &ast.data {
    Data::Struct(data) => match &data.fields {
        Fields::Named(fields) => &fields.named,
        _ => panic!("expected named fields"),
    },
    _ => panic!("expected struct"),
};

// Generate field initializers
let field_inits: Vec<TokenStream2> = fields.iter().map(|f| {
    let field_name = f.ident.as_ref().unwrap();
    let field_name_str = field_name.to_string();
    match field_name_str.as_str() {
        "params" => quote! { params: *concrete_params },
        "outputs" => quote! { outputs: Default::default() },
        "_channel_count" => quote! { _channel_count: deserialized.channel_count },
        _ => quote! { #field_name: Default::default() },
    }
}).collect();
```

Then in the constructor:

```rust
let inner = #name #static_ty_generics {
    #(#field_inits),*
};
crate::types::OutputStruct::set_all_channels(&mut inner.outputs, deserialized.channel_count);
```

Wait — `_channel_count` is injected by the proc macro itself (it adds the field to the struct). So we need to include it in the field iteration. The injected field will appear in `fields` after the injection step. Make sure the field iteration happens AFTER the `_channel_count` field injection.

Currently, the `_channel_count` injection happens at the top of `module_impl()` (line ~190), before `impl_module_macro_attr()` is called. So when `impl_module_macro_attr()` iterates the struct fields, `_channel_count` WILL be present. Good.

**Step 2: Remove `#[derive(Default)]` from all module structs**

For each module struct (e.g., `SineOscillator`, `PlaitsOscillator`, etc.), remove `#[derive(Default)]` from the struct. The params struct keeps `#[derive(Default)]` for now (Phase 2 will remove it).

Actually — if the proc macro generates per-field initialization, module structs no longer need `Default` at all. But their non-params, non-outputs fields (like `channels: [ChannelState; 16]`) still need `Default` on their types. The struct itself doesn't need `#[derive(Default)]`.

**Step 3: Run cargo check**

Run: `cargo check -p modular_core`
Expected: Should compile — every field type individually impls Default, even if the module struct as a whole doesn't derive it.

**Step 4: Run all tests and commit**

Run: `cargo test -p modular_core && cargo test -p modular && yarn test:unit`
Expected: All tests pass.

```bash
git add -A && git commit -m "refactor: remove Default from module structs, construct via per-field initialization"
```

---

## Phase 2: Per-module Required/Optional Annotations

With the constructor no longer needing `Default` on params structs, we can now:

1. Remove `#[derive(Default)]` from params structs
2. Remove `#[serde(default)]` from fields that should be required
3. Make some signal fields bare `PolySignal` (required) instead of `Option<PolySignal>`

### Task 2.1: Remove Default from params structs

**Files:**

- Modify: ALL ~45 DSP module params structs

**Step 1: For each params struct, remove `#[derive(Default)]` or manual `Default` impl**

Remove `Default` from the derive list. Example:

```rust
// Before
#[derive(Clone, Deserialize, Default, JsonSchema, Connect, ChannelCount, SignalParams)]

// After
#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
```

**Exception:** `ClockParams` has a manual `impl Default` — remove it too.

**Step 2: Verify each params struct can still be deserialized from `{}`**

Since all fields currently have `#[serde(default)]`, deserialization from `{}` should still work even without `#[derive(Default)]` on the struct — serde uses per-field defaults, not the struct-level Default.

Run: `cargo test -p modular_core`
Expected: All tests pass.

**Step 3: Commit**

```bash
git add -A && git commit -m "refactor: remove Default from all params structs"
```

---

### Task 2.2: Make semantically required signal fields bare PolySignal

**Files:**

- Modify: DSP module params structs (selective — only modules with truly required inputs)

This is the core semantic change. For each module, decide which signal inputs are truly required vs optional. The decision criteria:

- **Required** (bare `PolySignal`): The module is meaningless without this input. E.g., a filter's `input`, a VCA's `input`, a quantizer's `input`.
- **Optional** (`Option<PolySignal>`): The module has a sensible default behavior when this input is not connected. E.g., an oscillator's `freq` (defaults to C4), a filter's `resonance` (defaults to 0).

**Step 1: Identify required vs optional for each module**

Analyze each module's `process`/`update` method. If a field uses `value_or(ch, some_default)`, it has a sensible default → keep as `Option<PolySignal>`. If a field is essential to the module's operation, make it bare `PolySignal`.

**Representative examples:**

| Module         | Field        | Decision               | Reason                             |
| -------------- | ------------ | ---------------------- | ---------------------------------- |
| SineOscillator | `freq`       | Optional               | Defaults to 0V (C4)                |
| LowpassFilter  | `input`      | **Required**           | Filter with no input is useless    |
| LowpassFilter  | `cutoff`     | Optional               | Defaults to 0V (C4)                |
| LowpassFilter  | `resonance`  | Optional               | Defaults to 0                      |
| Remap          | `input`      | **Required**           | Remapper with no input is useless  |
| Remap          | `in_min` etc | Optional               | Have `#[signal(default = ...)]`    |
| VCA            | `input`      | **Required**           | VCA with no input is useless       |
| Mix            | `inputs`     | Keep `Vec<PolySignal>` | Vec is naturally optional (empty)  |
| Delay          | `input`      | **Required**           | Delay with no input is useless     |
| Quantizer      | `input`      | **Required**           | Quantizer with no input is useless |

**Step 2: For each required field, change from `Option<PolySignal>` to bare `PolySignal`**

Example for lowpass filter:

```rust
// Before
#[serde(default)]
input: Option<PolySignal>,

// After  (no #[serde(default)], no Option)
input: PolySignal,
```

**Step 3: Update the process/update methods**

For required fields, change from `self.params.input.value_or(ch, 0.0)` to `self.params.input.get_value(ch)` (direct access, no Option unwrapping).

For `PolySignal` (not Option), the methods are:

- `signal.get_value(ch)` — get value at channel
- `signal.channel_count()` — number of channels

**Step 4: Update ChannelCount / PolySignalFields derive**

The `PolySignalFields` derive needs to handle both bare `PolySignal` (returns `&PolySignal`) and `Option<PolySignal>` (returns the inner ref if Some). Check `channel_count.rs` to ensure it handles bare `PolySignal` fields.

Currently `channel_count.rs` was updated to handle `Option<PolySignal>` — it may need to also handle bare `PolySignal` (it likely already did before our Phase 4 migration, but verify).

**Step 5: Update Connect derive**

The `Connect` derive needs to handle bare `PolySignal` fields (resolve cables). Check `connect.rs` — bare `PolySignal` should already work via the blanket impl.

**Step 6: Run tests**

Run: `cargo test -p modular_core && cargo test -p modular`
Expected: All tests pass.

**Step 7: Commit**

```bash
git add -A && git commit -m "feat: make semantically required signal inputs bare PolySignal (not Option)"
```

---

### Task 2.3: Add per-field serde defaults for non-signal fields with sensible defaults

**Files:**

- Modify: DSP module params structs (selective)

For non-signal fields that have sensible defaults, change from `#[serde(default)]` (which uses `Default::default()`) to `#[serde(default = "specific_fn")]`:

```rust
// Before (uses 0.0 from Default::default())
#[serde(default)]
mix: f32,

// After (explicit default function)
#[serde(default = "default_mix")]
mix: f32,

fn default_mix() -> f32 { 0.5 }
```

This is a refinement — the `required` array in the JSON schema only includes fields that have NEITHER `Option<T>` NOR `#[serde(default)]`. So this step doesn't change the required/optional semantics, just makes defaults explicit.

---

## Phase 3: TypeScript Changes

### Task 3.1: Use schema `required` array in typescriptLibGen.ts

**Files:**

- Modify: `src/main/dsl/typescriptLibGen.ts`

**Step 1: Update positional arg rendering**

Find (around line 1188):

```typescript
const optional = arg.optional ? '?' : '';
args.push(`${arg.name}${optional}: ${type}`);
```

Replace with logic that checks the schema's `required` array:

```typescript
const schemaRequired: string[] = moduleSchema.paramsSchema.required || [];
const isRequired = schemaRequired.includes(arg.name);
```

For positional args, implement the spec's trailing-optional logic:

- If this optional arg is TRAILING (all subsequent positional args are also optional), use `?`
- If this optional arg is NON-TRAILING (a required arg follows), use `| undefined` instead of `?`

```typescript
// Determine trailing optional status for each positional arg
const isTrailingOptional = (index: number): boolean => {
    // An arg is "trailing optional" if it's optional AND all args after it are also optional
    if (isRequired) return false;
    for (let j = index + 1; j < positionalArgs.length; j++) {
        if (schemaRequired.includes(positionalArgs[j].name)) return false;
    }
    return true;
};

// For each positional arg:
if (isRequired) {
    args.push(`${arg.name}: ${type}`);
} else if (isTrailingOptional(i)) {
    args.push(`${arg.name}?: ${type}`);
} else {
    args.push(`${arg.name}: ${type} | undefined`);
}
```

**Step 2: Update config object rendering**

Find (around line 1234):

```typescript
configProps.push(`${key}?: ${type}`);
```

Replace with:

```typescript
const isConfigRequired = schemaRequired.includes(key);
const optionalMark = isConfigRequired ? '' : '?';
configProps.push(`${key}${optionalMark}: ${type}`);
```

**Step 3: Update config object optionality**

Find (around line 1261):

```typescript
args.push(`config?: ${configType}`);
```

Replace with:

```typescript
const hasRequiredConfig = configProps.some((p) => !p.includes('?:'));
args.push(`config${hasRequiredConfig ? '' : '?'}: ${configType}`);
```

Wait — need to be more careful. `configProps` contains the rendered strings. Better to track required-ness separately:

```typescript
const requiredConfigKeys: string[] = [];
for (const key of allParamKeys) {
    if (!positionalKeys.has(key)) {
        const isConfigRequired = schemaRequired.includes(key);
        if (isConfigRequired) requiredConfigKeys.push(key);
        // ... render as before
    }
}

const configOptional = requiredConfigKeys.length === 0 ? '?' : '';
args.push(`config${configOptional}: ${configType}`);
```

**Step 4: Run tests**

Run: `yarn test:unit`
Expected: Tests pass (some may need updating for new required signatures).

Run: `yarn generate-lib && yarn typecheck`
Expected: Generated types compile. May need to update DSL example code.

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: TypeScript lib uses schema required array for param optionality"
```

---

### Task 3.2: Structured error reporting from deriveChannelCount

**Files:**

- Modify: `crates/modular/src/lib.rs` (the `derive_channel_count` N-API function)
- Modify: `src/main/dsl/factories.ts` (surface errors as DSL diagnostics)

**Step 1: Change `derive_channel_count` return type**

Current:

```rust
#[napi]
pub fn derive_channel_count(module_type: String, params: serde_json::Value) -> Option<u32> {
    deserialize_params(&module_type, params, true)
        .ok()
        .map(|d| d.channel_count as u32)
}
```

Change to return a result object:

```rust
#[derive(Serialize)]
#[napi(object)]
pub struct DeriveChannelCountResult {
    pub channel_count: Option<u32>,
    pub errors: Vec<DeriveChannelCountError>,
}

#[derive(Serialize)]
#[napi(object)]
pub struct DeriveChannelCountError {
    pub message: String,
    pub params: Vec<String>,
}

#[napi]
pub fn derive_channel_count(module_type: String, params: serde_json::Value) -> DeriveChannelCountResult {
    match deserialize_params(&module_type, params, true) {
        Ok(d) => DeriveChannelCountResult {
            channel_count: Some(d.channel_count as u32),
            errors: vec![],
        },
        Err(e) => {
            // Parse serde error to extract missing field names
            let error_msg = e.to_string();
            let missing_params = parse_missing_params(&error_msg);
            DeriveChannelCountResult {
                channel_count: None,
                errors: vec![DeriveChannelCountError {
                    message: format!("{} is missing required params", module_type),
                    params: missing_params,
                }],
            }
        }
    }
}
```

**Step 2: Update factories.ts to surface errors**

In `factories.ts`, find the `deriveChannelCount` call (around line 286):

```typescript
const derivedChannels = deriveChannelCount(
    schema.name,
    node.getParamsSnapshot(),
);
if (derivedChannels !== null) {
    node._setDerivedChannelCount(derivedChannels);
}
```

Update to handle the new return type:

```typescript
const result = deriveChannelCount(schema.name, node.getParamsSnapshot());
if (result.channelCount !== null && result.channelCount !== undefined) {
    node._setDerivedChannelCount(result.channelCount);
}
if (result.errors && result.errors.length > 0) {
    // Surface as DSL diagnostic at the call site
    for (const error of result.errors) {
        const msg = `${error.message}: [${error.params.join(', ')}]`;
        // TODO: Integrate with Monaco diagnostic system
        console.warn(msg);
    }
}
```

**Step 3: Run all tests**

Run: `cargo test -p modular && yarn test:unit`
Expected: All tests pass. May need to update TS tests for new return type.

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: deriveChannelCount returns structured errors for missing required params"
```

---

## Phase 3B: Clean up stale code

### Task 3B.1: Remove stale references

**Files:**

- Modify: `src/main/dsl/typescriptLibGen.ts` — remove `arg.optional` reference (line 1188)
- Verify: No remaining references to `Signal::Disconnected` in Rust or TypeScript

**Step 1: Clean up**

Run: `rg "Disconnected\|disconnected\|arg\.optional" --type-add 'source:*.{rs,ts,tsx}' --type source`
Fix any remaining references.

**Step 2: Run all tests**

Run: `cargo test -p modular_core && cargo test -p modular && yarn test:unit`

**Step 3: Final commit**

```bash
git add -A && git commit -m "chore: clean up stale Disconnected and optional references"
```

---

## Summary of phases and commits

| Phase | Description                                 | Commit                                           |
| ----- | ------------------------------------------- | ------------------------------------------------ |
| 1A    | Constructor accepts DeserializedParams      | `refactor: constructor takes DeserializedParams` |
| 1B    | Remove Default from Sampleable wrapper      | `refactor: remove Default from wrapper`          |
| 1C    | Remove Default from inner module structs    | `refactor: per-field initialization`             |
| 2.1   | Remove Default from params structs          | `refactor: remove Default from params structs`   |
| 2.2   | Make required signal fields bare PolySignal | `feat: required signal inputs`                   |
| 2.3   | Explicit serde default functions            | `refactor: explicit serde defaults`              |
| 3.1   | TypeScript uses schema required array       | `feat: TS required params`                       |
| 3.2   | Structured deriveChannelCount errors        | `feat: structured errors`                        |
| 3B    | Clean up stale code                         | `chore: cleanup`                                 |

**Testing strategy:** Run `cargo test -p modular_core && cargo test -p modular && yarn test:unit` after each phase. The E2E tests (`yarn test:e2e`) should be run at the end.
