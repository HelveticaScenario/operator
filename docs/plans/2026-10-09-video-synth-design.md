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

**Control:** sliders and buttons bind to uniform slots by their backing module id, and main pushes new values on `SYNTH_SET_MODULE_PARAM`. Any other audio signal binds to an engine tap: the DSL inserts a hidden `_videoTap` module whose input is the signal, and the module writes every sample into a 4096-sample ring on the audio thread. The renderer asks main for the samples produced since its last request once per display frame, and only while the shader reads a tap, and plays them back against the display clock through a jitter buffer (`TapStream`), so each frame reads the signal exactly where it is at that moment. The buffer's trail behind the newest sample is not a constant: it is the widest recent gap between batches that carried data, which already contains the audio buffer size, the display's refresh rate and the transport's jitter, so changing the audio buffer or the sample rate (the stream starts over) or moving to another display adjusts it by itself.

The engine produces samples a callback at a time, so the newest sample only moves every ~11.6 ms; reading "the latest value" made a 1 Hz LFO step visibly. With the stream, a 1 Hz LFO feeding a feedback patch at 120 Hz delivered about 1180 frames with no repeated value and a largest frame-to-frame step of 0.012 to 0.013 against 0.0105 for the ideal sine. In simulation the worst trail behind real time ranged from 12 ms (144 Hz display, 128-sample buffer) to 29 ms (60 or 120 Hz display, 512-sample buffer at 44.1 kHz), and grew to 85 ms for a 2048-sample buffer, which hands samples over in 43 ms bursts. The audible output delay of the audio device is not measured. A patch can read 64 audio signals, one channel each, in volts as the audio graph produces them.

