// GPU render passes for the XY scope, operating on a shared ScopeXyContext.
// The context owns the GL handles, framebuffers (reassigned on resize), and
// the live beam settings (mutated by the pipeline's setters); the passes read
// them at call time. pipeline.ts owns construction and the per-frame `draw`
// orchestration that sequences these.

import type { Fbo } from './glUtils';
import { SCOPE_XY_CAPACITY, UPSAMPLE_STEPS } from './lanczos';
import type { ScopeXYPairData } from './pipeline';

export interface ScopeXyContext {
    gl: WebGLRenderingContext;
    canvas: HTMLCanvasElement;
    // Shader programs.
    lineShader: WebGLProgram;
    fadeShader: WebGLProgram;
    copyShader: WebGLProgram;
    blurShader: WebGLProgram;
    compositeShader: WebGLProgram;
    // Static geometry + the upsampler kernel.
    fullscreenBuf: WebGLBuffer;
    quadIndexBuf: WebGLBuffer;
    vertexIndexBuf: WebGLBuffer;
    upsampleKernel: Float32Array;
    sampleScratch: Float32Array;
    // Framebuffers — reassigned on resize.
    lineFbo: Fbo;
    tightFboA: Fbo;
    tightFboB: Fbo;
    bigFboA: Fbo;
    bigFboB: Fbo;
    // Beam settings — mutated by the pipeline's setters.
    lineSize: number;
    intensity: number;
    fadeAmount: number;
    upsample: boolean;
    exposure: number;
    beamColor: Float32Array;
    backgroundColor: Float32Array;
    // One-shot: the Lanczos kernel uniform is uploaded on the first line draw.
    lineShaderUniformsSetup: boolean;
}

export interface UploadedTrace {
    outLen: number;
    xMin: number;
    xSpan: number;
    yMin: number;
    ySpan: number;
}

function bindFullscreenQuad(ctx: ScopeXyContext, shader: WebGLProgram) {
    const { gl } = ctx;
    gl.bindBuffer(gl.ARRAY_BUFFER, ctx.fullscreenBuf);
    const aPos = gl.getAttribLocation(shader, 'aPos');
    if (aPos > -1) {
        gl.enableVertexAttribArray(aPos);
        gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);
    }
    return aPos;
}

/**
 * Interleave one trace's chronological x/y volts into the RGBA float scratch
 * and upload it to `tex`, returning the per-trace draw metadata. The samples
 * arrive pre-linearized by the audio thread (index 0 oldest); slots past the
 * valid range are padded with the last sample so the upsampler's neighborhood
 * reads stay in-bounds.
 */
export function uploadTraceSamples(
    ctx: ScopeXyContext,
    tex: WebGLTexture,
    pair: ScopeXYPairData,
): UploadedTrace | null {
    const { gl, sampleScratch } = ctx;
    const inLen = Math.min(SCOPE_XY_CAPACITY, pair.x.length, pair.y.length);
    if (inLen < 2) return null;

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

    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texSubImage2D(
        gl.TEXTURE_2D,
        0,
        0,
        0,
        SCOPE_XY_CAPACITY,
        1,
        gl.RGBA,
        gl.FLOAT,
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

/** Pass 1: decay `lineFbo` toward black for CRT phosphor persistence. */
export function fadePass(ctx: ScopeXyContext, alpha: number) {
    const { gl, lineFbo, fadeShader } = ctx;
    gl.bindFramebuffer(gl.FRAMEBUFFER, lineFbo.fb);
    gl.viewport(0, 0, lineFbo.width, lineFbo.height);
    gl.useProgram(fadeShader);
    const aPos = bindFullscreenQuad(ctx, fadeShader);
    const uColorLoc = gl.getUniformLocation(fadeShader, 'uColor');
    gl.enable(gl.BLEND);

    // Subtract a small constant per frame to push dim trails past 8-bit's
    // 1/255 quantization floor, where multiplicative decay rounds a value back
    // to itself and the trail sticks.
    gl.blendEquation(gl.FUNC_REVERSE_SUBTRACT);
    gl.blendFunc(gl.ONE, gl.ONE);
    const epsilon = 2.0 / 255.0;
    gl.uniform4f(uColorLoc, epsilon, epsilon, epsilon, 0);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

    // Multiplicative decay: dst *= (1 - alpha).
    gl.blendEquation(gl.FUNC_ADD);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.uniform4f(uColorLoc, 0, 0, 0, alpha);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

    gl.disable(gl.BLEND);
    if (aPos > -1) gl.disableVertexAttribArray(aPos);
}

/** Pass 2: additive draw of one trace into `lineFbo`. */
export function drawLine(
    ctx: ScopeXyContext,
    sampleTex: WebGLTexture,
    color: Float32Array,
    upload: UploadedTrace,
) {
    const { gl, lineShader } = ctx;
    gl.useProgram(lineShader);
    // Kernel is constant for the lifetime of the pipeline; upload once.
    if (!ctx.lineShaderUniformsSetup) {
        gl.uniform1fv(
            gl.getUniformLocation(lineShader, 'uKernel[0]'),
            ctx.upsampleKernel,
        );
        ctx.lineShaderUniformsSetup = true;
    }
    gl.uniform1f(gl.getUniformLocation(lineShader, 'uSize'), ctx.lineSize);
    // Cube the slider value so the brightness knob spans a wide perceptual
    // range. The beam alpha accumulates additively before the exposure
    // tonemap, so a linear multiplier barely dims a dense trace; the cubic
    // gives near-black at the low end. Normalised by 0.36 so the 0.6 default
    // lands at unity gain (0.6³ / 0.36 = 0.6).
    const beamGain = (ctx.intensity * ctx.intensity * ctx.intensity) / 0.36;
    gl.uniform1f(gl.getUniformLocation(lineShader, 'uIntensity'), beamGain);
    gl.uniform1f(
        gl.getUniformLocation(lineShader, 'uFadeAmount'),
        ctx.fadeAmount,
    );
    // Afterglow gradient spans this trace's actual length so the newest vertex
    // (outIdx = outLen-1) reaches full intensity even before the ring fills,
    // instead of being scaled by the full buffer size.
    gl.uniform1f(
        gl.getUniformLocation(lineShader, 'uNumSamples'),
        Math.max(upload.outLen - 1, 1),
    );
    gl.uniform4fv(gl.getUniformLocation(lineShader, 'uColor'), color);
    gl.uniform2f(
        gl.getUniformLocation(lineShader, 'uXRange'),
        upload.xMin,
        upload.xSpan,
    );
    gl.uniform2f(
        gl.getUniformLocation(lineShader, 'uYRange'),
        upload.yMin,
        upload.ySpan,
    );
    gl.uniform1f(
        gl.getUniformLocation(lineShader, 'uUpsample'),
        ctx.upsample ? 1.0 : 0.0,
    );

    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, sampleTex);
    gl.uniform1i(gl.getUniformLocation(lineShader, 'uSamples'), 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, ctx.quadIndexBuf);
    const aIdx = gl.getAttribLocation(lineShader, 'aIdx');
    if (aIdx > -1) {
        gl.enableVertexAttribArray(aIdx);
        gl.vertexAttribPointer(aIdx, 1, gl.UNSIGNED_SHORT, false, 2, 0);
    }

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, ctx.vertexIndexBuf);
    gl.drawElements(
        gl.TRIANGLES,
        (upload.outLen - 1) * 6,
        gl.UNSIGNED_SHORT,
        0,
    );
    gl.disable(gl.BLEND);

    if (aIdx > -1) gl.disableVertexAttribArray(aIdx);
}

