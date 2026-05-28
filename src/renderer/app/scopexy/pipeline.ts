// Pipeline combining m1el/woscope's analytic line shader with the rendering
// passes from dood.al/oscilloscope (Neil Thapen). Five stages per frame:
//
//   1. fade           — alpha-blend a dark quad over the previous frame's
//                       lineFbo so the trace decays exponentially (CRT
//                       phosphor persistence).
//   2. line           — additive draw of every active trace into lineFbo.
//   3. tight bloom    — half-res downsample then separable 17-tap Gaussian.
//   4. big bloom      — further 1/8-res downsample then another 17-tap blur
//                       for the wide ambient halo.
//   5. composite      — tonemapped sum of line + tight + big with bright
//                       pixels washing toward white.

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
     * trail. dood.al exposes this as a slider; we hard-code a default.
     */
    fadeAmount?: number;
    /** Tonemap exposure passed to the composite (1-exp(-uExposure*L)). */
    exposure?: number;
}

export interface ScopeXY {
    draw(pairs: ScopeXYPairData[]): void;
    resize(width: number, height: number): void;
    setColors(
        beam: [number, number, number],
        background: [number, number, number],
    ): void;
    setIntensity(intensity: number): void;
    setFadeAmount(fadeAmount: number): void;
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

    const lineSize = options.beamSize ?? 0.012;
    let intensity = options.intensity ?? 0.6;
    let fadeAmount = options.fadeAmount ?? 0.15;
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

    const nSamples = SCOPE_XY_CAPACITY;

    // aIdx: SHORT, one component, vertex index 0..nSamples*4-1.
    const quadIndexData = new Int16Array(nSamples * 4);
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

    // Scratch CPU buffer reused across traces: nSamples × 8 floats per
    // sample (x,y replicated four times so woscope's stride-8 attribute
    // trick works).
    const scratch = new Float32Array(nSamples * 8);
    const traceVbos: WebGLBuffer[] = [];
    function getTraceVbo(idx: number): WebGLBuffer {
        while (traceVbos.length <= idx) {
            const vbo = gl!.createBuffer();
            if (!vbo) throw new Error('createBuffer failed');
            gl!.bindBuffer(gl!.ARRAY_BUFFER, vbo);
            gl!.bufferData(
                gl!.ARRAY_BUFFER,
                scratch.byteLength,
                gl!.DYNAMIC_DRAW,
            );
            traceVbos.push(vbo);
        }
        return traceVbos[idx];
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

    function setFadeAmount(v: number) {
        fadeAmount = v;
    }

    function loadTraceVbo(vbo: WebGLBuffer, pair: ScopeXYPairData): number {
        const len = Math.min(nSamples, pair.x.length, pair.y.length);
        if (len < 2) return 0;
        const xMin = pair.xRange?.[0] ?? -5;
        const xMax = pair.xRange?.[1] ?? 5;
        const yMin = pair.yRange?.[0] ?? -5;
        const yMax = pair.yRange?.[1] ?? 5;
        const xSpan = Math.max(xMax - xMin, 1e-6);
        const ySpan = Math.max(yMax - yMin, 1e-6);
        const head = pair.head;
        for (let s = 0; s < len; s++) {
            const r = (head + s) % len;
            const x = ((pair.x[r] - xMin) / xSpan) * 2 - 1;
            const y = ((pair.y[r] - yMin) / ySpan) * 2 - 1;
            const t = s * 8;
            scratch[t] =
                scratch[t + 2] =
                scratch[t + 4] =
                scratch[t + 6] =
                    x;
            scratch[t + 1] =
                scratch[t + 3] =
                scratch[t + 5] =
                scratch[t + 7] =
                    y;
        }
        gl!.bindBuffer(gl!.ARRAY_BUFFER, vbo);
        gl!.bufferSubData(
            gl!.ARRAY_BUFFER,
            0,
            scratch.subarray(0, len * 8),
        );
        return len;
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

        // Subtract a small constant per frame so dim trails actually reach
        // zero. Without this, multiplicative decay in 8-bit asymptotes at
        // 1/255 (it rounds back to itself) and the bloom amplifies that
        // floor into a persistent ghost.
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

    function drawLine(vbo: WebGLBuffer, color: Float32Array, len: number) {
        gl!.useProgram(lineShader);
        gl!.uniform1f(
            gl!.getUniformLocation(lineShader, 'uSize'),
            lineSize,
        );
        gl!.uniform1f(
            gl!.getUniformLocation(lineShader, 'uIntensity'),
            intensity,
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

        gl!.bindBuffer(gl!.ARRAY_BUFFER, quadIndexBuf);
        const aIdx = gl!.getAttribLocation(lineShader, 'aIdx');
        if (aIdx > -1) {
            gl!.enableVertexAttribArray(aIdx);
            gl!.vertexAttribPointer(aIdx, 1, gl!.SHORT, false, 2, 0);
        }

        gl!.bindBuffer(gl!.ARRAY_BUFFER, vbo);
        const aStart = gl!.getAttribLocation(lineShader, 'aStart');
        if (aStart > -1) {
            gl!.enableVertexAttribArray(aStart);
            gl!.vertexAttribPointer(aStart, 2, gl!.FLOAT, false, 8, 0);
        }
        const aEnd = gl!.getAttribLocation(lineShader, 'aEnd');
        if (aEnd > -1) {
            gl!.enableVertexAttribArray(aEnd);
            gl!.vertexAttribPointer(aEnd, 2, gl!.FLOAT, false, 8, 8 * 4);
        }

        gl!.enable(gl!.BLEND);
        gl!.blendFunc(gl!.SRC_ALPHA, gl!.ONE);
        gl!.bindBuffer(gl!.ELEMENT_ARRAY_BUFFER, vertexIndexBuf);
        gl!.drawElements(
            gl!.TRIANGLES,
            (len - 1) * 6,
            gl!.UNSIGNED_SHORT,
            0,
        );
        gl!.disable(gl!.BLEND);

        if (aIdx > -1) gl!.disableVertexAttribArray(aIdx);
        if (aStart > -1) gl!.disableVertexAttribArray(aStart);
        if (aEnd > -1) gl!.disableVertexAttribArray(aEnd);
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
            const vbo = getTraceVbo(i);
            const len = loadTraceVbo(vbo, pairs[i]);
            if (len < 2) continue;
            drawLine(vbo, beamColor, len);
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
        for (const vbo of traceVbos) gl!.deleteBuffer(vbo);
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
        setFadeAmount,
        dispose,
    };
}
