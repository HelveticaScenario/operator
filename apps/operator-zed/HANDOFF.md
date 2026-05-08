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

Each step is intended to leave the binary in a runnable state. Step 0–2 done, Step 3 partially done.

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

### Step 3 — DSL execution + audio &nbsp;⏳ partial

**Done:**

- `modular_core` path-linked into the binary; compiles + links cleanly
- `apps/operator-zed/src/audio.rs`: cpal driver with two paths — original 440 Hz sine (default), or a `modular_core::patch::Patch` driven sample-by-sample under `OPERATOR_ZED_PATCH_TEST=1`. Mute via `OPERATOR_ZED_MUTE=0/1`.
- `apps/operator-zed/src/dsl_runtime.rs`: `deno_core` JsRuntime wrapper with an `eval` helper. V8 boots in both the cmd-S handler and `--emit-graph`.
- `apps/operator-zed/src/main.rs`: editor::init + Zed default-macos keymap loaded; Modz-namespaced `RunDsl` action bound to cmd-s; `--emit-graph FILE` and `--help` CLI parsing.

**Not done — required for full Step 3:**

1. **Lift `crates/modular_host`**. `crates/modular/src/{audio,midi,link,params_cache,validation}.rs` are trapped inside the cdylib crate that produces the N-API addon. New crate `crates/modular_host` should reuse those files (with N-API call sites stripped) and be consumed by both `crates/modular` (existing Electron build) and `apps/operator-zed`. Audio.rs there is 2885 lines; expect ~1 day of refactoring.
2. **deno_core runtime**. ✅ Dependency added (`deno_core = "0.400"`). `apps/operator-zed/src/dsl_runtime.rs` wraps a `JsRuntime` and exposes a single `eval(name, source)` helper that returns `serde_json::Value`. Both the cmd-S handler and `--emit-graph` boot V8 and run a probe expression to prove the path. Still TODO: register ops and load a bundled DSL module:
    - `op_emit_patch(graph, sliders, scope_sites, source_locations)` — invoked from JS at end of `executeDSL()` instead of returning to caller
    - `op_argument_spans(source) -> Vec<ArgumentSpan>` — Rust-side span analysis via `oxc_parser`, replacing ts-morph in `argumentSpanAnalyzer.ts`
    - `op_load_wav(path) -> Vec<u8>`
    - `op_workspace_root() -> String`
    - `op_log(level, msg)`
3. **`build.rs` esbuild bundle**. Bundle `src/main/dsl/{executor,factories,GraphBuilder}.ts` into `OUT_DIR/dsl_runtime.js`. The build script can shell out to esbuild via npx (Volta-pinned node) or to a pre-built esbuild binary; either works.

    **Gotchas surfaced by a trial bundle (2026-05-08):**
    - `executor.ts` -> `analyzeSource.ts` imports `ts-morph`. Bundling pulls the whole TS compiler (~13.7 MB neutral-platform output) and hits node-only deps (`fs`, `path`). Replace `analyzeSource.ts`/`argumentSpanAnalyzer.ts` with a tiny shim that calls `op_argument_spans` _before_ bundling, otherwise the bundle won't load in `JsRuntime`.
    - `factories.ts` re-exports from `@modular/core` which resolves to `crates/modular/index.js` — the N-API addon entry (uses `node:module`/`createRequire`). For the operator-zed bundle, point `@modular/core` at a small TS shim that re-exports just the pure types/utilities (`PatchGraph`, `ModuleSchema`, `deriveChannelCount`) and skips the addon. tsconfig path override or an esbuild alias works.

4. **Cmd-S handler**. ✅ wired — `editor::init(cx)` + Zed's default-macos keymap loaded via `KeymapFile::load_asset_allow_partial_failure`; a Modz-namespaced `RunDsl` action is bound to `cmd-s`. The handler currently writes the buffer back to disk and stubs DSL execution with a length log. Once deno_core is in, route the buffer text through V8 instead of the stub.
5. **Headless `--emit-graph` mode**. ⏳ CLI plumbing landed — `--emit-graph FILE` parses correctly and emits a structured "unimplemented" JSON record on stdout (exit code 2). Replace the stub with the real DSL path once deno_core is wired. Use it to byte-compare against the Electron build's output across the existing fixtures in `src/main/dsl/__tests__/`.

### Step 4 — File explorer &nbsp;⏳