/** Downsample `src` into `dst` with a plain bilinear copy. */
export function copyPass(ctx: ScopeXyContext, src: Fbo, dst: Fbo) {
    const { gl, copyShader } = ctx;
    gl.bindFramebuffer(gl.FRAMEBUFFER, dst.fb);
    gl.viewport(0, 0, dst.width, dst.height);
    gl.useProgram(copyShader);
    const aPos = bindFullscreenQuad(ctx, copyShader);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, src.tex);
    gl.uniform1i(gl.getUniformLocation(copyShader, 'uTexture'), 0);
    gl.disable(gl.BLEND);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    if (aPos > -1) gl.disableVertexAttribArray(aPos);
}

/** Separable gaussian blur of `src` into `dst` along (dx, dy). */
export function blurPass(
    ctx: ScopeXyContext,
    src: Fbo,
    dst: Fbo,
    dx: number,
    dy: number,
) {
    const { gl, blurShader } = ctx;
    gl.bindFramebuffer(gl.FRAMEBUFFER, dst.fb);
    gl.viewport(0, 0, dst.width, dst.height);
    gl.useProgram(blurShader);
    const aPos = bindFullscreenQuad(ctx, blurShader);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, src.tex);
    gl.uniform1i(gl.getUniformLocation(blurShader, 'uTexture'), 0);
    gl.uniform2f(gl.getUniformLocation(blurShader, 'uOffset'), dx, dy);
    gl.disable(gl.BLEND);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    if (aPos > -1) gl.disableVertexAttribArray(aPos);
}

/** Pass 5: tonemapped sum of line + tight + big glow to the default fbo. */
export function compositePass(ctx: ScopeXyContext) {
    const { gl, canvas, compositeShader } = ctx;
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.useProgram(compositeShader);
    const aPos = bindFullscreenQuad(ctx, compositeShader);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, ctx.lineFbo.tex);
    gl.uniform1i(gl.getUniformLocation(compositeShader, 'uLine'), 0);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, ctx.tightFboA.tex);
    gl.uniform1i(gl.getUniformLocation(compositeShader, 'uTight'), 1);
    gl.activeTexture(gl.TEXTURE2);
    gl.bindTexture(gl.TEXTURE_2D, ctx.bigFboA.tex);
    gl.uniform1i(gl.getUniformLocation(compositeShader, 'uBig'), 2);
    gl.uniform1f(
        gl.getUniformLocation(compositeShader, 'uExposure'),
        ctx.exposure,
    );
    gl.uniform3f(
        gl.getUniformLocation(compositeShader, 'uBackground'),
        ctx.backgroundColor[0],
        ctx.backgroundColor[1],
        ctx.backgroundColor[2],
    );
    gl.disable(gl.BLEND);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    if (aPos > -1) gl.disableVertexAttribArray(aPos);
}
