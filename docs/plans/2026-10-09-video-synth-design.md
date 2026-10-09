# Video Synth — Design and Phase Roadmap

LZX-style video synthesis modules, authored in the Operator DSL, evaluated on the GPU, and displayed in a dedicated output window.

Phase 1 task detail lives in [2026-10-09-video-synth-phase1.md](./2026-10-09-video-synth-phase1.md). Later phases are scoped here and get their own task plans when they start.

---

## 1. Concept

An analog video synthesizer is the audio synthesizer's architecture at a higher rate. The raster scan is two ramp oscillators (horizontal and vertical). Every module is a function over signals that vary across the frame: shapes come from shaping and comparing ramps, and mixing, keying and colorizing are arithmetic on the results. Feedback and video-as-modulator both come from wiring.

The digital equivalent: **a video signal is a scalar field `f(x, y, t)`**, and a video patch is a graph of fields. The output is three fields (R, G, B).

## 2. Architecture decisions

| Decision | Choice | Reason |
|---|---|---|
| Where video is evaluated | GPU fragment (later compute) shaders | Per-pixel evaluation at video rates does not fit the real-time audio-thread rules. |
| Shader language | WGSL | Runs under Chromium WebGPU now and under `wgpu`/naga if a native sink is ever needed. |
| Where the graph is compiled | TypeScript, in the main process DSL layer | Rust never sees the video graph; the audio contract is unchanged. |
| Display | Dedicated Electron `BrowserWindow` using WebGPU | Same shader throughput as a native window (both are GPU-bound); the existing Syphon bridge can publish the window. |
| Signal domain | Separate `$v.*` namespace; video handles cannot connect to audio inputs | Prevents mixing signals of different rates by accident. |
| Signal convention | Normalized "volts" in `[0, 1]`, clipping where the analog module clips | Overdrive, clipping and wrap behavior are much of the aesthetic. |
| Control inputs | Per-frame uniforms (numeric literals, `$slider` values, later audio-derived CV) | Cheap, and covers most control use. |

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

### Planned file layout

- `src/shared/video/videoGraph.ts` — IR types.
- `src/main/dsl/video/VideoGraphBuilder.ts` — `$v.*` factories and graph construction.
- `src/main/dsl/video/wgslCompiler.ts` — IR → WGSL.
- `src/main/dsl/video/modules/*.ts` — one file per module: params, WGSL snippet, doc examples.
- `src/main/videoWindow.ts` — output window lifecycle.
- `src/renderer/video/VideoRenderer.ts` — WebGPU device, pipelines, uniform buffer, frame loop.
- `src/renderer/video/monitors/` — waveform and vector monitors (phase 4).

Files stay under ~400 lines; split by domain.

---

## 3. Module catalog

Target set, grouped as in an LZX system. Names are provisional.

| Group | Modules |
|---|---|
| Scan / sync | `$v.ramp` (H, V, diagonal; rotate and offset), `$v.out` |
| Generators | `$v.osc` (H/V frequency, phase, shape, sync), `$v.shape` (circle, box, line with soft edge) |
| Math | `$v.mix`, `$v.mult`, `$v.diff`, `$v.invert`, `$v.warp` (coordinate modulation) |
| Logic / keying | `$v.comparator`, `$v.keyer`, `$v.wipe` |
| Color | `$v.colorize` (RGB or HSV from three fields), `$v.procAmp`, `$v.posterize` |
| Memory | `$v.feedback` (zoom, rotate, shift, gain), `$v.frameDelay` |
| Bridges | `$v.fromAudio`, `$v.toCV` |

---

## 4. Phases

### Phase 1 — Vertical slice

**Delivers:** `$v.ramp`, `$v.osc`, `$v.colorize`, `$v.out`; the IR, the WGSL compiler, `VideoGraphBuilder`, the output window with a WebGPU renderer, uniform updates from literals and `$slider`, and Monaco typings for `$v.*`.

**Done when:** a patch containing those modules renders in the output window, editing a slider updates the picture without recompiling the pipeline, and the compiler has snapshot tests.

### Phase 2 — Mixing, keying and control from the audio engine

**Delivers:** `$v.mix`, `$v.mult`, `$v.diff`, `$v.invert`, `$v.comparator`, `$v.keyer`, `$v.wipe`, `$v.shape`, `$v.procAmp`, `$v.posterize`, `$v.warp`; audio-engine signals as uniforms.

**Audio-to-uniform path:** a tap in the engine publishes selected control values (the same polling pattern as `get_scopes()`), the renderer fetches them once per frame, and they populate uniform slots. No always-on JS polling beyond what a video graph with audio-derived inputs needs; with no such inputs the poll does not run.

**Done when:** an LFO or envelope in the audio graph drives a video parameter, and module behavior (clipping, wrap, polarity) has tests.

### Phase 3 — Feedback and memory

**Delivers:** `$v.feedback` and `$v.frameDelay` using ping-pong textures; pass partitioning in the compiler; resolution-independent feedback transforms.

**Done when:** a feedback patch is stable across window resizes, graph swaps preserve or deliberately reset feedback state (decided and documented), and the pass partitioner is unit-tested.

### Phase 4 — Display and monitoring

**Delivers:**
- Output window options: fullscreen on a chosen display, aspect (4:3, 16:9, free), resolution scale.
- Inline thumbnails of any video signal in the editor, reusing the scope view-zone machinery (`scopeViewZones.ts`, `trackedViewZones.ts`).
- Waveform monitor (luma against horizontal position) and vectorscope.
- Optional analog-look post-processing: scanlines, noise, soft bloom (the glow work from `2026-03-05-glow-effect-plan.md` is the reference).
- Syphon publishing of the output window through the existing `SyphonBridge`.

**Done when:** a thumbnail follows its call site as the code is edited, monitors can be toggled per signal, and Syphon publishing works from the video window.

**Open question:** thumbnails need a read of an intermediate texture. Decide between a small-resolution copy rendered by the output window and shipped to the editor, or a second lightweight renderer in the editor process. Measure both before choosing.

### Phase 5 — Audio and video bridges

**Delivers:**
- `$v.fromAudio`: writes audio-rate samples along scanlines so audio-rate signals become horizontal structure.
- `$v.toCV`: samples a pixel or region average per frame and returns a control signal to the audio graph (a GPU readback, once per frame).

**Done when:** an audio oscillator visibly shapes the picture, and a pixel region controls an audio parameter, with the readback latency characterized.

### Phase 6 — Polish and documentation

**Delivers:** doc examples for every `$v.*` module (validated by the example-validity harness), help-window pages, example patches, performance profile, and decisions on any deferred items below.

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

## 7. Deferred

- A native `wgpu` sink (HDR, 10-bit, NDI, genlock-grade pacing). Revisit only if Syphon and the Electron window prove insufficient.
- Compute-shader modules (histogram, convolution) beyond what phases 4–5 need.
- Image or live camera inputs as source fields.
- Deterministic offline rendering to video files.
