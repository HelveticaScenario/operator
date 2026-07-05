# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

"Operator" — a real-time modular synthesizer desktop app with a JavaScript DSL for live-coding audio patches. Electron + React frontend, Rust DSP engine exposed via N-API.

## Commands

### Setup

Repo uses git submodules for vendored Rust DSP crates. Required after clone:

```bash
git submodule update --init --recursive
```

Skipping this produces `failed to load manifest for workspace member` on the first cargo/yarn build.

```bash
yarn install               # Install dependencies
yarn start                 # Build native Rust module + launch Electron app (watches Rust changes)
yarn build-native          # Build native Rust module only
yarn generate-lib          # Regenerate DSL TypeScript types after Rust N-API type changes
yarn typecheck             # TypeScript type checking
yarn lint                  # oxlint on src (also runs on pre-commit via husky)
yarn lint:fix              # oxlint --fix
```

### Testing

| Change type            | Test command                                     |
| ---------------------- | ------------------------------------------------ |
| Rust DSP modules       | `cargo test -p modular_core` or `yarn test:rust` |
| DSL factories/executor | `yarn test:unit`                                 |
| N-API bindings         | `yarn test:unit`                                 |
| Renderer UI/UX         | `yarn test:e2e`                                  |
| Visual snapshots       | `yarn test:e2e:update`                           |
| Everything             | `yarn test:all`                                  |

E2E tests require the webpack build to exist — run `yarn start` once first.

## Architecture

### Data Flow

1. **DSL execution** — JavaScript code runs in a `node:vm` context with an execution timeout in `src/main/dsl/executor.ts`, producing a `PatchGraph` JSON structure.
2. **IPC transport** — PatchGraph sent from renderer to main process over Electron IPC (channels defined in `src/shared/ipcTypes.ts`), which calls `synthesizer.updatePatch(graph)`.
3. **Validation** — Rust validates the graph on the main thread (`crates/modular/src/validation.rs`).
4. **Audio thread** — Applied via lock-free command queue (rtrb SPSC) to the audio thread (`crates/modular/src/audio.rs`). Modules instantiated and processed here.
5. **Scope data** — Audio thread writes to ring buffer; renderer polls via `get_scopes()` N-API call; Monaco editor draws oscilloscope overlays.

### Crate Structure

- **`crates/modular_core/`** — Pure Rust DSP library: module trait (`Sampleable`), types (`types.rs`), patch graph (`patch.rs`), all DSP modules (`dsp/`), pattern system.
- **`crates/modular/`** — N-API bindings (`lib.rs`), audio callback (`audio.rs`), validation (`validation.rs`), MIDI input (`midi.rs`), command queue (`commands.rs`).
- **`crates/modular_derive/`** — Proc macros for the module output system.
- **`crates/mi-plaits-dsp-rs/`** — Mutable Instruments Plaits DSP port (git submodule, third-party).
- **`crates/rust-music-theory/`** — Vendored fork (git submodule) for note/scale theory helpers. Treat as third-party.

### Frontend Structure

- **`src/main/`** — Electron main process (`main.ts`), DSL runtime (`dsl/executor.ts`, `dsl/factories.ts`, `dsl/GraphBuilder.ts`).
- **`src/renderer/`** — React app (`App.tsx`), Monaco editor (`components/MonacoPatchEditor.tsx`, `components/monaco/`), UI components.
- **`src/preload/`** — Electron context isolation bridge.
- **`src/shared/`** — Shared IPC types.

## Critical Safety Rules

### Thread Safety (violating these causes UB or crashes)

- **NEVER** call `Sampleable` trait methods from the main thread.
- **NEVER** clone module `Arc`s and send them across threads.
- **NEVER** access `Patch::sampleables` from outside `AudioProcessor`.
- **ALWAYS** use the command queue for main-to-audio communication.

### Real-Time Audio Thread

- **No heap allocation on the audio thread.** No `Vec::new`, `Box::new`, `String`, `HashMap`, `.clone()` of heap types in `process` or anything it calls.
- **Allocate in `init` or param deserialization** (both run on the main thread). `process` should only operate on pre-allocated memory.
- Store initialized data on the params struct with `#[serde(skip)] #[schemars(skip)]` for fields hidden from serialization.
- Once deserialization is complete, treat the params object as immutable — `process` reads but never mutates it.

### Module Lifecycle Hooks (`init` vs `on_patch_update` vs `update`)

A module has three places to put logic. Pick by **how often the inputs change** and **what's resolved yet**. Lifecycle order on every patch update is:

```
init (construct, main thread) → transfer_state_from → connect → on_patch_update → … → update (per sample)
```

| Hook | Opt-in | When/thread | Put here |
| ---- | ------ | ----------- | -------- |
| `fn init(&mut self, sample_rate: f32)` | `has_init` flag | Once at construction, main thread, **may allocate** | Heap allocation; one-time seeding of runtime state (phases, RNG); **sample-rate-only** derived constants. |
| `fn on_patch_update(&mut self)` | `patch_update` flag + `impl PatchUpdateHandler` | After every patch update, once all modules are connected, audio thread, **no alloc** | **Param-derived** constant caches; anything needing the resolved graph (connected modules, `Wav::sample_rate()` — resolves in `connect()`). No `sample_rate` arg, so capture it in `init` if needed. |
| `fn update(&mut self, sample_rate: f32)` | — | Every sample, audio thread, hot path, **no alloc** | Only work that reads a live signal (`PolySignal::get_value`/`value_or` — may be a cable varying per sample) or evolving state. Never recompute lifetime-constant work here — hoist it. |

