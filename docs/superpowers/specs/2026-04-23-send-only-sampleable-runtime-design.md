# Send-Only Sampleable Runtime Design

## Overview

Replace fake shared ownership in module graph with single-owner runtime model. `Sampleable` becomes `Send` but not `Sync`, patch/runtime own modules directly as `Box<dyn Sampleable>`, and hot connection paths cache non-owning raw pointers resolved during `connect()`. True cross-thread shared data such as `Arc<WavData>` remain shared. This aligns type model with actual runtime behavior, removes unnecessary `Arc` and `Weak` traffic from audio graph, and clears path for moving more runtime compilation work off audio thread.

## Goals

- Make `Sampleable` ownership model honest: transferable across threads, not concurrently shared
- Remove `Sync` requirement from `Sampleable` trait and generated wrappers
- Replace module-graph `Arc` and `Weak` usage that exists only to satisfy fake shared ownership
- Preserve current patch update semantics, including `transfer_state_from()` continuity
- Keep hot signal and buffer reads cheap after migration
- Support further removal of audio-thread allocations by compiling more runtime state on main thread

## Non-Goals

- Full arena / slot-based runtime rewrite in this pass
- Removing `Arc` from truly shared assets or UI-facing shared state
- Reworking every scheduler structure at once
- Replacing all string module ids with numeric handles everywhere
- Changing DSP semantics or connection behavior visible to patch authors

## Background

Today module graph uses shared-ownership types even though module execution follows a single-owner phase model.

- `Sampleable` currently requires `Send + Sync` in `crates/modular_core/src/types.rs`
- `Patch.sampleables` stores `Arc<Box<dyn Sampleable>>` in `crates/modular_core/src/patch.rs`
- `Signal::Cable` stores `Weak<Box<dyn Sampleable>>` in `crates/modular_core/src/types.rs`
- `Buffer` caches a strong module `Arc` only to call `update()` on demand in `crates/modular_core/src/types.rs`
- `SeqSourceConnections` stores `Weak<Box<dyn Sampleable>>` in `crates/modular_core/src/dsp/seq/seq_value.rs`
- message listeners also use weak module references in `crates/modular_core/src/patch.rs`

This model implies concurrent shared access that engine does not actually support. Real rule is stronger and simpler:

- main thread may create unpublished modules and runtimes
- ownership may transfer to audio thread
- once published to audio thread, main thread must not touch that runtime or its modules
- module graph mutation and audio processing happen in disjoint phases

Type model should express that rule directly.

## Approach

Adopt hybrid ownership model:

- use owned `Box<dyn Sampleable>` for module graph and queued module transfers
- use raw non-owning cached pointers for hot intra-runtime module connections
- keep `Arc` only for data that is truly shared across threads or subsystems

This keeps hot path close to current API shape while removing fake shared ownership from module graph.

## Why This Approach

- Smallest honest change from current architecture
- Avoids per-sample hash lookup for `Signal` and `Buffer`
- Avoids much larger arena-handle refactor right now
- Preserves current `connect()` model, where connection caches are rebuilt after graph changes
- Supports later main-thread compilation of runtime metadata without requiring full graph rewrite first

## Architecture

### Trait and Wrapper Semantics

`Sampleable` changes from:

```rust
pub trait Sampleable: MessageHandler + Send + Sync
```

to:

```rust
pub trait Sampleable: MessageHandler + Send
```

Generated wrappers in `crates/modular_derive/src/module_attr.rs` must stop claiming `Sync`. Any remaining `unsafe impl Send` blocks must be reviewed and kept only where required by interior mutability patterns that still satisfy single-thread execution contract.

### Runtime Ownership

Patch and runtime own modules directly.

Conceptually:

```rust
type SampleableMap = HashMap<String, Box<dyn Sampleable>>;

struct Patch {
    sampleables: SampleableMap,
    wav_data: HashMap<String, Arc<WavData>>,
    message_listeners: HashMap<MessageTag, Vec<String>>,
}
```

Queued updates and garbage handoff also move owned boxes rather than shared `Arc`s.

Conceptually:

```rust
pub struct PatchUpdate {
    pub inserts: Vec<(String, Box<dyn Sampleable>)>,
    // ...
}

pub enum GraphCommand {
    SingleModuleUpdate { module_id: String, module: Box<dyn Sampleable> },
    // ...
}

pub enum GarbageItem {
    Module(Box<dyn Sampleable>),
    // ...
}
```

### Cached Connection Model

#### Signal

`Signal::Cable` keeps module id for identity / serialization and caches raw pointer for hot reads.

Conceptually:

```rust
pub enum Signal {
    Volts(f32),
    Cable {
        module: String,
        resolved: Option<NonNull<dyn Sampleable>>,
        port: String,
        channel: usize,
    },
}
```

`connect()` resolves `module` through `patch.sampleables` and stores `NonNull::from(module.as_ref())`. `get_value()` dereferences cached pointer only on audio thread during processing phase.

#### Buffer

`Buffer` no longer keeps strong module ownership. It caches:

- source module id and port name for reconnect
- raw pointer to source module for `ensure_source_updated()`
- shared `Arc<BufferData>` only for actual buffer payload

This keeps demand-driven buffer refresh cheap without forcing shared module ownership.

#### Seq Source Connections

`SeqSourceConnections` replaces `Vec<Weak<Box<dyn Sampleable>>>` with `Vec<Option<NonNull<dyn Sampleable>>>`. `connect()` refreshes cached pointers in parallel with sorted source module ids. Query path reads cached pointers directly.

