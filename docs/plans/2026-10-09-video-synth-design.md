# Video Synth — Design and Phase Roadmap

LZX-style video synthesis modules, authored in the Operator DSL, evaluated on the GPU, and displayed in a dedicated output window.

Phase 1 task detail lives in [2026-10-09-video-synth-phase1.md](./2026-10-09-video-synth-phase1.md). Later phases are scoped here and get their own task plans when they start.

---

## 1. Concept

An analog video synthesizer is the audio synthesizer's architecture at a higher rate. The raster scan is two ramp oscillators (horizontal and vertical). Every module is a function over signals that vary across the frame: shapes come from shaping and comparing ramps, and mixing, keying and colorizing are arithmetic on the results. Feedback and video-as-modulator both come from wiring.

The digital equivalent: **a video signal is a scalar field `f(x, y, t)`**, and a video patch is a graph of fields. The output is three fields (R, G, B).

## 2. Architecture decisions

| Decision                    | Choice                                                                          | Reason                                                                                                             |
| --------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| Where video is evaluated    | GPU fragment (later compute) shaders                                            | Per-pixel evaluation at video rates does not fit the real-time audio-thread rules.                                 |
| Shader language             | WGSL                                                                            | Runs under Chromium WebGPU now and under `wgpu`/naga if a native sink is ever needed.                              |
| Where the graph is compiled | TypeScript, in the main process DSL layer                                       | Rust never sees the video graph; the audio contract is unchanged.                                                  |
| Display                     | Dedicated Electron `BrowserWindow` using WebGPU                                 | Same shader throughput as a native window (both are GPU-bound); the existing Syphon bridge can publish the window. |
| Signal domain               | Separate `$v.*` namespace; video handles cannot connect to audio inputs         | Prevents mixing signals of different rates by accident.                                                            |
| Signal convention           | Normalized "volts" in `[0, 1]`, clipping where the analog module clips          | Overdrive, clipping and wrap behavior are much of the aesthetic.                                                   |
| Control inputs              | Per-frame uniforms (numeric literals, `$slider` values, later audio-derived CV) | Cheap, and covers most control use.                                                                                |

Departure from the repo convention "patch graphs are the contract — update Rust types": the video graph is a second, TypeScript-owned contract that travels beside the audio `PatchGraph` and is validated in TypeScript.

### Backend-neutral IR

`VideoGraph` (in `src/shared/video/`) is the single description consumed by the compiler:

- **Nodes:** module kind, params, input edges.
- **Uniform slots:** one per runtime-variable scalar, indexed into a single uniform buffer.
- **Passes:** the graph is partitioned into passes. Adjacent stateless nodes fuse into one pass; nodes with memory (feedback, frame delay) begin a new pass and read the previous frame's texture.

The compiler is a pure function `VideoGraph → { wgsl, uniformLayout, passes }`, unit-tested by shader-text snapshots.

### Data flow

1. DSL runs in `executePatchScript`; `$v.*` calls populate a `VideoGraphBuilder`.
2. `executePatchScript` compiles the graph and returns the `CompiledVideoShader` on `DSLExecutionResult`; compile errors surface as patch errors.
3. Main keeps the latest shader and pushes it to the output window over IPC (`VIDEO_ON_SHADER`; the window fetches the current one with `VIDEO_GET_SHADER` on load).
4. The output window's `VideoRenderer` rebuilds the pipeline on shader change and writes uniforms every frame.

### File layout

