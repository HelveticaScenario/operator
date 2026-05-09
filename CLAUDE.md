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

1. **DSL execution** — JavaScript code runs via `new Function(...)` in `src/main/dsl/executor.ts`, producing a `PatchGraph` JSON structure.
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

## Tooling

- **Node 24.12.0 / Yarn 4.12.0** pinned via Volta.
- **Rust edition 2024**.
- **Husky pre-commit hooks** run Prettier on `*.{ts,tsx,js,jsx,mjs,json,css,md}` and `cargo fmt` on Rust files.
- **Vitest** for JS/TS tests, **Playwright** for E2E, **cargo test** for Rust.
