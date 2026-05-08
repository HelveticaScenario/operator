# operator-zed — handoff

Prototype native Rust shell that replaces Operator's Electron host with a binary built on top of [Zed's](https://github.com/zed-industries/zed) crates. The goal is feature parity with the existing Operator UI: editor, file explorer, sliders bound to `$slider()`, control panel, audio engine, and inline oscilloscope overlays in the editor — but driven by gpui + Zed's `editor` crate rather than React + Monaco.

This repo is the existing `~/dev/modular` monorepo. The new app lives at `apps/operator-zed/`. Zed is vendored at `vendor/zed/` (git submodule, pinned).

## Architecture decisions (locked)

- **Repo location**: `apps/operator-zed/` inside the modular monorepo. Cargo workspace member; `vendor/zed/` is `exclude`'d from the workspace so Zed's own workspace stays self-contained and we depend on its crates via path.
- **License**: AGPL-3.0. Operator is already AGPL; Zed's `editor`/`language`/`theme`/etc. are GPL-3.0-or-later — compatible.
- **JS runtime**: `deno_core` (V8). Replaces `new Function()` from `src/main/dsl/executor.ts`.
- **DSL bridge**: keep existing JS factories/`GraphBuilder`, esbuild-bundle them, run in V8, return graph through one Rust op.
- **Span analysis**: Rust-side `oxc_parser` replaces ts-morph (`src/main/dsl/argumentSpanAnalyzer.ts`).
- **Audio reuse**: lift cpal/midir/link/params_cache/validation out of the cdylib `crates/modular` into a new `crates/modular_host` consumed by both Electron and operator-zed. **Not done yet** — the prototype currently has its own minimal cpal driver. As of 2026-05-08, that driver can either play the original 440 Hz sine or, with `OPERATOR_ZED_PATCH_TEST=1`, drive a hand-crafted `Patch::from_graph(...)` (one `$sine` -> `ROOT_OUTPUT` `$signal`) sample-by-sample. The audio loop integration shape is therefore validated against `modular_core` independently of the JS path.
- **modular_core**: linked as a regular path dep with `napi-derive` left in. The `#[napi]` attributes expand into static items that are inert when the binary isn't loaded by Node. Feature-gating napi was attempted and reverted — `modular_derive` proc macros emit unqualified `napi::` paths that don't resolve through a stub module.
- **Zed pinned at**: `7ce845210d3af82a57a7518e0abe8c167d60cc6a` (master at the time of this handoff).

## Plan: 7 milestone steps

Each step is intended to leave the binary in a runnable state. As of 2026-05-08, Steps 0–6 have prototype-level coverage. Step 6's inline-block-decoration variant is the only feature gap; the data path runs through a panel today.

### Step 0 — submodule + workspace plumbing &nbsp;✅

- `vendor/zed` git submodule
- `apps/operator-zed/{Cargo.toml,src/main.rs}`
- Workspace member in root `Cargo.toml` + `exclude = ["vendor/zed"]`
- `rust-toolchain.toml` channel `1.92` (matches Zed's)
- **Verify**: `cargo check -p operator-zed`

### Step 1 — gpui hello window &nbsp;✅

- Path-dep on `gpui` only
- Pattern lifted from `vendor/zed/crates/gpui/examples/hello_world.rs`

### Step 2 — Editor view loading a file &nbsp;✅

- Path-deps: `editor`, `multi_buffer`, `language`, `text` (transitive), `rope` (transitive), `buffer_diff` (transitive), `project` (transitive), `fs` (transitive), `theme`, `settings`, `assets`, `ui`
- Boot: `Application::new().with_assets(Assets).run(|cx| { settings::init(cx); theme::init(theme::LoadThemes::JustBase, cx); ... })`
- Editor created via `Editor::for_buffer(buffer, None, window, cx)` with `Buffer::local(initial_text, cx)`
- File path passed via argv[1]; falls back to a placeholder string

### Step 3 — DSL execution + audio &nbsp;✅

End-to-end: a `.modular` source file is parsed by the bundled JS DSL, executed in V8, and the resulting `PatchGraph` materializes into a real `Patch` that drives a cpal output stream. cmd-S re-runs the same path; the file explorer's click handler does too. `--emit-graph FILE` runs the DSL headless and prints the JSON envelope on stdout.

- **modular_core**: path-linked.
- **audio.rs**: cpal callback owns a `Patch`; a crossbeam_channel `Sender<Patch>` lets the main thread hot-swap. Falls back to a hardcoded 440 Hz sine until the first patch arrives.
- **dsl_runtime.rs**: `deno_core 0.400` `JsRuntime` with a custom extension exposing `op_modz_derive_channel_count`, `op_modz_reserved_output_names`, and `op_modz_log`. The bundled JS lives at `$OUT_DIR/dsl_runtime.js` (built by `build.rs` -> `dsl/build.mjs`).
- **dsl/entry.ts**: bundle entry that wraps `executePatchScript` and routes `console.*` through `op_modz_log` so `--emit-graph` stdout stays clean JSON.
- **dsl/modular_core_shim.ts**: replaces `@modular/core` so the bundle doesn't pull `crates/modular/index.js` (the N-API addon entry, uses `node:module`).
- **dsl/analyze_source_stub.ts**: replaces `src/main/dsl/analyzeSource` so the bundle doesn't pull ts-morph (~14 MB TS compiler). Returns empty registries; argument-span highlighting is disabled until the Rust-side `op_argument_spans` lands.
- **dsl/build.mjs**: esbuild API driver — CLI `--alias` doesn't match the relative `./analyzeSource` import inside executor.ts, so a path-resolve plugin redirects it explicitly.
- **build.rs**: invokes `dsl/build.mjs` to produce `$OUT_DIR/dsl_runtime.js`. Falls back to a placeholder bundle if `node_modules/` is absent so the binary still builds in clean checkouts.

**Cmd-S handler**: `editor::init(cx)` + Zed's default-macos keymap loaded via `KeymapFile::load_asset_allow_partial_failure`; the Modz-namespaced `RunDsl` action runs the DSL, sends the resulting `Patch` into the audio thread, and updates the shared `DslState` so the controls + scopes panels re-render.

**`--emit-graph FILE`**: prints the `{ ok, value | error }` envelope verbatim, exits 0 on success / 1 on DSL error.

**Carry-over follow-ups (not blocking the prototype):**

1. **Lift `crates/modular_host`**. `crates/modular/src/{audio,midi,link,params_cache,validation}.rs` still live inside the cdylib crate. operator-zed currently has its own minimal cpal driver and a small inline copy of the params deserializer entry-point; the production audio engine (BPM, MIDI, Link, etc.) is unused. ~1 day of refactor.
2. **`op_argument_spans` (oxc_parser)** — replaces the analyzeSource stub so argument-span highlighting works in the Zed shell.
3. **`op_load_wav` / `op_workspace_root`** — needed once `$wavs()` is exercised in real DSL.
4. **Patch param mismatch**: `dsl::sanitize_graph_for_modular_core` strips `tempoSet` before deserialization. If more napi-only fields appear, extend the sanitizer or push the change into modular_core.

### Step 4 — File explorer &nbsp;✅

- `apps/operator-zed/src/file_explorer.rs` — gpui `uniform_list` over the parent dir of the CLI path (or `$PWD`). `.modular` files sort first and render in yellow. Hidden files filtered. Click swaps the EditorView buffer in place via `EditorView::open_file`, which also re-runs the DSL.
- Currently shallow (no directory recursion). A simple toggle on dir entries could be added later.

### Step 5 — Sliders / control panel &nbsp;✅

- `apps/operator-zed/src/controls.rs` — one row per `$slider()` declaration. Label, current value, range, and `−`/`+` buttons.
- `apps/operator-zed/src/dsl_state.rs::bump_slider` mutates the cached graph JSON (`modules[__slider_<label>].params.source = newValue`), rebuilds a `Patch` via `dsl::build_patch` (no JS), and pushes it to the audio thread. ~5% of range per click; clamped to [min, max].
- The graph is cached at JS-execution time so slider drags don't pay the V8 round-trip cost.
- TODO: a true draggable slider widget (drag-to-set) on top of the buttons. Pure UI work.

### Step 6 — Scopes &nbsp;✅ (panel) / ⏳ (inline blocks)

The data path lands in this prototype:

- `dsl_state.rs::ScopeTarget` — `{ label, module_id, port_name, channel, range, samples: Arc<Mutex<VecDeque<f32>>> }`. Audio thread pushes one sample/frame; UI thread reads on each render. Capacity ~250 ms at 48 kHz.
- `audio.rs` — cpal callback iterates the shared `Vec<ScopeTarget>` per audio frame, calls `patch.sampleables[id].get_poly_sample(port)?.get(channel)`, and pushes into the matching ring.
- `dsl.rs::parse_scopes` — walks the JS-emitted `graph.scopes` array (camelCase keys) and builds `ScopeTarget`s.
- `apps/operator-zed/src/scopes.rs::ScopesView` — renders an 80-wide unicode-block waveform per channel; a `cx.spawn` timer drives a 30 Hz repaint.

What's still in HANDOFF Step 6 (the _inline_ version):

- Lift the renderer up into `editor::display_map::block_map::BlockProperties` + `RenderBlock`. Entry point: `Editor::insert_blocks(blocks, autoscroll, cx)` at `vendor/zed/crates/editor/src/editor.rs:20536`. `BlockProperties` definition at `vendor/zed/crates/editor/src/display_map/block_map.rs:224`, `RenderBlock` typedef at `:101`.
- Anchor each `.scope()` call site via the DSL's source-location map (already produced by `executePatchScript`); insert one block with `placement: BlockPlacement::Below(anchor)`, `height: Some(8)`, `style: BlockStyle::Flex`, and a `RenderBlock` closure that draws the live waveform straight from the same ring buffer the panel reads today.
- On DSL re-exec: diff scope sites and call `replace_blocks` / `insert_blocks` / `remove_blocks` accordingly.
- **Open question**: does `BlockPlacement::Below` survive edits above the anchor? Run a small sanity test before building everything on top of it.

## Computer-use MCP integration (drivable app)

The computer-use MCP requires the binary to be a properly-registered macOS `.app` bundle. A bare Rust binary is rejected with `not_installed`. Several non-obvious requirements:

1. **Spotlight indexing must be on**. Default macOS dev environments often have it disabled. To enable:

    ```
    sudo mdutil -i on /
    sudo mdutil -i on /System/Volumes/Data
    ```

    Without this, `kMDItemCFBundleIdentifier` is null and the MCP rejects the app.

2. **The bundle must contain Xcode-style build metadata** in `Info.plist` — `DTCompiler`, `DTSDKBuild`, `DTSDKName`, `DTXcode`, `DTXcodeBuild`, plus `NSPrincipalClass = NSApplication`. A minimal Info.plist with only `CFBundle*` keys is rejected as `not_installed` even after Spotlight indexing. The exact heuristic isn't documented; assume the MCP filters on these fields.

3. **The bundle must live under `/Applications/`**. Install with `cp -R` (not symlink — symlinks are also rejected).

4. **Tier classification**. The MCP grants apps at one of three tiers (`read` / `click` / `full`). Some category strings cause it to classify the app as a browser → `read` only. The Info.plist in `apps/operator-zed/macos/Info.plist` is intentionally pared down to avoid this — no `LSApplicationCategoryType`, no browser-suggestive keys.

5. **First launch matters**. Call `lsregister -f` and `mdimport` after installing. Then launch via `open /Applications/Modz.app` once before requesting MCP access.

The current bundle id is **`dev.danlewis.modz`** (display name "Modz"). Keep this stable — once a bundle id has been granted by the MCP at full tier, re-running the build doesn't lose the grant. Renaming the bundle id forces a fresh classification run.

### How to build + install the bundle

```
./apps/operator-zed/macos/build-app.sh --install        # debug build + install
./apps/operator-zed/macos/build-app.sh --release --install
```

Then from any session:

```
open /Applications/Modz.app --args apps/operator-zed/examples/hello.modular
```

Or via the MCP:

```
mcp__computer-use__request_access apps=["dev.danlewis.modz"]
mcp__computer-use__open_application app="dev.danlewis.modz"
```

## Files map

```
apps/operator-zed/
├── Cargo.toml                          # workspace member, deps on Zed crates + deno_core
├── build.rs                            # invokes dsl/build.mjs, embeds the JS bundle
├── src/
│   ├── main.rs                         # CLI parse + gpui Application + window
│   ├── audio.rs                        # cpal driver: hot-swappable Patch + scope ring fill
│   ├── controls.rs                     # ControlsView (slider rows, +/- buttons)
│   ├── dsl.rs                          # run(source) -> { graph, sliders, scopes, patch }
│   ├── dsl_runtime.rs                  # deno_core JsRuntime + ops
│   ├── dsl_state.rs                    # shared DslState (graph cache, sliders, scope rings)
│   ├── editor_view.rs                  # Editor + cmd-S handler + open_file
│   ├── file_explorer.rs                # FileExplorer panel (uniform_list)
│   ├── root_view.rs                    # explorer | (editor + scopes + controls)
│   └── scopes.rs                       # ScopesView (ASCII waveform per ring)
├── dsl/
│   ├── entry.ts                        # bundle entry: wraps executePatchScript
│   ├── modular_core_shim.ts            # @modular/core replacement
│   ├── analyze_source_stub.ts          # ts-morph-free analyzeSource
│   └── build.mjs                       # esbuild API driver
├── examples/
│   ├── hello.modular                   # original sample
│   └── scope.modular                   # sliders + scopes
├── macos/
│   ├── Info.plist                      # Xcode-style metadata for MCP
│   └── build-app.sh                    # build + bundle + install script
└── HANDOFF.md                          # this file
```

Adjacent files in the monorepo:

```
Cargo.toml                              # added apps/operator-zed to members, exclude vendor/zed
Cargo.lock                              # massive churn from Zed's deps
rust-toolchain.toml                     # 1.92 to match vendor/zed
vendor/zed                              # git submodule pinned at 7ce8452
.gitmodules
```

## Build prerequisites

- macOS with **full Xcode** installed and selected via `sudo xcode-select -s /Applications/Xcode.app/Contents/Developer`. Command Line Tools alone are insufficient — gpui's Metal shader compile needs `xcrun metal` from full Xcode.
- Rust 1.92 (auto-installed by `rust-toolchain.toml`).
- Spotlight indexing enabled (see above) **only if** you want to use the computer-use MCP.

First build is heavy — the dep tree pulls livekit's `webrtc-sys` which downloads a ~160 MB binary blob from `github.com/livekit/rust-sdks/releases/download/webrtc-b99fd2c-6/webrtc-mac-arm64-release.zip`. Occasionally returns 502; retry resolves. Disk: budget ~10 GB for `target/` and ~1 GB for the submodule.

## Known issues / things to watch

- **No syntax highlighting**. The buffer is `Buffer::local(text, cx)` with no language. Future: register a JavaScript / `.modular` language with the `LanguageRegistry`, or hand-roll a small one.
- **Argument-span highlighting disabled**. `dsl/analyze_source_stub.ts` returns empty registries. Replace with a real `op_argument_spans` (oxc_parser) when needed.
- **`webrtc-sys` is dragged in transitively** by `editor` → `workspace` → `call` → `livekit_client` → `webrtc-sys`. There is no clean way to disable it short of patching the `call` crate. The runtime overhead is zero (we never instantiate any livekit objects); it just bloats the binary and adds compile time.
- **`@modular/core` shim incompletes**. `deriveChannelCount` and `getReservedOutputNames` proxy to Rust ops; other exports (e.g. `Synthesizer`, `validatePatchGraph`, `getMiniLeafSpans`) aren't shimmed. The current DSL doesn't reach those paths, but exotic scripts could.
- **`tempoSet` strip**. `dsl::sanitize_graph_for_modular_core` removes `ROOT_CLOCK.params.tempoSet` because modular_core's `ClockParams` uses `deny_unknown_fields`. If GraphBuilder.ts grows more napi-only fields, extend the sanitizer.
- **modular's `target/` was wiped** during a prior session to free disk space. Existing Electron build artifacts rebuild on first `yarn build`.
- **Bundled DSL is ~600 KB** of JS, embedded via `include_str!` so no external file is needed at runtime.

## Bringing this prototype up after a fresh checkout

```sh
git checkout zed-prototype
git submodule update --init --recursive vendor/zed
yarn install                          # so dsl/build.mjs can find esbuild
cargo build -p operator-zed           # ~10 min on first build (V8 + zed deps)

# Run with audio + UI:
./target/debug/operator-zed apps/operator-zed/examples/scope.modular

# Headless DSL execution:
./target/debug/operator-zed --emit-graph apps/operator-zed/examples/hello.modular | jq
```

Optional: `./apps/operator-zed/macos/build-app.sh --install` to make the binary drivable through the computer-use MCP.

## Follow-up backlog

Ordered roughly by leverage, not by size.

1. **Inline scope blocks (HANDOFF Step 6, the rest of it)**. The data is already on a ring; lift `scopes::render_waveform` into a `RenderBlock` keyed off the source-location anchor and let `Editor::insert_blocks` paint it inline. ~1 day, mostly geometry/diff bookkeeping.
2. **Lift `crates/modular_host`**. Move `crates/modular/src/{audio,midi,link,params_cache,validation}.rs` into a shared crate so operator-zed can stop carrying its own minimal driver. Unlocks Link sync, MIDI, BPM detection. ~1 day refactor; touches the napi addon.
3. **Real slider drag**. Replace the `−`/`+` buttons with a draggable rail (gpui mouse handlers). Pure UI work.
4. **`op_argument_spans` (oxc_parser)** + remove `analyze_source_stub`. Re-enables DSL argument highlighting in the editor.
5. **`op_load_wav`, `op_workspace_root`** so `$wavs(...)` works.
6. **--emit-graph parity test**. Run `--emit-graph` against every fixture in `src/main/dsl/__tests__/` and byte-compare with the Electron build's output. Catches drift between the napi shim and the real runtime.
7. **Workspace tree (recursion + collapse)** in the file explorer. Today it's flat.

Branch: `zed-prototype` on `~/dev/modular`. Recent heads:

- `9b8a469` — scopes panel
- `1218586` — slider controls panel
- `3fa990c` — file explorer + module split
- `882e2c1` — hot-swap Patch from cmd-S into cpal
- (earlier) — deno_core bundle, --emit-graph, editor::init + cmd-S