- `src/shared/video/` — IR types (`videoGraph.ts`), uniform buffer layout (`uniformLayout.ts`).
- `src/shared/dsl/videoDocs.ts` — the `$v` documentation table.
- `src/main/dsl/video/VideoGraphBuilder.ts` — `$v.*` calls to graph.
- `src/main/dsl/video/wgslCompiler.ts` — graph to WGSL.
- `src/main/dsl/video/modules/*.ts` — one WGSL definition per module (shared helpers in `transform.ts`).
- `src/main/dsl/videoLibGen.ts` — typings from the docs table.
- `src/main/performanceWindow.ts` — window lifecycle, uniform and tap polling.
- `src/renderer/video/` — `VideoRenderer` (frame loop), `ShaderProgram`, `FeedbackBuffers`, `PreviewCapture`, `previewViews` (monitors).
- `src/renderer/components/PerformanceWindow.tsx`, `VideoHelp.tsx`, `monaco/videoPreviewViewZones.ts`, `app/videoPreviewAnchors.ts`.
- `crates/modular_core/src/dsp/utilities/video_tap.rs` — the `_videoTap` module and its atomics.

Files stay under ~400 lines; split by domain.

---

## 3. Module catalog

Target set, grouped as in an LZX system. Names are provisional.

| Group          | Modules                                                                                     |
| -------------- | ------------------------------------------------------------------------------------------- |
| Scan / sync    | `$v.ramp` (H, V, diagonal; rotate and offset), `$v.out`                                     |
| Generators     | `$v.osc` (H/V frequency, phase, shape, sync), `$v.shape` (circle, box, line with soft edge) |
| Math           | `$v.mix`, `$v.mult`, `$v.diff`, `$v.invert`, `$v.warp` (coordinate modulation)              |
| Logic / keying | `$v.comparator`, `$v.keyer`, `$v.wipe`                                                      |
| Color          | `$v.colorize` (RGB or HSV from three fields), `$v.procAmp`, `$v.posterize`                  |
| Memory         | `$v.feedback` (zoom, rotate, shift, gain), `$v.frameDelay`                                  |
| Bridges        | `$v.fromAudio`, `$v.toCV`                                                                   |

---

## 4. Phases

Status as built. Phases 1–4 are implemented; the remaining work is listed under each phase and in section 7.

### Phase 1 — Vertical slice (done)

The IR, the WGSL compiler, `VideoGraphBuilder`, the performance window with a WebGPU renderer, uniform updates and Monaco typings. `$v.ramp`, `$v.osc`, `$v.colorize` and `$v.out` were the first modules.

### Phase 2 — Mixing, keying and control from the audio engine (done)

**Modules:** `add`, `mult`, `diff`, `min`, `max`, `invert`, `mix` (fields, or colors when either operand is a color), `comparator`, `key`, `shape`, `procAmp`, `posterize`, `hsv`, `wrap`, `fold`, and ramps that can be zoomed, rotated and shifted and that have radial (`r`) and angular (`a`) axes. Coordinate warping the LZX way, by modulating ramps before the oscillators, works with these. Warping an arbitrary field would need its inputs re-evaluated at shifted coordinates and is not implemented.

**Control:** sliders and buttons bind to uniform slots by their backing module id, and main pushes new values on `SYNTH_SET_MODULE_PARAM`. Any other audio signal binds to an engine tap: the DSL inserts a hidden `_videoTap` module whose input is the signal, the module stores the value in a static array of atomics on the audio thread, and main polls the array at 60 Hz while the performance window is open and the shader reads a tap. Taps carry volts as the audio graph produces them. A patch can read 64 audio signals, one channel each.

### Phase 3 — Feedback and memory (done)

`$v.feedback(update, config)` compiles to a read of the previous frame, resampled through a zoom, rotation, shift and edge mode, and a write of the new frame. The fused shader writes each loop into an extra render target; ping-pong half-float textures carry it to the next frame, up to seven loops. Buffers survive patch re-runs so edits do not wipe the picture, and a window resize clears them. `$v.frameDelay` is not implemented.

### Phase 4 — Display and monitoring (previews done)

**Direction:** the output window is the **performance window**: the general-purpose, audience-facing surface that replaces showing the audience the editor. Video is its first content; the code view and other visuals are meant to share it. It opens from View → Toggle Performance Window (`operator.togglePerformanceWindow`, default Ctrl+Shift+V) and when a patch first calls `$v.out`.

