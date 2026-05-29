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

export const MAX_TRACES = 16;
export const SCOPE_XY_CAPACITY = 2048;

// Lanczos upsampler tuning: each input sample produces STEPS interpolated
// outputs using a sinc window of half-width RADIUS source samples. Output
// count = N*STEPS + 1.
const UPSAMPLE_STEPS = 6;
const UPSAMPLE_RADIUS = 8;
const UPSAMPLE_LANCZOS_TWEAK = 1.5;
const UPSAMPLED_LEN = SCOPE_XY_CAPACITY * UPSAMPLE_STEPS + 1;

function buildLanczosKernel(a: number, steps: number): Float32Array {
    const kernel = new Float32Array(a * steps);
    kernel[0] = 1;
    for (let i = 1; i < kernel.length; i++) {
        const piX = (Math.PI * i) / steps;
        const sinc = Math.sin(piX) / piX;
        const window = (a * Math.sin(piX / a)) / piX;
        kernel[i] = sinc * Math.pow(window, UPSAMPLE_LANCZOS_TWEAK);
    }
    return kernel;
}


export interface ScopeXYPairData {
    x: Float32Array;
    y: Float32Array;
    /** Index of the most recently written sample (one past the newest). */
    head: number;
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
    /** Tonemap exposure passed to the composite (1-exp(-uExposure*L)). */
    exposure?: number;
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

function compile(
    gl: WebGLRenderingContext,
    type: number,
    source: string,
): WebGLShader {
    const sh = gl.createShader(type);
    if (!sh) throw new Error('createShader failed');
    gl.shaderSource(sh, source);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
        const log = gl.getShaderInfoLog(sh);
        gl.deleteShader(sh);
        throw new Error(`Shader compile failed: ${log}`);
    }
    return sh;
}

function link(
    gl: WebGLRenderingContext,
    vs: string,
    fs: string,
): WebGLProgram {
    const v = compile(gl, gl.VERTEX_SHADER, vs);
    const f = compile(gl, gl.FRAGMENT_SHADER, fs);
    const prog = gl.createProgram();
    if (!prog) throw new Error('createProgram failed');
    gl.attachShader(prog, v);
    gl.attachShader(prog, f);
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
        const log = gl.getProgramInfoLog(prog);
        gl.deleteShader(v);
        gl.deleteShader(f);
        gl.deleteProgram(prog);
        throw new Error(`Program link failed: ${log}`);
    }
    gl.deleteShader(v);
    gl.deleteShader(f);
    return prog;
}

interface Fbo {
    fb: WebGLFramebuffer;
    tex: WebGLTexture;
    width: number;
    height: number;
}