**Hydra-inspired additions** (from reading Hydra's function library: sources, coordinate transforms, color ops, blends and "modulate" warps):

- **Warps that move a whole sub-patch.** Every node can be compiled as a function of the coordinate, `fn f<i>(uv) -> T`, alongside the memoized statement at the pixel's own coordinate. `warp`, `displace`, `modulate`, `kaleid`, `pixelate` and `repeat` call their input as a function at coordinates they compute, so they move, bend or fold everything behind it, not just its output. Graphs without warps compile as before.
- **Chaining.** Every video signal has the `$v` functions as methods with itself as the first argument (`$v.osc(...).modulate(...).kaleid(6).hsv().out()`), plus `rotate`, `scale` and `scroll` as shorthand for `warp`.
- **Named buffers.** `$v.buffer()` gives a frame store any signal can write and any number of signals can read the previous frame of, so buffers can feed themselves and each other. `$v.feedback` is the same mechanism with one read and one write.
- **All of Hydra's modulators.** `modulate`, `modulateScale`, `modulateRotate`, `modulatePixelate`, `modulateKaleid`, `modulateHue`, `modulateRepeat`, `modulateRepeatX`, `modulateRepeatY`, `modulateScrollX` and `modulateScrollY`, each a coordinate module like `warp`: the input's whole sub-patch is read at coordinates that the modulator moves. A color modulator supplies its red, green and blue as Hydra's do, and a field its one value for every channel it is asked for (so `modulateHue`, which uses channel differences, does nothing with a field). Counts and scale factors take their numbers from Hydra; angles, offsets, scrolls and the other fractions are fractions of 5 volts, so `modulateRotate`'s `multiple` of 5 is a full turn.
- **Library gaps filled:** `voronoi`, `polygon`, `hueShift`, `contrast`, `channel`. Hydra's `thresh`, `luma`, `brightness` and `saturate` are `comparator`, `key` and `procAmp`.
- **Filters on whole sub-patches.** Because any node can be read as a function of the coordinate, `blur` (sixteen taps over a disk, turned per pixel so the gaps read as grain, not ghost copies) and `edges` (a Sobel filter) work on any signal without textures. Their cost is the input's cost times the taps.
- **Media.** `$v.image(path)` and `$v.video(path)` read pictures and recordings from the workspace folder as colors, with `cover`, `contain` or `stretch` fitting. They are textures the shader samples at the coordinate being drawn, so warps and feedback move footage like any pattern. The files are served to the renderer over an `operator-media://` scheme that resolves inside the workspace folder only; videos play muted in a loop and a texture refreshes when the video has a new frame. H.264 MP4 and VP9 WebM both play in the app's browser engine.
- **Post effects:** `scanlines`, `vignette` and `grain`.
- **Chaining mechanics match the audio graph.** `.$` is a namespace of the `$v` functions that take a signal first, `.$m` adds a leading mix, `.pipe(fn)` and `.pipe(fn, array)` apply functions, and `.pipeMix(fn, mix)` crossfades; `out`, `preview`, `toCV` and `write` end or tap a chain as direct methods. `tint(hue, saturation)` colors a mask, where `hsv` on a field treats it as the hue.

### Phase 3 — Feedback and memory (done)

`$v.feedback(update, config)` compiles to a read of the previous frame, resampled through a zoom, rotation, shift and edge mode, and a write of the new frame. The fused shader writes each loop into an extra render target; ping-pong half-float textures carry it to the next frame, up to seven loops. Buffers survive patch re-runs so edits do not wipe the picture, and a window resize clears them. `$v.frameDelay(input, frames)` is a chain of buffers, each written with the previous one's read, so it costs a buffer per frame held back. The renderer asks the adapter for enough color-attachment bytes per pixel to draw all seven buffers; the default limit allows three.

### Phase 4 — Display and monitoring (previews done)

**Direction:** the picture is drawn once, by the editor window's renderer, and shown in two places. Behind the code it fills the editor area. The **performance window**, the audience-facing surface that replaces showing the audience the editor, opens from View → Toggle Performance Window (`operator.togglePerformanceWindow`, default Ctrl+Shift+V) and never opens by itself.

- The editor opens the performance window with `window.open`, so it can reach the new window's document. The window holds a plain canvas, and after each frame the renderer copies its canvas into it. One renderer means one clock, one set of audio taps and one set of videos and feedback buffers, so the two views are the same frame and nothing needs syncing.
- While the performance window is open, the renderer's canvas takes its pixel size and the picture behind the code is fitted inside the editor area at that aspect ratio, so the performer sees exactly what the audience sees. Otherwise the canvas follows the editor area.
- The window is a sibling of the editor, not the editor's own content. While it is open the editor window keeps painting when backgrounded, because the performance window is only as live as the editor's frames.
- The video canvas sits above the `$scopeXY` background canvas and below the code, so a patch with video hides the XY scope.
- **Shader and audio change together.** The audio taps a shader reads are numbered afresh by every compile, and the engine's tap slots carry the new patch's signals from the moment it applies the update. So the renderer builds a new shader as soon as it arrives but makes it live only once the engine reports that update applied (`last_applied_update_id`); an update the engine discards (`last_cancelled_update_id`) leaves the previous shader. From the moment the engine applies the update the old shader holds its last frame instead of reading the new patch's signals, and when the new shader goes live every tap stream starts again from the engine's newest samples, with no frame drawn until each tap the shader reads has data. A beat-quantized update therefore changes the picture on the beat with the audio.

**Previews:** `$v.preview(signal, { view })` returns its signal and shows it in a panel under the call in the editor, as an image, a waveform monitor or a vectorscope.

- The compiler adds a fragment entry point `preview_k` per preview that evaluates only that signal's dependencies and reads the feedback textures without writing them, so a preview shows the same frame the audience sees.
- The renderer draws previews into small targets (144 pixels tall at the output's aspect ratio) every other frame, reads them back and sends them to main, which relays them to the editor.
- The editor anchors each panel to its call with a tracked range, so panels follow edits.
- Monitors are computed in the editor from the preview's pixels.
- Previews update whether or not the performance window is open.

**Resolved question:** thumbnails read an intermediate texture back through staging buffers rather than running a second renderer in the editor, which would have drifted from the real output wherever feedback is involved. With three previews on a feedback patch, frame pacing on a 120 Hz display was unchanged: mean 8.33 ms, p95 9.2 ms, max 9.4 ms, no frame over 20 ms in 570. That is rAF pacing, not an isolated GPU measurement.

**Performance window options.** The editor window's settings give the performance window a shape (free, 16:9, 4:3, 1:1 or 9:16) and a resolution scale (100%, 75%, 50% or 25% of the window's pixels, which the window shows stretched to fit). View → Performance Window Fullscreen lists each display and puts the window fullscreen on it, opening the window first if needed; on macOS it uses simple fullscreen, which takes no Spaces transition, and Exit Fullscreen returns the window to its earlier size, shape and place. The menu follows displays as they come and go. The window logic is in `src/main/performanceWindow.ts`.

**Syphon.** View has two Syphon items, one publishing the editor window and one the performance window; choosing the other while one is on moves publishing over, and closing the performance window stops its feed. The helper is a screen-capture of the chosen window, so each item needs Screen Recording permission, and the performance-window path has not been tried against a real Syphon client. Both windows keep painting while they are captured because the editor window does not throttle while the performance window is open or Syphon is on.

**Not built:**

- Analog-look post-processing (scanlines, noise, bloom) as a single chain; the pieces exist as `scanlines`, `vignette`, `grain` and `bloom`.

### Phase 5 — Audio and video bridges (done)

- **Audio to video, per frame:** taps, as above.
- **Audio to video, at audio rate:** `$v.fromAudio(signal, position, { samples, trigger })` lays the last `samples` samples (up to 4096) of an audio signal along `position`, oldest at 0 and newest at 1. The window can start at a rising zero crossing so a periodic wave holds still. Compare it with the vertical ramp to draw an oscilloscope, or feed it a radial ramp for rings.
- **Video to audio:** `$v.toCV(signal, { x, y, size })` averages a region of the picture each frame into a 0..1 audio control signal. A closed loop (a ramp sampled at x = 0.25 and x = 0.9, sent through the audio graph and back in as a gray level) read back 64 and 229 of 255.

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

## Value scale

Video values are in volts like audio signals: a field runs from 0 to 5 and 5 is full. A ramp spans 0 to 5 across the frame, a color channel at 5 is fully on, an amount or strength of 5 is all of it, and a position of 2.5 is the center. Audio signals, sliders and `$v.time` are volts too, so a 0 to 5 volt signal drives a full-scale input as it is. This matches the audio convention that 5 volts is unity, as `amp` has it, so the same numbers mean the same thing on both sides of `toCV` and `fromAudio`.

- **Full-scale inputs** (colour channels, hue, saturation, value, mix amounts, thresholds, softness, strengths, sizes, radii, shifts, positions, the push of a displacement) take volts, with 5 meaning all of it.
- **Angles and phases** are fractions of 5 as well: a `rotate` of 5 is a full turn and a `phase` of 5 a full cycle, so a unipolar audio ramp drives a rotation or a phase directly, with no `.range`.
- **Natural-unit inputs** keep their own units and receive volts as they are: `freq` in cycles, `zoom` as a factor, the sides of a polygon or kaleidoscope, cell counts, the number of scanlines, noise coordinates in cells, and `gain` of `wrap` and `fold`. `freq: 10` is ten cycles whatever the input. `osc` spreads `freq` cycles across the 5 volts of its input, so `$v.osc($v.ramp(), 8)` is eight stripes and, because `$v.time` counts seconds as volts, `$v.osc($v.time, 5)` is once a second.
- **`.range(min, max)`** on a field, or `$v.range(field, min, max)`, maps 0 to 5 volts onto `min` to `max`, as the audio `.range` does. It gives a natural-unit input a span from a field: `$v.osc($v.time, 0.5).range(1, 3)` is a zoom swinging between one and three.
- **Products and the other combiners** work in fractions of 5, so `mult(a, b)` is `a * b / 5`: multiplying by 5 changes nothing, by 2.5 halves, and a field times a 0 to 5 field darkens as before.

The shader still computes with 5 volts as 1. The compiler converts as it resolves each input: a constant, an audio signal or the clock is divided by 5 on its way into a full-scale input and left alone for a natural-unit one, and a field is multiplied by 5 on its way into a natural-unit input. Modules declare which inputs are natural with `natural` in their definition. `toCV` multiplies its region average by 5, so it produces 0 to 5 volts, and the audio history texture is divided by 5 on reading.

## 7. Remaining and deferred

Not built yet, in rough order of value:

- **Camera and screen capture as sources.** Needs a camera entitlement and usage string in the packaging and an OS permission prompt, so it was left for a step that can be tested with a real camera. Everything else about it (a texture refreshed from a video element, bound like `$v.video`) already exists.
- Sequences as parameters, as Hydra's arrays: a `$p` pattern through an audio tap already steps values with exact edges, but there is no video-side shorthand.

Deferred indefinitely:

- A native `wgpu` sink (HDR, 10-bit, NDI, genlock-grade pacing). Revisit only if Syphon and the Electron window prove insufficient.
- Compute-shader modules (histogram, convolution) beyond what the above needs.
- Deterministic offline rendering to video files.