**Why param-derived caches must NOT go in `init`** (the clobber rule): on a patch update every module is reconstructed, then `transfer_state_from` does `std::mem::swap` on the **entire `state` struct** to preserve runtime continuity — overwriting anything `init` computed into `state`. So a value derived from a non-signal param (recomputed when that param changes) must be set in `on_patch_update`, which runs *after* the swap and reads the current `self.params`.

**Why sample-rate-only caches are fine in `init`:** a transfer only happens between two modules at the same engine rate (a rate change rebuilds the processor — it is not a state transfer), so the swapped-in old value equals what `init` would compute. `init` also runs **before `connect()`**, so it cannot read connection/topology state.

Examples: `supersaw` — `init` seeds phases + `inv_sample_rate` (sr-only); `on_patch_update` computes `voice_t`/`gain`/`voices` (param-derived). `sampler` — `rate_ratio` is in `on_patch_update` because `wav.sample_rate()` only resolves in `connect()`. Test harnesses that build a module directly must mirror this: call `init` then `on_patch_update` before driving `update`.

### Detecting audio-thread allocations (dev-only)

`yarn build-native-alloc` builds the rust code with a runtime allocation detector compiled in (`--features=alloc-detector` on `crates/modular`). It installs a `#[global_allocator]` that flags any heap allocation/deallocation made on the audio thread and writes a warning to **stderr** — the `rust`-labelled stream in the `concurrently` output — naming the offending module, e.g.:

```
[alloc-detector] AUDIO-THREAD ALLOC in module "osc_3" — 48 bytes (×127 since last report). Move allocation out of process()/update() into init()/on_patch_update() (see CLAUDE.md lifecycle rules).
```

Use it to verify the "no heap allocation on the audio thread" rule above. Notes:

- **Opt-in only.** Plain `yarn build-native` never compiles the detector in (zero cost, byte-identical binary). The detector installs a process-wide global allocator — never enable the feature in a shipped build.
- **Auto-attribution.** Module profiling is force-enabled so attribution works with the editor profiler closed. Allocations on the audio thread but outside any module/scope frame report as `"<unknown>"`.
- **Detect-and-flag, never fail.** The real `System` allocation always runs first; the detector only records. All formatting/logging happens on a background thread, never the audio thread. Dropped/dealloc events and running totals are summarized periodically on the same `[alloc-detector]` stderr stream.

## Conventions

### Voltage Standards

- **V/Oct pitch**: 1V/oct, 0V = C4 (~261.63 Hz).
- **Gates/triggers**: `GATE_HIGH_VOLTAGE` (5V) high, `GATE_LOW_VOLTAGE` (0V) low. Constants in `crates/modular_core/src/dsp/utils/` (module directory).
- **Gate detection**: Schmitt trigger with hysteresis — high threshold 1.0V, low threshold 0.1V. Use `SchmittTrigger::default()`.
- **Output attenuation**: `AUDIO_OUTPUT_ATTENUATION` in `crates/modular/src/audio.rs`.

### Adding/Changing Module Params

1. Update param structs + DSP in `crates/modular_core/src/dsp/**/*.rs`.
2. Wire schema/validators in category modules (e.g., `oscillators/mod.rs` via `install_constructors` / `install_param_validators`).
3. Rebuild N-API (`yarn build-native`) to refresh `crates/modular/index.d.ts`.
4. Adjust DSL factories in `src/main/dsl/factories.ts` if needed.

### Reserved Output Names

When adding methods to `ModuleOutput`, `ModuleOutputWithRange`, `BaseCollection`, `Collection`, or `CollectionWithRange`, add the method name to `RESERVED_OUTPUT_NAMES` in `crates/reserved_output_names.rs`. This is the single source of truth shared by the Rust proc-macro and TypeScript DSL via NAPI.

### Code Organization

- Break files over ~400 lines into smaller domain-specific modules.
- Patch graphs are the contract — update Rust types in `modular_core::types`, not TypeScript.
- Prefer Electron APIs over web/React APIs when either could solve a task.

### Comments

**The codebase is a present-tense artifact. It must read as though it was written this way from the start.** This is a hard invariant, the single most important rule here — never relaxed, no exceptions:

- **No history, anywhere.** Never reference commits, PRs, tickets, or past states — no "used to", "now", "previously", "formerly", "renamed", "instead of", "we changed", "this was". History lives in git, and only in git.
- **No roads not taken.** Never mention alternatives considered, decisions made, approaches rejected, or bugs once present. The current code is the only thing that exists; comments describe what must be true, never what once was. (A regression test states the invariant it guards, not the bug it fixed.)

Within that invariant, comment well:

- **Only the non-obvious.** Explain a constraint, invariant, unit, or consequence a reader needs to use the code correctly. If the code is clear, leave it uncommented.
- **Short and complete.** One or two full sentences. Cut jargon and detail the reader doesn't need.
- **No redundant restatement.** If a comment re-says the line below it, delete it.
- **Don't over-comment.** Match the surrounding file's density. Spend comments on safety/thread-safety/real-time rules and boundary semantics (IPC, DSL, Electron quirks).
- **Leave nothing dead.** No commented-out code; no unowned `TODO`/`FIXME`.

Models to emulate: `crates/modular_core/src/types.rs` (safety invariants), `src/main/bufferSwitch.ts` (boundary semantics).

## Tooling

- **Node 24.12.0 / Yarn 4.12.0** pinned via Volta.
- **Rust edition 2024**.
- **Husky pre-commit hooks** run Prettier on `*.{ts,tsx,js,jsx,mjs,json,css,md}` and `cargo fmt` on Rust files.
- **Vitest** for JS/TS tests, **Playwright** for E2E, **cargo test** for Rust.
