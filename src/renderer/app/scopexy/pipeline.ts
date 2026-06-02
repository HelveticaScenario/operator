// XY scope pipeline. Renders pairs of voltage samples per trace through a
// five-stage GPU pipeline each frame:
//
//   1. fade        — alpha-blend a dark quad over lineFbo so the trace
//                    decays exponentially (CRT phosphor persistence).
//   2. line        — additive draw of every active trace into lineFbo.
//   3. tight bloom — half-res downsample + separable 17-tap gaussian.
//   4. big bloom   — 1/8-res downsample + another 17-tap blur for the
//                    wide ambient halo.
//   5. composite   — tonemapped sum of line + tight + big to the canvas,
//                    with bright pixels washing toward white.
//
// Attribution: line geometry/integral adapted from m1el/woscope (MIT,
// https://github.com/m1el/woscope); persistence pass, dual-stage gaussian
// bloom, and exposure-mapped composite adapted from dood.al/oscilloscope
// by Neil Thapen (https://dood.al/oscilloscope/).

import {
    fsBlur,
    fsComposite,
    fsCopy,
    fsFade,
    fsLine,
    vsLine,
    vsQuad,
} from './shaders';
import { createFbo, disposeFbo, link } from './glUtils';
import {
    MAX_TRACES,
    SCOPE_XY_CAPACITY,
    UPSAMPLE_RADIUS,
    UPSAMPLE_STEPS,
    UPSAMPLED_LEN,
    buildLanczosKernel,
} from './lanczos';
import {
    type ScopeXyContext,
    blurPass,
    compositePass,
    copyPass,
    drawLine,
    fadePass,
    uploadTraceSamples,
} from './passes';

export interface ScopeXYPairData {
    /** Samples in chronological order: index 0 oldest, last element newest. */
    x: Float32Array;
    y: Float32Array;
    /** Per-axis voltage windows (`[min, max]`). Falls back to `[-5, 5]`. */
    xRange?: readonly [number, number];
    yRange?: readonly [number, number];
}

export interface ScopeXYOptions {
    /** Beam colour in [0, 1]. */
    color?: [number, number, number];
    /** Background tint added in the composite. */
    background?: [number, number, number];
    /** Beam half-width in clip-space units. */
    beamSize?: number;
    /** Per-fragment line-shader intensity multiplier. */
    intensity?: number;
    /**
     * Fraction of `lineFbo` discarded per frame (0..1). Higher = shorter
     * trail; 0 = no fade (trail accumulates indefinitely).
     */
    fadeAmount?: number;
    /** Enable GPU Lanczos upsampling. Default true. */
    upsample?: boolean;
}

export interface ScopeXY {
    draw(pairs: ScopeXYPairData[]): void;
    resize(width: number, height: number): void;
    setColors(
        beam: [number, number, number],
        background: [number, number, number],
    ): void;
    setIntensity(intensity: number): void;
    setLineWidth(lineWidth: number): void;
    setFadeAmount(fadeAmount: number): void;
    setUpsample(enabled: boolean): void;
    dispose(): void;
}