### Message Listeners

Message listeners no longer need weak refs. Store module ids only.

Conceptually:

```rust
struct MessageListenerRef {
    id: String,
}
```

Dispatch path looks up listener id in `patch.sampleables` at message-send time. This removes another fake shared-ownership seam and naturally prunes removed modules.

### True Shared Data

Keep `Arc` where ownership is genuinely shared across threads or long-lived subsystems.

Examples:

- `Arc<WavData>` shared between main-thread cache and audio runtime
- UI meters / shared state guarded by atomics or mutexes
- scope collection / recording handles / transport meter
- any immutable asset cache intentionally retained by multiple owners

Rule: keep `Arc` only when two live owners may legitimately coexist. Do not use it as connection glue inside audio module graph.

## Raw Pointer Safety Contract

Raw pointer caching is allowed only under existing phase-separation rule.

Required invariants:

1. Production runtime dereferences cached module pointers only on audio thread. Test-only single-thread runtimes such as `Patch::from_graph(...)` may also dereference them under the same non-overlap phase rule.
2. Graph mutation and `connect()` happen in command-processing phase, never during `process()` / `update()` traversal.
3. Any module insert, replace, remap, or removal is followed by reconnect before next processing pass uses cached pointers.
4. Cached pointers are cleared or overwritten during reconnect if source module no longer exists.
5. Main thread never dereferences cached runtime pointers after ownership transfer.

This contract must be documented adjacent to each raw-pointer cache site.

## Patch Update Flow

Patch updates keep current two-part semantic model:

- main thread constructs fresh modules with fresh params
- audio thread transfers runtime state from old module to new module

After migration, audio-thread apply still performs state transfer, but no longer manages shared module ownership.

Conceptually:

1. take owned `Box<dyn Sampleable>` inserts from queued update
2. remove old owned module if present
3. call `transfer_state_from(old.as_ref())`
4. queue old owned module for main-thread drop
5. insert new owned module into patch
6. rebuild listeners and reconnect all modules

This preserves existing state continuity semantics while matching real ownership.

For current `Signal`, `Buffer`, and seq source caches, precomputed bindings against unpublished new runtime would be sound across `transfer_state_from()` because state transfer mutates new modules in place rather than replacing them with different module objects. Even so, published-runtime `connect()` should remain on audio thread for now. This keeps one canonical binding point after state transfer and runtime install, and preserves room for future optimizations or cache types that may depend on final live-runtime state rather than only new-runtime object identity.

## Runtime Compilation Boundary

This ownership cleanup should also tighten boundary between main-thread preparation and audio-thread apply.

Main thread should increasingly prepare:

- scheduler analysis
- debug snapshot inputs
- region execution plan
- module order
- block module specs
- preallocated region output buffers sized for host callback

Audio thread should increasingly limit itself to:

- state transfer
- owned runtime swap / field replacement
- reconnect
- pointer cache refresh
- old runtime garbage handoff

This change is not required to land as one patch, but design should move in this direction because it removes more audio-thread allocation and rebuild work.

## Error Handling

- Missing source module during `connect()` clears cached pointer and behaves like disconnected input
- `Signal::get_value()` on unresolved cable returns `0.0`, matching current fallback
- `Buffer` unresolved source becomes disconnected buffer state
- message dispatch skips listener ids no longer present in patch
- patch apply continues to queue removed modules for main-thread destruction

No new user-facing error surface is required for this migration.

## Testing Strategy

Add or update tests for:

- `Sampleable` requires `Send` but not `Sync`
- generated wrappers no longer require or claim `Sync`
- `Signal` reconnect refreshes cached pointer after module replacement and remap
- `Signal` missing source clears pointer and returns `0.0`
- `Buffer.ensure_source_updated()` still triggers source update after reconnect
- seq source cache refresh works after patch updates
- message listener dispatch still reaches active modules and ignores removed ones
- `SingleModuleUpdate` preserves state continuity with owned modules
- patch update / clear path hands old modules to garbage queue without audio-thread drop

If runtime compilation work moves off audio thread in same effort, add tests that verify new runtime metadata is precomputed before enqueue.

## Migration Plan

Implement in small steps to keep system working:

1. Replace message listener weak refs with id-based lookup.
2. Convert `Signal` cable cache from `Weak` to raw pointer.
3. Convert `Buffer` cached source module from strong `Arc` to raw pointer.
4. Convert seq source caches from `Weak` to raw pointer.
5. Change module ownership types from `Arc<Box<dyn Sampleable>>` to `Box<dyn Sampleable>` across `Patch`, commands, and garbage queue.
6. Drop `Sync` from `Sampleable` and wrapper impls.
7. Expand main-thread runtime preparation to remove more audio-thread rebuild and allocation work.

Each step should land with focused tests because connection bugs here will be subtle and timing-sensitive.

## Alternatives Considered

### Keep `Arc` and only remove `Sync`

Rejected.

This would reduce some trait dishonesty but leave fake shared ownership embedded in patch graph, commands, listeners, and caches. It would also preserve too much architectural pressure toward concurrent access patterns the engine does not support.

### Full Arena / Slot Runtime Rewrite

Rejected for now.

Stable slot handles with generations would be clean long-term, but would require much larger API churn because hot call sites such as `Signal::get_value()` would need runtime context or equivalent handle lookup machinery. Current migration should stay smaller and preserve hot-path API shape.

## Open Questions

None for this design phase. Raw-pointer hybrid model is selected for current migration.
