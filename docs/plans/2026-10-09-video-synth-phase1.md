# Video Synth — Phase 1 (WGSL vertical slice)

> Roadmap for all phases: [2026-10-09-video-synth-design.md](./2026-10-09-video-synth-design.md).

**Goal:** LZX-style video modules authored in the DSL, compiled to a WGSL fragment shader, and shown in a dedicated output window. Phase 1 proves the compile path end to end with `$v.ramp`, `$v.osc`, `$v.colorize` and `$v.out`.

## Model

A video signal is a scalar field `f(x, y, t)`. A video patch is a graph of fields; the output is three fields (R, G, B). Values are normalized "volts" in `[0, 1]` that clip at module boundaries where the analog module would clip.

## Decisions

- **GPU, not the Rust engine.** Per-pixel evaluation at video rates does not fit the audio-thread model. Rust never sees the video graph.
- **Compile in TypeScript to WGSL.** The same WGSL runs under Chromium WebGPU now and under `wgpu`/naga if a native sink is ever needed.
- **Separate signal domain.** `$v.*` factories return `VideoOutput` handles. They cannot be wired into audio inputs.
- **Control inputs are uniforms.** Numeric literals and `$slider` values become per-frame uniforms. Audio-engine signals arrive in phase 2.
- **Transport.** The compiled video graph rides on `DSLExecuteResult` beside `sliders`/`buttons`; the output window receives it from main.

## Layout

- `src/shared/video/videoGraph.ts` — `VideoGraph` IR types (nodes, edges, uniform slots).
- `src/main/dsl/video/VideoGraphBuilder.ts` — `$v.*` factories and graph construction.
- `src/main/dsl/video/wgslCompiler.ts` — IR → WGSL (pure function, unit-tested).
- `src/main/dsl/video/modules/*.ts` — one file per module: params, WGSL snippet.
- `src/renderer/video/VideoRenderer.ts` — WebGPU device, pipeline, uniform buffer, frame loop.
- `src/main/videoWindow.ts` — output `BrowserWindow` lifecycle.

## Tasks

1. IR types and a hand-built graph compiled to WGSL; snapshot test of the shader text.
2. WebGPU renderer in the output window; render a hard-coded graph.
3. `$v.ramp`, `$v.osc`, `$v.colorize`, `$v.out` modules and the `VideoGraphBuilder`.
4. Wire into `executePatchScript`: expose `$v`, return `video` on `DSLExecutionResult` and `DSLExecuteResult`.
5. Main-process window management; send graph and uniform updates over IPC.
6. Monaco typings for `$v.*` in `typescriptLibGen.ts`.
7. Verification: unit tests for the compiler and builder; run the app and confirm a patch renders.

## Out of scope

Feedback passes, inline thumbnails, audio-to-video bridges and monitor views (phases 2–5).