export function createScopeXY(
    canvas: HTMLCanvasElement,
    options: ScopeXYOptions = {},
): ScopeXY {
    const gl = canvas.getContext('webgl', {
        antialias: false,
        depth: false,
        premultipliedAlpha: false,
        preserveDrawingBuffer: false,
        stencil: false,
    });
    if (!gl) {
        throw new Error('WebGL is not available');
    }
    // OES_texture_float is required for the per-trace sample texture (raw
    // voltages stored as 32-bit floats). Available everywhere we care about
    // (macOS Chromium / Electron, modern desktop GPUs).
    if (!gl.getExtension('OES_texture_float')) {
        throw new Error('OES_texture_float not supported');
    }

    const lineShader = link(gl, vsLine, fsLine);
    const fadeShader = link(gl, vsQuad, fsFade);
    const copyShader = link(gl, vsQuad, fsCopy);
    const blurShader = link(gl, vsQuad, fsBlur);
    const compositeShader = link(gl, vsQuad, fsComposite);

    const nSamples = UPSAMPLED_LEN;

    // Lanczos kernel uploaded once as a vertex-shader uniform array; the GPU
    // does the upsampling.
    const upsampleKernel = buildLanczosKernel(UPSAMPLE_RADIUS, UPSAMPLE_STEPS);

    // aIdx: UNSIGNED_SHORT, one component, vertex index 0..nSamples*4-1.
    // Upsampled max (~49k) exceeds signed SHORT range, so unsigned.
    const quadIndexData = new Uint16Array(nSamples * 4);
    for (let i = 0; i < quadIndexData.length; i++) quadIndexData[i] = i;
    const quadIndexBuf = gl.createBuffer();
    if (!quadIndexBuf) throw new Error('createBuffer failed');
    gl.bindBuffer(gl.ARRAY_BUFFER, quadIndexBuf);
    gl.bufferData(gl.ARRAY_BUFFER, quadIndexData, gl.STATIC_DRAW);

    // Triangle indices: two tris per segment, four verts per segment.
    const vertexIndexLen = (nSamples - 1) * 6;
    const vertexIndexData = new Uint16Array(vertexIndexLen);
    {
        let pos = 0;
        for (let i = 0; i < vertexIndexLen; ) {
            vertexIndexData[i++] = pos;
            vertexIndexData[i++] = pos + 2;
            vertexIndexData[i++] = pos + 1;
            vertexIndexData[i++] = pos + 1;
            vertexIndexData[i++] = pos + 2;
            vertexIndexData[i++] = pos + 3;
            pos += 4;
        }
    }
    const vertexIndexBuf = gl.createBuffer();
    if (!vertexIndexBuf) throw new Error('createBuffer failed');
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, vertexIndexBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, vertexIndexData, gl.STATIC_DRAW);

    // Fullscreen quad: aPos.xy only — vTexCoord derived in vsQuad.
    const fullscreenQuad = new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]);
    const fullscreenBuf = gl.createBuffer();
    if (!fullscreenBuf) throw new Error('createBuffer failed');
    gl.bindBuffer(gl.ARRAY_BUFFER, fullscreenBuf);
    gl.bufferData(gl.ARRAY_BUFFER, fullscreenQuad, gl.STATIC_DRAW);

    // Interleaves one trace's x/y volts into RGBA float texels before
    // texSubImage2D. 2048 samples × 4 channels, reused across traces.
    const sampleScratch = new Float32Array(SCOPE_XY_CAPACITY * 4);

    // Per-trace sample textures (2048×1 RGBA float). Allocated lazily to match
    // how many traces are actually drawn.
    const traceTextures: WebGLTexture[] = [];
    function getTraceTexture(idx: number): WebGLTexture {
        while (traceTextures.length <= idx) {
            const tex = gl!.createTexture();
            if (!tex) throw new Error('createTexture failed');
            gl!.bindTexture(gl!.TEXTURE_2D, tex);
            gl!.texImage2D(
                gl!.TEXTURE_2D,
                0,
                gl!.RGBA,
                SCOPE_XY_CAPACITY,
                1,
                0,
                gl!.RGBA,
                gl!.FLOAT,
                null,
            );
            gl!.texParameteri(gl!.TEXTURE_2D, gl!.TEXTURE_MIN_FILTER, gl!.NEAREST);
            gl!.texParameteri(gl!.TEXTURE_2D, gl!.TEXTURE_MAG_FILTER, gl!.NEAREST);
            gl!.texParameteri(gl!.TEXTURE_2D, gl!.TEXTURE_WRAP_S, gl!.CLAMP_TO_EDGE);
            gl!.texParameteri(gl!.TEXTURE_2D, gl!.TEXTURE_WRAP_T, gl!.CLAMP_TO_EDGE);
            traceTextures.push(tex);
        }
        return traceTextures[idx];
    }

    function halfDim(n: number) {
        return Math.max(1, Math.floor(n / 2));
    }
    function eighthDim(n: number) {
        return Math.max(1, Math.floor(n / 8));
    }

    // Shared render context: GL handles, framebuffers (reassigned on resize),
    // and the live beam settings the passes read each frame.
    const ctx: ScopeXyContext = {
        gl,
        canvas,
        lineShader,
        fadeShader,
        copyShader,
        blurShader,
        compositeShader,
        fullscreenBuf,
        quadIndexBuf,
        vertexIndexBuf,
        upsampleKernel,
        sampleScratch,
        lineFbo: createFbo(gl, canvas.width, canvas.height),
        tightFboA: createFbo(gl, halfDim(canvas.width), halfDim(canvas.height)),
        tightFboB: createFbo(gl, halfDim(canvas.width), halfDim(canvas.height)),
        bigFboA: createFbo(gl, eighthDim(canvas.width), eighthDim(canvas.height)),
        bigFboB: createFbo(gl, eighthDim(canvas.width), eighthDim(canvas.height)),
        lineSize: options.beamSize ?? 0.012,
        intensity: options.intensity ?? 0.6,
        fadeAmount: options.fadeAmount ?? 0.15,
        upsample: options.upsample ?? true,
        // Fixed tonemap exposure for the composite (1 - exp(-uExposure*L)).
        exposure: 1.2,
        beamColor: new Float32Array([
            ...(options.color ?? [0.05, 1.0, 0.35]),
            1,
        ]),
        backgroundColor: new Float32Array([...(options.background ?? [0, 0, 0]), 1]),
        lineShaderUniformsSetup: false,
    };

    function resize(w: number, h: number) {
        w = Math.max(1, Math.floor(w));
        h = Math.max(1, Math.floor(h));
        canvas.width = w;
        canvas.height = h;
        disposeFbo(gl!, ctx.lineFbo);
        disposeFbo(gl!, ctx.tightFboA);
        disposeFbo(gl!, ctx.tightFboB);
        disposeFbo(gl!, ctx.bigFboA);
        disposeFbo(gl!, ctx.bigFboB);
        ctx.lineFbo = createFbo(gl!, w, h);
        ctx.tightFboA = createFbo(gl!, halfDim(w), halfDim(h));
        ctx.tightFboB = createFbo(gl!, halfDim(w), halfDim(h));
        ctx.bigFboA = createFbo(gl!, eighthDim(w), eighthDim(h));
        ctx.bigFboB = createFbo(gl!, eighthDim(w), eighthDim(h));
    }

    function setColors(
        beam: [number, number, number],
        bg: [number, number, number],
    ) {
        ctx.beamColor = new Float32Array([beam[0], beam[1], beam[2], 1]);
        ctx.backgroundColor = new Float32Array([bg[0], bg[1], bg[2], 1]);
    }

    function setIntensity(v: number) {
        ctx.intensity = v;
    }

    function setLineWidth(v: number) {
        ctx.lineSize = v;
    }

    function setFadeAmount(v: number) {
        ctx.fadeAmount = v;
    }

    function setUpsample(enabled: boolean) {
        ctx.upsample = enabled;
    }

    function draw(pairs: ScopeXYPairData[]) {
        if (canvas.width === 0 || canvas.height === 0) return;

        // Pass 1: persistence fade.
        fadePass(ctx, ctx.fadeAmount);

        // Pass 2: lines additive over the faded trace.
        gl!.bindFramebuffer(gl!.FRAMEBUFFER, ctx.lineFbo.fb);
        gl!.viewport(0, 0, ctx.lineFbo.width, ctx.lineFbo.height);
        const drawCount = Math.min(pairs.length, MAX_TRACES);
        for (let i = 0; i < drawCount; i++) {
            const tex = getTraceTexture(i);
            const upload = uploadTraceSamples(ctx, tex, pairs[i]);
            if (!upload) continue;
            drawLine(ctx, tex, ctx.beamColor, upload);
        }

        // Pass 3: tight bloom — downsample, then horizontal+vertical blur.
        copyPass(ctx, ctx.lineFbo, ctx.tightFboA);
        blurPass(ctx, ctx.tightFboA, ctx.tightFboB, 1.0 / ctx.tightFboA.width, 0);
        blurPass(ctx, ctx.tightFboB, ctx.tightFboA, 0, 1.0 / ctx.tightFboB.height);

        // Pass 4: big bloom — further downsample then blur so the 1/8-res
        // kernel still covers a perceptible radius.
        copyPass(ctx, ctx.tightFboA, ctx.bigFboA);
        blurPass(ctx, ctx.bigFboA, ctx.bigFboB, 1.0 / ctx.bigFboA.width, 0);
        blurPass(ctx, ctx.bigFboB, ctx.bigFboA, 0, 1.0 / ctx.bigFboB.height);

        // Pass 5: composite to default framebuffer.
        compositePass(ctx);
    }

    function dispose() {
        disposeFbo(gl!, ctx.lineFbo);
        disposeFbo(gl!, ctx.tightFboA);
        disposeFbo(gl!, ctx.tightFboB);
        disposeFbo(gl!, ctx.bigFboA);
        disposeFbo(gl!, ctx.bigFboB);
        for (const tex of traceTextures) gl!.deleteTexture(tex);
        gl!.deleteBuffer(quadIndexBuf);
        gl!.deleteBuffer(vertexIndexBuf);
        gl!.deleteBuffer(fullscreenBuf);
        gl!.deleteProgram(lineShader);
        gl!.deleteProgram(fadeShader);
        gl!.deleteProgram(copyShader);
        gl!.deleteProgram(blurShader);
        gl!.deleteProgram(compositeShader);
        // Note: deliberately NOT calling WEBGL_lose_context.loseContext() here.
        // With a single shared canvas there is no per-buffer context churn to
        // guard against, and force-losing the context on a dev StrictMode/HMR
        // remount wedges the GPU process (later contexts come up degraded —
        // missing extensions, failing shader compiles). The detached canvas's
        // context is reclaimed by GC.
    }

    return {
        draw,
        resize,
        setColors,
        setIntensity,
        setLineWidth,
        setFadeAmount,
        setUpsample,
        dispose,
    };
}