**Previews:** `$v.preview(signal, { view })` returns its signal and shows it in a panel under the call in the editor, as an image, a waveform monitor or a vectorscope.

- The compiler adds a fragment entry point `preview_k` per preview that evaluates only that signal's dependencies and reads the feedback textures without writing them, so a preview shows the same frame the audience sees.
- The performance window draws previews into small targets (144 pixels tall at the output's aspect ratio) every other frame, reads them back and sends them to main, which relays them to the editor.
- The editor anchors each panel to its call with a tracked range, so panels follow edits.
- Monitors are computed in the editor from the preview's pixels.
- The performance window must be open for previews to update.

**Resolved question:** thumbnails read an intermediate texture back through staging buffers rather than running a second renderer in the editor, which would have drifted from the real output wherever feedback is involved. With three previews on a feedback patch, frame pacing on a 120 Hz display was unchanged: mean 8.33 ms, p95 9.2 ms, max 9.4 ms, no frame over 20 ms in 570. That is rAF pacing, not an isolated GPU measurement.

**Not built:**

- Fullscreen on a chosen display, aspect and resolution scale options for the performance window.
- Syphon publishing of the performance window. `SyphonBridge.start` takes the window to capture, but the occlusion and throttling handling that keeps a captured window painting is written for the main window and needs checking against a real Syphon client.
- Analog-look post-processing (scanlines, noise, bloom).

### Phase 5 — Audio and video bridges (partly done)

Audio-to-video is covered by taps for control-rate signals. Not built: `$v.fromAudio`, which writes audio-rate samples along scanlines, and `$v.toCV`, which samples a pixel or region per frame and returns a control signal to the audio graph.

### Phase 6 — Polish and documentation (done for the module set)

One table, `src/shared/dsl/videoDocs.ts`, generates both the Monaco typings (JSDoc and declarations) and the Help window's Video page, and a test fails if `$v` gains a member without an entry. Every example in the table is run by the example-validity harness.

---

## 5. Verification strategy

- **Compiler:** pure-function unit tests with shader-text snapshots; cover fusion, pass partitioning and uniform layout.
- **Builder / DSL:** vitest tests for graph construction and argument validation; DSL positional-args-only convention applies to `$v.*`.
- **Typings:** `typescriptLibGen.ts` hand-mirrors signatures and has no typecheck guard, so add a test that the generated `$v.*` declarations match the factories.
- **Rendering:** a headless WebGPU frame-comparison test for a small set of reference graphs where the environment supports it; otherwise e2e through the Playwright harness against the output window.
- **Performance:** frame-time capture at 1080p60 and 4K using WebGPU timestamp queries. No speedup or capacity claims until measured.

## 6. Risks

- **WebGPU availability and limits** in the pinned Electron version; confirm early in phase 1 and keep the feature check explicit.
- **Shader size growth** as graphs get large; mitigate with fusion and a node-count cap.
- **Uniform plumbing correctness** — hidden allowlists in the DSL layer fail silently or late (see the pattern-wrapper checklist); add tests for every new wrapper method.
- **Thumbnail cost** (phase 4) and **readback latency** (phase 5).

## 7. Remaining and deferred

Not built yet, in rough order of value:

- Performance window options: fullscreen on a chosen display, aspect, resolution scale.
- Syphon publishing of the performance window.
- `$v.frameDelay`, `$v.fromAudio` and `$v.toCV`.
- Arbitrary-field warping (needs re-evaluating a field's inputs at shifted coordinates).
- Analog-look post-processing: scanlines, noise, bloom.
- Generators and filters that need more than the current pixel: noise, blur.

Deferred indefinitely:

- A native `wgpu` sink (HDR, 10-bit, NDI, genlock-grade pacing). Revisit only if Syphon and the Electron window prove insufficient.
- Compute-shader modules (histogram, convolution) beyond what the above needs.
- Image or live camera inputs as source fields.
- Deterministic offline rendering to video files.