function createFbo(
    gl: WebGLRenderingContext,
    width: number,
    height: number,
): Fbo {
    const tex = gl.createTexture();
    if (!tex) throw new Error('createTexture failed');
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        width,
        height,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        null,
    );
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

    const fb = gl.createFramebuffer();
    if (!fb) throw new Error('createFramebuffer failed');
    gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
    gl.framebufferTexture2D(
        gl.FRAMEBUFFER,
        gl.COLOR_ATTACHMENT0,
        gl.TEXTURE_2D,
        tex,
        0,
    );
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return { fb, tex, width, height };
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

    let lineSize = options.beamSize ?? 0.012;
    let intensity = options.intensity ?? 0.6;
    let fadeAmount = options.fadeAmount ?? 0.15;
    let upsample = options.upsample ?? true;
    const exposure = options.exposure ?? 1.2;
    let beamColor = new Float32Array([
        ...(options.color ?? [0.05, 1.0, 0.35]),
        1,
    ]);
    let backgroundColor = new Float32Array([
        ...(options.background ?? [0, 0, 0]),
        1,
    ]);

    const lineShader = link(gl, vsLine, fsLine);
    const fadeShader = link(gl, vsQuad, fsFade);
    const copyShader = link(gl, vsQuad, fsCopy);
    const blurShader = link(gl, vsQuad, fsBlur);
    const compositeShader = link(gl, vsQuad, fsComposite);

    const nSamples = UPSAMPLED_LEN;

    // Lanczos kernel uploaded once as a vertex-shader uniform array; the
    // GPU does the upsampling.
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
    const fullscreenQuad = new Float32Array([
        -1, -1, 1, -1, -1, 1, 1, 1,
    ]);
    const fullscreenBuf = gl.createBuffer();
    if (!fullscreenBuf) throw new Error('createBuffer failed');
    gl.bindBuffer(gl.ARRAY_BUFFER, fullscreenBuf);
    gl.bufferData(gl.ARRAY_BUFFER, fullscreenQuad, gl.STATIC_DRAW);

    // Interleaves one trace's x/y volts into RGBA float texels before
    // texSubImage2D. 2048 samples × 4 channels, reused across traces.
    const sampleScratch = new Float32Array(SCOPE_XY_CAPACITY * 4);

    // Per-trace sample textures (2048×1 RGBA float). Allocated lazily to
    // match how many traces are actually drawn.
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
            gl!.texParameteri(
                gl!.TEXTURE_2D,
                gl!.TEXTURE_MIN_FILTER,
                gl!.NEAREST,
            );
            gl!.texParameteri(
                gl!.TEXTURE_2D,
                gl!.TEXTURE_MAG_FILTER,
                gl!.NEAREST,
            );
            gl!.texParameteri(
                gl!.TEXTURE_2D,
                gl!.TEXTURE_WRAP_S,
                gl!.CLAMP_TO_EDGE,
            );
            gl!.texParameteri(
                gl!.TEXTURE_2D,
                gl!.TEXTURE_WRAP_T,
                gl!.CLAMP_TO_EDGE,
            );
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

    let lineFbo = createFbo(gl, canvas.width, canvas.height);
    let tightFboA = createFbo(
        gl,
        halfDim(canvas.width),
        halfDim(canvas.height),
    );
    let tightFboB = createFbo(
        gl,
        halfDim(canvas.width),
        halfDim(canvas.height),
    );
    let bigFboA = createFbo(
        gl,
        eighthDim(canvas.width),
        eighthDim(canvas.height),
    );
    let bigFboB = createFbo(
        gl,
        eighthDim(canvas.width),
        eighthDim(canvas.height),
    );

    function disposeFbo(f: Fbo) {
        gl!.deleteFramebuffer(f.fb);
        gl!.deleteTexture(f.tex);
    }

    function resize(w: number, h: number) {
        w = Math.max(1, Math.floor(w));
        h = Math.max(1, Math.floor(h));
        canvas.width = w;
        canvas.height = h;
        disposeFbo(lineFbo);
        disposeFbo(tightFboA);
        disposeFbo(tightFboB);
        disposeFbo(bigFboA);
        disposeFbo(bigFboB);
        lineFbo = createFbo(gl!, w, h);
        tightFboA = createFbo(gl!, halfDim(w), halfDim(h));
        tightFboB = createFbo(gl!, halfDim(w), halfDim(h));
        bigFboA = createFbo(gl!, eighthDim(w), eighthDim(h));
        bigFboB = createFbo(gl!, eighthDim(w), eighthDim(h));
    }

    function setColors(
        beam: [number, number, number],
        bg: [number, number, number],
    ) {
        beamColor = new Float32Array([beam[0], beam[1], beam[2], 1]);
        backgroundColor = new Float32Array([bg[0], bg[1], bg[2], 1]);
    }

    function setIntensity(v: number) {
        intensity = v;
    }

    function setLineWidth(v: number) {
        lineSize = v;
    }

    function setFadeAmount(v: number) {
        fadeAmount = v;
    }

    function setUpsample(enabled: boolean) {
        upsample = enabled;
    }

    interface UploadedTrace {
        outLen: number;
        xMin: number;
        xSpan: number;
        yMin: number;
        ySpan: number;
    }

    function uploadTraceSamples(
        tex: WebGLTexture,
        pair: ScopeXYPairData,
    ): UploadedTrace | null {
        const inLen = Math.min(
            SCOPE_XY_CAPACITY,
            pair.x.length,
            pair.y.length,
        );
        if (inLen < 2) return null;

        // pair.x/y arrive pre-linearized by the audio thread's snapshot():
        // index 0 is the oldest sample, index inLen-1 the newest. Read
        // sequentially; `pair.head` is metadata only.
        for (let s = 0; s < inLen; s++) {
            const t = s * 4;
            sampleScratch[t] = pair.x[s];
            sampleScratch[t + 1] = pair.y[s];
            sampleScratch[t + 2] = 0;
            sampleScratch[t + 3] = 0;
        }
        const lastX = pair.x[inLen - 1];
        const lastY = pair.y[inLen - 1];
        for (let s = inLen; s < SCOPE_XY_CAPACITY; s++) {
            const t = s * 4;
            sampleScratch[t] = lastX;
            sampleScratch[t + 1] = lastY;
            sampleScratch[t + 2] = 0;
            sampleScratch[t + 3] = 0;
        }

        gl!.bindTexture(gl!.TEXTURE_2D, tex);
        gl!.texSubImage2D(
            gl!.TEXTURE_2D,
            0,
            0,
            0,
            SCOPE_XY_CAPACITY,
            1,
            gl!.RGBA,
            gl!.FLOAT,
            sampleScratch,
        );

        const xMin = pair.xRange?.[0] ?? -5;
        const xMax = pair.xRange?.[1] ?? 5;
        const yMin = pair.yRange?.[0] ?? -5;
        const yMax = pair.yRange?.[1] ?? 5;
        return {
            outLen: inLen * UPSAMPLE_STEPS + 1,
            xMin,
            xSpan: Math.max(xMax - xMin, 1e-6),
            yMin,
            ySpan: Math.max(yMax - yMin, 1e-6),
        };
    }

    function bindFullscreenQuad(shader: WebGLProgram) {
        gl!.bindBuffer(gl!.ARRAY_BUFFER, fullscreenBuf);
        const aPos = gl!.getAttribLocation(shader, 'aPos');
        if (aPos > -1) {
            gl!.enableVertexAttribArray(aPos);
            gl!.vertexAttribPointer(aPos, 2, gl!.FLOAT, false, 0, 0);
        }
        return aPos;
    }

    function fadePass(alpha: number) {
        gl!.bindFramebuffer(gl!.FRAMEBUFFER, lineFbo.fb);
        gl!.viewport(0, 0, lineFbo.width, lineFbo.height);
        gl!.useProgram(fadeShader);
        const aPos = bindFullscreenQuad(fadeShader);
        const uColorLoc = gl!.getUniformLocation(fadeShader, 'uColor');
        gl!.enable(gl!.BLEND);

        // Subtract a small constant per frame to push dim trails past
        // 8-bit's 1/255 quantization floor, where multiplicative decay
        // rounds a value back to itself and the trail sticks.
        gl!.blendEquation(gl!.FUNC_REVERSE_SUBTRACT);
        gl!.blendFunc(gl!.ONE, gl!.ONE);
        const epsilon = 2.0 / 255.0;
        gl!.uniform4f(uColorLoc, epsilon, epsilon, epsilon, 0);
        gl!.drawArrays(gl!.TRIANGLE_STRIP, 0, 4);

        // Multiplicative decay: dst *= (1 - alpha).
        gl!.blendEquation(gl!.FUNC_ADD);
        gl!.blendFunc(gl!.SRC_ALPHA, gl!.ONE_MINUS_SRC_ALPHA);
        gl!.uniform4f(uColorLoc, 0, 0, 0, alpha);
        gl!.drawArrays(gl!.TRIANGLE_STRIP, 0, 4);

        gl!.disable(gl!.BLEND);
        if (aPos > -1) gl!.disableVertexAttribArray(aPos);
    }

    let lineShaderUniformsSetup = false;
    function drawLine(
        sampleTex: WebGLTexture,
        color: Float32Array,
        upload: UploadedTrace,
    ) {
        gl!.useProgram(lineShader);
        // Kernel is constant for the lifetime of the pipeline; upload once.
        if (!lineShaderUniformsSetup) {
            gl!.uniform1fv(
                gl!.getUniformLocation(lineShader, 'uKernel[0]'),
                upsampleKernel,
            );
            lineShaderUniformsSetup = true;
        }
        gl!.uniform1f(
            gl!.getUniformLocation(lineShader, 'uSize'),
            lineSize,
        );
        // Cube the slider value so the brightness knob spans a wide
        // perceptual range. The beam alpha accumulates additively before the
        // exposure tonemap, so a linear multiplier barely dims a dense trace;
        // the cubic gives near-black at the low end. Normalised by 0.36 so the
        // 0.6 default lands at unity gain (0.6³ / 0.36 = 0.6).
        const beamGain = (intensity * intensity * intensity) / 0.36;
        gl!.uniform1f(
            gl!.getUniformLocation(lineShader, 'uIntensity'),
            beamGain,
        );
        gl!.uniform1f(
            gl!.getUniformLocation(lineShader, 'uFadeAmount'),
            fadeAmount,
        );
        gl!.uniform1f(
            gl!.getUniformLocation(lineShader, 'uNumSamples'),
            nSamples,
        );
        gl!.uniform4fv(
            gl!.getUniformLocation(lineShader, 'uColor'),
            color,
        );
        gl!.uniform2f(
            gl!.getUniformLocation(lineShader, 'uXRange'),
            upload.xMin,
            upload.xSpan,
        );
        gl!.uniform2f(
            gl!.getUniformLocation(lineShader, 'uYRange'),
            upload.yMin,
            upload.ySpan,
        );
        gl!.uniform1f(
            gl!.getUniformLocation(lineShader, 'uUpsample'),
            upsample ? 1.0 : 0.0,
        );

        gl!.activeTexture(gl!.TEXTURE0);
        gl!.bindTexture(gl!.TEXTURE_2D, sampleTex);
        gl!.uniform1i(gl!.getUniformLocation(lineShader, 'uSamples'), 0);

        gl!.bindBuffer(gl!.ARRAY_BUFFER, quadIndexBuf);
        const aIdx = gl!.getAttribLocation(lineShader, 'aIdx');
        if (aIdx > -1) {
            gl!.enableVertexAttribArray(aIdx);
            gl!.vertexAttribPointer(aIdx, 1, gl!.UNSIGNED_SHORT, false, 2, 0);
        }

        gl!.enable(gl!.BLEND);
        gl!.blendFunc(gl!.SRC_ALPHA, gl!.ONE);
        gl!.bindBuffer(gl!.ELEMENT_ARRAY_BUFFER, vertexIndexBuf);
        gl!.drawElements(
            gl!.TRIANGLES,
            (upload.outLen - 1) * 6,
            gl!.UNSIGNED_SHORT,
            0,
        );
        gl!.disable(gl!.BLEND);

        if (aIdx > -1) gl!.disableVertexAttribArray(aIdx);
    }

    function copyPass(src: Fbo, dst: Fbo) {
        gl!.bindFramebuffer(gl!.FRAMEBUFFER, dst.fb);
        gl!.viewport(0, 0, dst.width, dst.height);
        gl!.useProgram(copyShader);
        const aPos = bindFullscreenQuad(copyShader);
        gl!.activeTexture(gl!.TEXTURE0);
        gl!.bindTexture(gl!.TEXTURE_2D, src.tex);
        gl!.uniform1i(gl!.getUniformLocation(copyShader, 'uTexture'), 0);
        gl!.disable(gl!.BLEND);
        gl!.drawArrays(gl!.TRIANGLE_STRIP, 0, 4);
        if (aPos > -1) gl!.disableVertexAttribArray(aPos);
    }

    function blurPass(src: Fbo, dst: Fbo, dx: number, dy: number) {
        gl!.bindFramebuffer(gl!.FRAMEBUFFER, dst.fb);
        gl!.viewport(0, 0, dst.width, dst.height);
        gl!.useProgram(blurShader);
        const aPos = bindFullscreenQuad(blurShader);
        gl!.activeTexture(gl!.TEXTURE0);
        gl!.bindTexture(gl!.TEXTURE_2D, src.tex);
        gl!.uniform1i(gl!.getUniformLocation(blurShader, 'uTexture'), 0);
        gl!.uniform2f(
            gl!.getUniformLocation(blurShader, 'uOffset'),
            dx,
            dy,
        );
        gl!.disable(gl!.BLEND);
        gl!.drawArrays(gl!.TRIANGLE_STRIP, 0, 4);
        if (aPos > -1) gl!.disableVertexAttribArray(aPos);
    }

    function compositePass() {
        gl!.bindFramebuffer(gl!.FRAMEBUFFER, null);
        gl!.viewport(0, 0, canvas.width, canvas.height);
        gl!.useProgram(compositeShader);
        const aPos = bindFullscreenQuad(compositeShader);
        gl!.activeTexture(gl!.TEXTURE0);
        gl!.bindTexture(gl!.TEXTURE_2D, lineFbo.tex);
        gl!.uniform1i(gl!.getUniformLocation(compositeShader, 'uLine'), 0);
        gl!.activeTexture(gl!.TEXTURE1);
        gl!.bindTexture(gl!.TEXTURE_2D, tightFboA.tex);
        gl!.uniform1i(gl!.getUniformLocation(compositeShader, 'uTight'), 1);
        gl!.activeTexture(gl!.TEXTURE2);
        gl!.bindTexture(gl!.TEXTURE_2D, bigFboA.tex);
        gl!.uniform1i(gl!.getUniformLocation(compositeShader, 'uBig'), 2);
        gl!.uniform1f(
            gl!.getUniformLocation(compositeShader, 'uExposure'),
            exposure,
        );
        gl!.uniform3f(
            gl!.getUniformLocation(compositeShader, 'uBackground'),
            backgroundColor[0],
            backgroundColor[1],
            backgroundColor[2],
        );
        gl!.disable(gl!.BLEND);
        gl!.drawArrays(gl!.TRIANGLE_STRIP, 0, 4);
        if (aPos > -1) gl!.disableVertexAttribArray(aPos);
    }

    function draw(pairs: ScopeXYPairData[]) {
        if (canvas.width === 0 || canvas.height === 0) return;

        // Pass 1: persistence fade.
        fadePass(fadeAmount);

        // Pass 2: lines additive over the faded trace.
        gl!.bindFramebuffer(gl!.FRAMEBUFFER, lineFbo.fb);
        gl!.viewport(0, 0, lineFbo.width, lineFbo.height);
        const drawCount = Math.min(pairs.length, MAX_TRACES);
        for (let i = 0; i < drawCount; i++) {
            const tex = getTraceTexture(i);
            const upload = uploadTraceSamples(tex, pairs[i]);
            if (!upload) continue;
            drawLine(tex, beamColor, upload);
        }

        // Pass 3: tight bloom — downsample, then horizontal+vertical blur.
        copyPass(lineFbo, tightFboA);
        blurPass(tightFboA, tightFboB, 1.0 / tightFboA.width, 0);
        blurPass(tightFboB, tightFboA, 0, 1.0 / tightFboB.height);

        // Pass 4: big bloom — further downsample then blur diagonally so the
        // 1/8-res kernel still covers a perceptible radius.
        copyPass(tightFboA, bigFboA);
        blurPass(bigFboA, bigFboB, 1.0 / bigFboA.width, 0);
        blurPass(bigFboB, bigFboA, 0, 1.0 / bigFboB.height);

        // Pass 5: composite to default framebuffer.
        compositePass();
    }

    function dispose() {
        disposeFbo(lineFbo);
        disposeFbo(tightFboA);
        disposeFbo(tightFboB);
        disposeFbo(bigFboA);
        disposeFbo(bigFboB);
        for (const tex of traceTextures) gl!.deleteTexture(tex);
        gl!.deleteBuffer(quadIndexBuf);
        gl!.deleteBuffer(vertexIndexBuf);
        gl!.deleteBuffer(fullscreenBuf);
        gl!.deleteProgram(lineShader);
        gl!.deleteProgram(fadeShader);
        gl!.deleteProgram(copyShader);
        gl!.deleteProgram(blurShader);
        gl!.deleteProgram(compositeShader);
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
