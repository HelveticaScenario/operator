# Required Module Params & Signal Refactor — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove implicit optionality of all module params by making serde the single source of truth for required vs optional, removing Signal::Disconnected, switching PolySignal to ArrayVec, and propagating required/optional through the JSON schema and TypeScript types.

**Architecture:** Signal loses Disconnected variant; param optionality uses Option<T>. PolySignal uses ArrayVec<Signal, 16>. Module constructors receive typed params at creation time. TypeScript uses the schemars `required` array.

**Tech Stack:** Rust (serde, schemars, arrayvec, napi), TypeScript (Monaco, Electron IPC)

**Worktree:** `~/.config/superpowers/worktrees/modular/required-params`

**Baseline:** 458 tests passing, 1 pre-existing failure (test_overlapping_names_panics)

---

## Key Decisions Record

| Decision                     | Choice                                          | Rationale                                                                                   |
| ---------------------------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------- | ---------- |
| Signal::Disconnected         | Remove entirely                                 | Option<Signal> is more precise; Disconnected conflated param-level and value-level concepts |
| PolySignal storage           | ArrayVec<Signal, 16>                            | Stack-allocated, no heap dealloc on audio thread, variable length                           |
| PolySignal min channels      | At least 1                                      | Optionality lives at Option<PolySignal> level                                               |
| MonoSignal behavior          | Still wraps PolySignal, sums channels           | Preserves poly-to-mono downmix capability                                                   |
| Normalling API               | Extension trait on Option<Signal>               | `.value_or(default)` pattern, zero-cost, ergonomic                                          |
| Optionality source of truth  | serde (via Option<T> and #[serde(default)])     | Single source, flows through schemars → JSON schema → TypeScript                            |
| TS required detection        | schemars `required` array in JSON schema        | Built-in behavior, no custom metadata needed                                                |
| has-default fields in TS     | Optional (not required)                         | User shouldn't be forced to provide params with sensible defaults                           |
| Positional arg ordering      | Not enforced at compile time                    | TS lib generator handles it: trailing optionals get `?`, non-trailing get `                 | undefined` |
| Constructor signature        | new(id, sample_rate, params)                    | Params available before init(); no Default needed                                           |
| Param updates                | Swap params, send old to garbage_tx, no re-init | Same as today but old params are garbage-collected off audio thread                         |
| Validation + deserialization | Merged into one step                            | Deserialization IS validation; errors returned as structured data from Rust                 |
| Error format                 | Batch all missing params in one error           | "$module is missing required params: ['a', 'b']"                                            |
| Migration                    | Big bang, all at once                           | No compat layer, clean break                                                                |
| Backward compat for patches  | None                                            | Breaking change accepted                                                                    |

---

## Phase 1: Signal Type Changes

### Task 1.1: Remove Signal::Disconnected variant

**Files:**

- Modify: `crates/modular_core/src/types.rs`

**Step 1: Remove the Disconnected variant from the Signal enum**

In the `Signal` enum (~line 551), remove `Disconnected` and remove `#[default]` / `Default` derive:

```rust
// Before
#[derive(Clone, Debug, Default)]
pub enum Signal {
    Volts(f32),
    Cable { module: String, module_ptr: ..., port: String, channel: usize },
    #[default]
    Disconnected,
}

// After
#[derive(Clone, Debug)]
pub enum Signal {
    Volts(f32),
    Cable { module: String, module_ptr: ..., port: String, channel: usize },
}
```

**Step 2: Update Signal::get_value()**

Remove the `Disconnected => 0.0` arm (~line 717):

```rust
// Before
pub fn get_value(&self) -> f32 {
    match self {
        Signal::Volts(v) => *v,
        Signal::Cable { ... } => { ... },
        Signal::Disconnected => 0.0,
    }
}

// After
pub fn get_value(&self) -> f32 {
    match self {
        Signal::Volts(v) => *v,
        Signal::Cable { ... } => { ... },
    }
}
```

**Step 3: Remove Signal::get_value_or()**

Remove the `get_value_or` method entirely (~line 721-727). Will be replaced by SignalExt trait.

**Step 4: Update Signal serializer**

Remove the `Disconnected` arm from the custom `Serialize` impl (~line 648):

```rust
// Remove:
Signal::Disconnected => {
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry("type", "disconnected")?;
    map.end()
}
```

**Step 5: Update Signal deserializer**

Remove `Disconnected` from the `SignalTagged` enum used in deserialization (~line 597):

```rust
// Remove the Disconnected variant from SignalTagged:
// Disconnected,
```

**Step 6: Update Signal JsonSchema**

Remove `Disconnected` from `SignalTaggedSchema` (~line 675):

```rust
// Remove:
// Disconnected,
```

**Step 7: Update Connect impl for Signal**

Remove the `Disconnected` arm from `Connect for Signal` (~line 360):

```rust
// Before
fn connect(&mut self, patch: &Patch) {
    match self {
        Signal::Cable { module, module_ptr, .. } => { ... }
        Signal::Disconnected | Signal::Volts(_) => {}
    }
}

// After
fn connect(&mut self, patch: &Patch) {
    match self {
        Signal::Cable { module, module_ptr, .. } => { ... }
        Signal::Volts(_) => {}
    }
}
```

**Step 8: Compile check**

Run: `cargo check -p modular_core 2>&1 | head -60`
Expected: Many errors in poly.rs and DSP modules (these will be fixed in later phases).

**Step 9: Commit**

```
feat: remove Signal::Disconnected variant
```

---

### Task 1.2: Add SignalExt trait for Option<Signal>

**Files:**

- Modify: `crates/modular_core/src/types.rs`

**Step 1: Add the SignalExt trait**

After the Signal impl block, add:

```rust
/// Extension trait for normalling pattern on optional signals.
pub trait SignalExt {
    /// Returns the signal's value, or `default` if None.
    fn value_or(&self, default: f32) -> f32;

    /// Returns the signal's value, or calls `f` if None.
    fn value_or_else(&self, f: impl FnOnce() -> f32) -> f32;

    /// Returns the signal's value, or 0.0 if None.
    fn value_or_zero(&self) -> f32;
}

impl SignalExt for Option<Signal> {
    fn value_or(&self, default: f32) -> f32 {
        match self {
            Some(s) => s.get_value(),
            None => default,
        }
    }

    fn value_or_else(&self, f: impl FnOnce() -> f32) -> f32 {
        match self {
            Some(s) => s.get_value(),
            None => f(),
        }
    }

    fn value_or_zero(&self) -> f32 {
        match self {
            Some(s) => s.get_value(),
            None => 0.0,
        }
    }
}
```

**Step 2: Export SignalExt from the crate**

Add `SignalExt` to the public exports in `crates/modular_core/src/lib.rs`.

**Step 3: Commit**

```
feat: add SignalExt trait for Option<Signal> normalling
```

---

## Phase 2: PolySignal & MonoSignal Changes

### Task 2.1: Refactor PolySignal to use ArrayVec

**Files:**

- Modify: `crates/modular_core/src/poly.rs`

**Step 1: Replace PolySignal internal storage**

```rust
// Before
#[derive(Clone, Debug, Default)]
pub struct PolySignal {
    signals: [Signal; PORT_MAX_CHANNELS],
    channels: usize,
}

// After
use arrayvec::ArrayVec;

#[derive(Clone, Debug)]
pub struct PolySignal {
    channels: ArrayVec<Signal, PORT_MAX_CHANNELS>,
}
```

- Remove `Default` derive.
- `channels.len()` replaces the old `channels` field.
- A PolySignal always has ≥1 channel (optionality at param level uses `Option<PolySignal>`).

**Step 2: Update all PolySignal methods**

- `channels()` → `self.channels.len()`
- `is_disconnected()` → remove (use `Option::is_none()` at call sites)
- `is_monophonic()` → `self.channels.len() == 1`
- `is_polyphonic()` → `self.channels.len() > 1`
- `get(ch)` → `self.channels.get(ch)` (returns `Option<&Signal>`)
- `get_cycling(ch)` → `self.channels[ch % self.channels.len()]`
- `get_value(ch)` → `self.channels[ch % self.channels.len()].get_value()` (cycling)
- `get_value_or(ch, default)` → remove (use Option<PolySignal> + PolySignalExt at call sites)
- `max_channels(&[&PolySignal])` → same logic using `.len()`
- `mono()` / `poly()` constructors → create from single Signal or slice of Signals

**Step 3: Add PolySignalExt trait for Option<PolySignal>**

```rust
pub trait PolySignalExt {
    fn value_or(&self, ch: usize, default: f32) -> f32;
    fn value_or_zero(&self, ch: usize) -> f32;
    fn channel_count(&self) -> usize;
}

impl PolySignalExt for Option<PolySignal> {
    fn value_or(&self, ch: usize, default: f32) -> f32 {
        match self {
            Some(ps) => ps.get_value(ch),
            None => default,
        }
    }

    fn value_or_zero(&self, ch: usize) -> f32 {
        self.value_or(ch, 0.0)
    }

    fn channel_count(&self) -> usize {
        match self {
            Some(ps) => ps.channels(),
            None => 0,
        }
    }
}
```

**Step 4: Update PolySignal Serialize/Deserialize**

Serialization: serialize as array of active signals (same as before but using ArrayVec).

Deserialization: accept single Signal (1-channel) or array of Signals (N-channel). No longer accept `{ "type": "disconnected" }` as a valid single value.

**Step 5: Update PolySignal JsonSchema**

Schema is `anyOf [Signal, Signal[]]` — no disconnected variant.

**Step 6: Update Connect impl for PolySignal**

Iterate `self.channels.iter_mut()` and call `signal.connect(patch)`.

**Step 7: Commit**

```
feat: refactor PolySignal to ArrayVec storage, remove Default
```

---

### Task 2.2: Update MonoSignal

**Files:**

- Modify: `crates/modular_core/src/poly.rs`

**Step 1: Remove Default from MonoSignal**

MonoSignal wraps PolySignal; since PolySignal no longer implements Default, MonoSignal can't either.

**Step 2: Update MonoSignal methods**

- `is_disconnected()` → remove
- `get_value()` → sums all channels in the inner ArrayVec
- `get_value_or()` → remove (use Option<MonoSignal> + MonoSignalExt)

**Step 3: Add MonoSignalExt trait for Option<MonoSignal>**

```rust
pub trait MonoSignalExt {
    fn value_or(&self, default: f32) -> f32;
    fn value_or_zero(&self) -> f32;
}

impl MonoSignalExt for Option<MonoSignal> {
    fn value_or(&self, default: f32) -> f32 {
        match self {
            Some(ms) => ms.get_value(),
            None => default,
        }
    }

    fn value_or_zero(&self) -> f32 {
        self.value_or(0.0)
    }
}
```

**Step 4: Commit**

```
feat: update MonoSignal to remove Default and Disconnected handling
```

---

## Phase 3: Proc Macro Changes

### Task 3.1: Remove ? from args syntax

**Files:**

- Modify: `crates/modular_derive/src/module_attr.rs`

**Step 1: Update ArgAttr parsing**

Remove the `optional` field and `?` peek logic from `ArgAttr::parse()`:

```rust
struct ArgAttr {
    name: Ident,
    // removed: optional: bool,
}

impl syn::parse::Parse for ArgAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        // removed: ? peek/parse
        Ok(ArgAttr { name })
    }
}
```

**Step 2: Update PositionalArg emission**

Remove the `optional` field from the emitted `PositionalArg` structs. Optionality is now determined by the field's type in the params struct.

**Step 3: Commit**

```
refactor: remove ? from module macro args syntax
```

---

### Task 3.2: Constructor takes typed params

**Files:**

- Modify: `crates/modular_derive/src/module_attr.rs`
- Modify: `crates/modular_core/src/types.rs` (SampleableConstructor type alias)

**Step 1: Change SampleableConstructor signature**

```rust
// Before
pub type SampleableConstructor = Box<dyn Fn(&String, f32) -> Result<Arc<Box<dyn Sampleable>>>>;

// After
pub type SampleableConstructor = Box<dyn Fn(&String, f32, Box<dyn CloneableParams>) -> Result<Arc<Box<dyn Sampleable>>>>;
```

**Step 2: Update generated constructor**

```rust
fn #constructor_name(id: &String, sample_rate: f32, params: Box<dyn CloneableParams>) -> napi::Result<Arc<Box<dyn Sampleable>>> {
    let concrete_params = params.into_any()
        .downcast::<#params_struct_name>()
        .map_err(|_| napi::Error::from_reason("param type mismatch"))?;

    let mut module = #inner_module_name {
        params: *concrete_params,
        ..Default::default()  // Default still needed for outputs, channel state
    };
    module._channel_count = /* derived from params */;

    #has_init_call  // init(sample_rate) reads self.params
    Ok(std::sync::Arc::new(Box::new(#wrapper_struct { ... })))
}
```

Wait — the spec says the **inner** module struct should NOT derive Default (since params no longer does). But the outputs and channel state still need defaults. Let me reconsider.

Actually, the module struct still derives Default, but the params field specifically gets assigned from the constructor argument. The `..Default::default()` works for everything except `params`. So we construct piecemeal:

```rust
let mut module = #inner_module_name {
    outputs: Default::default(),
    params: *concrete_params,
    // channels: Default::default(), etc. — if applicable
};
```

Actually the `#[module]` macro already knows the struct's fields. It generates the wrapper, not the inner struct. The inner struct construction needs to change.

**This task is complex enough to warrant careful implementation. See the detailed approach in Phase 3 notes below.**

**Step 3: Update apply_patch in audio.rs**

The `apply_patch` flow changes: params are deserialized first, then passed to the constructor. The constructor creates the module with typed params.

**Step 4: Commit**

```
feat: module constructor takes typed params at creation time
```

---

### Task 3.3: Update Connect derive for Option-wrapped signals

**Files:**

- Modify: `crates/modular_derive/src/connect.rs`

The current impl calls `Connect::connect(&mut self.field, patch)` for every field. Since `Connect` already has a blanket impl for `Option<T: Connect>`, this should "just work" — `Option<Signal>`, `Option<PolySignal>`, `Option<MonoSignal>` will skip `None` and resolve `Some`.

Verify this works by checking the existing `Option<T>` impl in types.rs.

**Step 1: Verify existing Option<T> Connect impl**

```rust
// Already exists in types.rs:
impl<T: Connect> Connect for Option<T> {
    fn connect(&mut self, patch: &Patch) {
        if let Some(inner) = self {
            inner.connect(patch);
        }
    }
}
```

This means no changes needed to the Connect derive macro itself.

**Step 2: Commit (if any changes needed)**

---

### Task 3.4: Update ChannelCount derive for Option-wrapped signals

**Files:**

- Modify: `crates/modular_derive/src/channel_count.rs`
- Modify: `crates/modular_derive/src/utils.rs`

**Step 1: Handle Option<PolySignal> fields**

The `is_poly_signal_type` helper currently matches `PolySignal` as the last path segment. Update it to also detect `Option<PolySignal>`.

For `Option<PolySignal>` fields, the generated code should use the `PolySignalExt::channel_count()` method (which returns 0 for None).

```rust
// For PolySignal fields: &self.field_name
// For Option<PolySignal> fields: self.field_name.as_ref() (with different trait method)
```

**Step 2: Update PolySignalFields trait**

The trait may need to return something that handles both `PolySignal` and `Option<PolySignal>`. Or the channel_count function can handle them separately.

**Step 3: Commit**

```
feat: update ChannelCount derive for Option<PolySignal>
```

---

## Phase 4: Migrate All DSP Modules

### Task 4.1: Migrate all module param structs

**All files in `crates/modular_core/src/dsp/`**

For each module params struct:

1. Remove `Default` from derives (keep `Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams`)
2. Remove `#[serde(default)]` from the struct level
3. For each field:
    - **Required input** (must always be connected): bare type `PolySignal` / `MonoSignal`. No annotation.
    - **Optional input** (can be disconnected): wrap in `Option<PolySignal>` / `Option<MonoSignal>`. The `#[signal(...)]` attribute stays.
    - **Has a sensible default**: add `#[serde(default = "fn")]` at field level.
    - **Runtime state** (skip): `#[serde(skip, default = "init_fn")]`
4. Remove `?` from `#[module(..., args(...))]`
5. Update `update()` / `process()`: replace `signal.get_value_or(ch, x)` calls:
    - If field is now `Option<PolySignal>`: use `self.params.field.value_or(ch, x)` (via PolySignalExt)
    - If field is now bare `PolySignal` (required): use `self.params.field.get_value(ch)`
6. Replace `is_disconnected()` checks with `Option::is_none()` checks

### Module-by-module migration guide

Each module needs individual analysis of which params are required vs optional. The decision is based on:

- If a module is useless without the input → required (bare PolySignal)
- If the module has meaningful behavior when an input is absent → optional (Option<PolySignal>)
- If a numeric param has a meaningful default → `#[serde(default = "fn")]`

**This is the largest single task. Each of the ~45 module files needs individual migration.**

---

## Phase 5: N-API & Validation

### Task 5.1: Update deriveChannelCount return type

**Files:**

- Modify: `crates/modular/src/lib.rs`
- Modify: `crates/modular/src/validation.rs`

deriveChannelCount now returns structured result:

- On success: `{ channelCount: number }`
- On failure: `{ errors: Array<{ message: string, params: string[] }> }`

### Task 5.2: Update apply_patch to pass params to constructor

**Files:**

- Modify: `crates/modular/src/audio.rs`

The construction flow changes:

1. Deserialize params first (already done)
2. Pass deserialized params to the constructor
3. Constructor creates module with typed params and calls init()

---

## Phase 6: TypeScript Changes

### Task 6.1: Update paramsSchema.ts signal detection

Remove `disconnected` tag check from `isSignalParamSchema`.

### Task 6.2: Update factories.ts

Remove signal pre-initialization logic. Surface structured validation errors.

### Task 6.3: Update GraphBuilder.ts

Remove signal pre-initialization in `addModule()` and `replaceSignals()` (stop converting null/undefined to disconnected).

### Task 6.4: Update typescriptLibGen.ts

Use schemars `required` array to determine TypeScript param optionality instead of marking everything optional.

### Task 6.5: Update schemaTypeResolver.ts

Remove disconnected Signal variant handling if any.

---

## Phase 7: Tests

### Task 7.1: Update Rust unit tests

- `types_tests.rs`: Update signal tests (remove disconnected)
- `poly.rs` tests: Update for ArrayVec, remove disconnected tests
- `dsp_fresh_tests.rs`: Update param JSON in integration tests

### Task 7.2: Update TypeScript tests

Update any unit tests that reference disconnected signals.

### Task 7.3: Run full test suite

Run: `cargo test -p modular_core && yarn test:unit`

---

## Execution Order

1. Phase 1 (Signal) → Phase 2 (PolySignal/MonoSignal) → Phase 3 (Proc macros) → Phase 4 (DSP modules) → Phase 5 (N-API) → Phase 6 (TypeScript) → Phase 7 (Tests)

Each phase builds on the previous. Cannot parallelize across phases.

Within Phase 4, all modules must be migrated together (big bang per spec).