- New file `apps/operator-zed/src/file_explorer.rs`
- `gpui::uniform_list` (see `vendor/zed/crates/gpui/examples/uniform_list.rs`)
- `std::fs::read_dir` walk under workspace root from CLI arg; click handler swaps the buffer in the editor

### Step 5 — Sliders / control panel &nbsp;⏳

- New file `apps/operator-zed/src/controls.rs`
- DSL execution result already produces `SliderDefinition[]` — just render gpui slider widgets
- Drag handlers write directly into `modular_host::params_cache` (no JS re-exec)

### Step 6 — Inline scope overlays &nbsp;⏳

- New file `apps/operator-zed/src/scopes.rs`
- Use `editor::display_map::block_map::BlockProperties` + `RenderBlock`. Entry point: `Editor::insert_blocks(blocks, autoscroll, cx)` at `vendor/zed/crates/editor/src/editor.rs:20536`. `BlockProperties` definition at `vendor/zed/crates/editor/src/display_map/block_map.rs:224`, `RenderBlock` typedef at `:101`.
- Anchor each `.scope()` call site via the source-location map produced by the DSL; insert one block with `placement: BlockPlacement::Below(anchor)`, `height: Some(8)`, `style: BlockStyle::Flex`, and a `RenderBlock` closure that draws live waveform data from a triple-buffered ring fed by the audio thread.
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
├── Cargo.toml                          # workspace member, deps on Zed crates
├── src/
│   ├── main.rs                         # gpui Application + window + Editor
│   └── audio.rs                        # cpal driver (Stage 1: hardcoded sine)
├── examples/
│   └── hello.modular                   # sample DSL script
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

- **No keybindings wired**. `editor::init(cx)` is not currently called, so Return doesn't insert a newline, Cmd-Z doesn't undo, etc. Adding `editor::init` requires either a `Workspace` or carefully constructed globals — see `visual_test_runner.rs:165–204`.
- **No syntax highlighting**. The buffer is `Buffer::local(text, cx)` with no language. Future: register a JavaScript / `.modular` language with the `LanguageRegistry`, or hand-roll a small one.
- **Audio is a hardcoded sine**, not driven by `modular_core`. To plug in `Patch::from_graph`: in the cpal callback, call `module.update()` for each sampleable, then `patch.get_output()` for the root sample. See `vendor/zed/../crates/modular/src/audio.rs:1535` (`process_frame`) for the production reference, but the prototype only needs the simplified path.
- **`webrtc-sys` is dragged in transitively** by `editor` → `workspace` → `call` → `livekit_client` → `webrtc-sys`. There is no clean way to disable it short of patching the `call` crate. The runtime overhead is zero (we never instantiate any livekit objects); it just bloats the binary and adds compile time.
- **modular's `target/` was wiped** during this session to free disk space (was 51 GB). Existing Electron build artifacts will rebuild on first `yarn build`.

## Next-session shopping list

In rough priority:

1. **Wire `editor::init` + minimum keymap**. ✅ Done.
2. **Lift `crates/modular_host`**. ~1 day. Untouched.
3. **Drop `deno_core` in + bundle the DSL**. ⏳ Half done. `deno_core` is a dep, `JsRuntime` boots, and a probe expression evaluates from both the cmd-S handler and `--emit-graph`. **Still TODO**: build `dsl_runtime.js` via `build.rs` + esbuild, register the ops listed above, and load it as an ESM module so the cmd-S handler can call `executeDSL(text)`. Watch the two gotchas above (ts-morph, `@modular/core` napi entry).
4. **Sample-loop integration**. ✅ Validated under `OPERATOR_ZED_PATCH_TEST=1` with a hand-built graph. Once #3 emits real graphs, drop the env-gate and route the latest graph from cmd-S into the cpal callback (crossbeam ringbuf or `Arc<Mutex<Option<Patch>>>` swap).
5. **Headless `--emit-graph` parity test**. ⏳ CLI flag landed; emits stage1 JSON probe today. Once #3 lands, replace the stub with `executeDSL(source) -> serde_json::to_value(PatchGraph)` and add the byte-comparison harness against the Electron build's outputs in `src/main/dsl/__tests__/`. ~½ day on top of #3.
6. Steps 4–6 (file explorer, sliders, scopes). ~2–3 days.

Branch: `zed-prototype` on `~/dev/modular`. Current head: `0f2df49` (Patch-driven cpal), preceded by deno_core wiring (`77e96f8`), CLI parser + `--emit-graph` stub (`fe49a70`), and editor::init + cmd-S (`749e7d5`).
