import {
    fsBlur,
    fsLine,
    fsOutput,
    vsBlur,
    vsLine,
    vsOutput,
} from './shaders';

/**
 * Maximum number of overlaid Lissajous traces. Matches PORT_MAX_CHANNELS so a
 * fully-polyphonic source paints every channel.
 */
export const MAX_TRACES = 16;

/**
 * Samples per trace per frame — matches the audio-thread ring buffer capacity.
 * Keep in sync with `SCOPE_XY_CAPACITY` in `crates/modular/src/audio.rs`.
 */
export const SCOPE_XY_CAPACITY = 2048;

export interface ScopeXYPairData {
    x: Float32Array;
    y: Float32Array;
    /** Index into the ring buffer of the most recently written sample (one past the newest). */
    head: number;
    /** Optional per-axis voltage windows (`[min, max]`). Falls back to `[-5, 5]`. */
    xRange?: readonly [number, number];
    yRange?: readonly [number, number];
}

export interface WoscopeOptions {
    /** Beam colour in [0, 1] linear-ish space. Default neon green. */
    color?: [number, number, number];
    /** Background tint added before the additive beam. Default near-black. */
    background?: [number, number, number];
    /** Beam half-width in clip-space units (will scale with canvas size). */
    beamSize?: number;
    /** Beam intensity multiplier. */
    intensity?: number;
    /** Bloom strength applied after the blur passes. */
    bloomIntensity?: number;
}

export interface Woscope {
    draw(pairs: ScopeXYPairData[]): void;
    resize(width: number, height: number): void;
    dispose(): void;
}

const SEGMENTS_PER_TRACE = SCOPE_XY_CAPACITY; // one segment per sample (wraps)
const VERTS_PER_SEGMENT = 4;

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

interface ChannelFramebuffer {
    fbo: WebGLFramebuffer;
    tex: WebGLTexture;
    width: number;
    height: number;
}

function createFloatFbo(
    gl: WebGLRenderingContext,
    halfFloatType: number | null,
    width: number,
    height: number,
): ChannelFramebuffer {
    const tex = gl.createTexture();
    if (!tex) throw new Error('createTexture failed');
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const type = halfFloatType ?? gl.UNSIGNED_BYTE;
    gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        width,
        height,
        0,
        gl.RGBA,
        type,
        null,
    );

    const fbo = gl.createFramebuffer();
    if (!fbo) throw new Error('createFramebuffer failed');
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferTexture2D(
        gl.FRAMEBUFFER,
        gl.COLOR_ATTACHMENT0,
        gl.TEXTURE_2D,
        tex,
        0,
    );
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return { fbo, tex, width, height };
}

export function createWoscope(
    canvas: HTMLCanvasElement,
    options: WoscopeOptions = {},
): Woscope {
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
    gl.getExtension('OES_standard_derivatives');
    const halfFloatExt = gl.getExtension('OES_texture_half_float');
    const halfFloatLinear = gl.getExtension('OES_texture_half_float_linear');
    const colorBufferHalfFloat = gl.getExtension(
        'EXT_color_buffer_half_float',
    );
    const halfFloatType =
        halfFloatExt && (halfFloatLinear || colorBufferHalfFloat)
            ? halfFloatExt.HALF_FLOAT_OES
            : null;
    if (!halfFloatType) {
        console.warn(
            'woscope: half-float textures unavailable; falling back to RGBA8 (will band on long persistence).',
        );
    }

    const color = options.color ?? [0.05, 1.0, 0.35];
    const background = options.background ?? [0.005, 0.008, 0.012];
    const beamSize = options.beamSize ?? 0.012;
    const intensity = options.intensity ?? 0.18;
    const bloomIntensity = options.bloomIntensity ?? 0.65;

    const lineProgram = link(gl, vsLine, fsLine);
    const blurProgram = link(gl, vsBlur, fsBlur);
    const outputProgram = link(gl, vsOutput, fsOutput);

    // Static index/vertex buffers for line segments. One quad per sample-segment.
    const indexData = new Uint16Array(SEGMENTS_PER_TRACE * 6);
    for (let s = 0; s < SEGMENTS_PER_TRACE; s++) {
        const base = s * VERTS_PER_SEGMENT;
        const o = s * 6;
        indexData[o + 0] = base + 0;
        indexData[o + 1] = base + 1;
        indexData[o + 2] = base + 2;
        indexData[o + 3] = base + 2;
        indexData[o + 4] = base + 1;
        indexData[o + 5] = base + 3;
    }
    const indexBuf = gl.createBuffer();
    if (!indexBuf) throw new Error('createBuffer failed');
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuf);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indexData, gl.STATIC_DRAW);

    // aIdx is a constant 0..3 cycling array — same for every segment.
    const idxData = new Float32Array(SEGMENTS_PER_TRACE * VERTS_PER_SEGMENT);
    for (let i = 0; i < idxData.length; i++) {
        idxData[i] = i % 4;
    }
    const idxBuf = gl.createBuffer();
    if (!idxBuf) throw new Error('createBuffer failed');
    gl.bindBuffer(gl.ARRAY_BUFFER, idxBuf);
    gl.bufferData(gl.ARRAY_BUFFER, idxData, gl.STATIC_DRAW);

    // Per-frame start/end vertex buffers — re-uploaded for each trace.
    const startBuf = gl.createBuffer();
    const endBuf = gl.createBuffer();
    if (!startBuf || !endBuf) throw new Error('createBuffer failed');
    const startScratch = new Float32Array(
        SEGMENTS_PER_TRACE * VERTS_PER_SEGMENT * 2,
    );
    const endScratch = new Float32Array(
        SEGMENTS_PER_TRACE * VERTS_PER_SEGMENT * 2,
    );
    gl.bindBuffer(gl.ARRAY_BUFFER, startBuf);
    gl.bufferData(gl.ARRAY_BUFFER, startScratch.byteLength, gl.DYNAMIC_DRAW);
    gl.bindBuffer(gl.ARRAY_BUFFER, endBuf);
    gl.bufferData(gl.ARRAY_BUFFER, endScratch.byteLength, gl.DYNAMIC_DRAW);

    // Fullscreen quad for blur + output passes.
    const quadBuf = gl.createBuffer();
    if (!quadBuf) throw new Error('createBuffer failed');
    gl.bindBuffer(gl.ARRAY_BUFFER, quadBuf);
    gl.bufferData(
        gl.ARRAY_BUFFER,
        new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
        gl.STATIC_DRAW,
    );

    let lineFbo = createFloatFbo(
        gl,
        halfFloatType,
        canvas.width,
        canvas.height,
    );
    let blurFboA = createFloatFbo(
        gl,
        halfFloatType,
        canvas.width,
        canvas.height,
    );
    let blurFboB = createFloatFbo(
        gl,
        halfFloatType,
        canvas.width,
        canvas.height,
    );

    function resize(width: number, height: number) {
        const w = Math.max(1, Math.floor(width));
        const h = Math.max(1, Math.floor(height));
        canvas.width = w;
        canvas.height = h;
        for (const f of [lineFbo, blurFboA, blurFboB]) {
            gl!.deleteFramebuffer(f.fbo);
            gl!.deleteTexture(f.tex);
        }
        lineFbo = createFloatFbo(gl!, halfFloatType, w, h);
        blurFboA = createFloatFbo(gl!, halfFloatType, w, h);
        blurFboB = createFloatFbo(gl!, halfFloatType, w, h);
    }

    function fillSegmentBuffers(
        pair: ScopeXYPairData,
        xMin: number,
        xSpan: number,
        yMin: number,
        ySpan: number,
    ): number {
        const len = Math.min(
            SEGMENTS_PER_TRACE,
            pair.x.length,
            pair.y.length,
        );
        if (len < 2) return 0;
        const head = pair.head;
        let startWrite = 0;
        let endWrite = 0;
        for (let s = 0; s < len - 1; s++) {
            const i0 = (head + s) % len;
            const i1 = (head + s + 1) % len;
            // Map [min,max] → [-1, 1].
            const sx = ((pair.x[i0] - xMin) / xSpan) * 2 - 1;
            const sy = ((pair.y[i0] - yMin) / ySpan) * 2 - 1;
            const ex = ((pair.x[i1] - xMin) / xSpan) * 2 - 1;
            const ey = ((pair.y[i1] - yMin) / ySpan) * 2 - 1;
            for (let v = 0; v < VERTS_PER_SEGMENT; v++) {
                startScratch[startWrite++] = sx;
                startScratch[startWrite++] = sy;
                endScratch[endWrite++] = ex;
                endScratch[endWrite++] = ey;
            }
        }
        return len - 1;
    }

    function bindAttrib(name: string, program: WebGLProgram, size: number) {
        const loc = gl!.getAttribLocation(program, name);
        if (loc < 0) return;
        gl!.enableVertexAttribArray(loc);
        gl!.vertexAttribPointer(loc, size, gl!.FLOAT, false, 0, 0);
    }

    function draw(pairs: ScopeXYPairData[]) {
        if (canvas.width === 0 || canvas.height === 0) return;
        gl!.bindFramebuffer(gl!.FRAMEBUFFER, lineFbo.fbo);
        gl!.viewport(0, 0, lineFbo.width, lineFbo.height);
        gl!.clearColor(0, 0, 0, 1);
        gl!.clear(gl!.COLOR_BUFFER_BIT);

        if (pairs.length > 0) {
            gl!.useProgram(lineProgram);
            gl!.uniform1f(
                gl!.getUniformLocation(lineProgram, 'uSize'),
                beamSize,
            );
            gl!.uniform1f(
                gl!.getUniformLocation(lineProgram, 'uIntensity'),
                intensity,
            );
            gl!.enable(gl!.BLEND);
            gl!.blendFunc(gl!.SRC_ALPHA, gl!.ONE);

            gl!.bindBuffer(gl!.ELEMENT_ARRAY_BUFFER, indexBuf);

            gl!.bindBuffer(gl!.ARRAY_BUFFER, idxBuf);
            bindAttrib('aIdx', lineProgram, 1);

            const drawCount = Math.min(pairs.length, MAX_TRACES);
            for (let p = 0; p < drawCount; p++) {
                const pair = pairs[p];
                const xRange = pair.xRange ?? [-5, 5];
                const yRange = pair.yRange ?? [-5, 5];
                const xMin = xRange[0];
                const xSpan = Math.max(xRange[1] - xRange[0], 1e-6);
                const yMin = yRange[0];
                const ySpan = Math.max(yRange[1] - yRange[0], 1e-6);
                const segCount = fillSegmentBuffers(
                    pair,
                    xMin,
                    xSpan,
                    yMin,
                    ySpan,
                );
                if (segCount === 0) continue;
                gl!.bindBuffer(gl!.ARRAY_BUFFER, startBuf);
                gl!.bufferSubData(
                    gl!.ARRAY_BUFFER,
                    0,
                    startScratch.subarray(0, segCount * VERTS_PER_SEGMENT * 2),
                );
                bindAttrib('aStart', lineProgram, 2);

                gl!.bindBuffer(gl!.ARRAY_BUFFER, endBuf);
                gl!.bufferSubData(
                    gl!.ARRAY_BUFFER,
                    0,
                    endScratch.subarray(0, segCount * VERTS_PER_SEGMENT * 2),
                );
                bindAttrib('aEnd', lineProgram, 2);

                gl!.bindBuffer(gl!.ARRAY_BUFFER, idxBuf);
                bindAttrib('aIdx', lineProgram, 1);

                gl!.drawElements(
                    gl!.TRIANGLES,
                    segCount * 6,
                    gl!.UNSIGNED_SHORT,
                    0,
                );
            }
            gl!.disable(gl!.BLEND);
        }

        // Bloom: horizontal pass into A, vertical pass into B.
        gl!.useProgram(blurProgram);
        gl!.bindBuffer(gl!.ARRAY_BUFFER, quadBuf);
        bindAttrib('aPos', blurProgram, 2);

        gl!.bindFramebuffer(gl!.FRAMEBUFFER, blurFboA.fbo);
        gl!.viewport(0, 0, blurFboA.width, blurFboA.height);
        gl!.clear(gl!.COLOR_BUFFER_BIT);
        gl!.activeTexture(gl!.TEXTURE0);
        gl!.bindTexture(gl!.TEXTURE_2D, lineFbo.tex);
        gl!.uniform1i(gl!.getUniformLocation(blurProgram, 'uTex'), 0);
        gl!.uniform2f(
            gl!.getUniformLocation(blurProgram, 'uDirection'),
            1 / lineFbo.width,
            0,
        );
        gl!.uniform1f(
            gl!.getUniformLocation(blurProgram, 'uIntensity'),
            bloomIntensity,
        );
        gl!.drawArrays(gl!.TRIANGLE_STRIP, 0, 4);

        gl!.bindFramebuffer(gl!.FRAMEBUFFER, blurFboB.fbo);
        gl!.viewport(0, 0, blurFboB.width, blurFboB.height);
        gl!.clear(gl!.COLOR_BUFFER_BIT);
        gl!.bindTexture(gl!.TEXTURE_2D, blurFboA.tex);
        gl!.uniform2f(
            gl!.getUniformLocation(blurProgram, 'uDirection'),
            0,
            1 / blurFboB.height,
        );
        gl!.uniform1f(gl!.getUniformLocation(blurProgram, 'uIntensity'), 1.0);
        gl!.drawArrays(gl!.TRIANGLE_STRIP, 0, 4);

        // Composite to default framebuffer.
        gl!.bindFramebuffer(gl!.FRAMEBUFFER, null);
        gl!.viewport(0, 0, canvas.width, canvas.height);
        gl!.clearColor(background[0], background[1], background[2], 1);
        gl!.clear(gl!.COLOR_BUFFER_BIT);

        gl!.useProgram(outputProgram);
        gl!.bindBuffer(gl!.ARRAY_BUFFER, quadBuf);
        bindAttrib('aPos', outputProgram, 2);
        gl!.activeTexture(gl!.TEXTURE0);
        gl!.bindTexture(gl!.TEXTURE_2D, lineFbo.tex);
        gl!.uniform1i(gl!.getUniformLocation(outputProgram, 'uLine'), 0);
        gl!.activeTexture(gl!.TEXTURE1);
        gl!.bindTexture(gl!.TEXTURE_2D, blurFboB.tex);
        gl!.uniform1i(gl!.getUniformLocation(outputProgram, 'uBloom'), 1);
        gl!.uniform3f(
            gl!.getUniformLocation(outputProgram, 'uColor'),
            color[0],
            color[1],
            color[2],
        );
        gl!.uniform3f(
            gl!.getUniformLocation(outputProgram, 'uBackground'),
            background[0],
            background[1],
            background[2],
        );
        gl!.drawArrays(gl!.TRIANGLE_STRIP, 0, 4);
    }

    function dispose() {
        for (const fbo of [lineFbo, blurFboA, blurFboB]) {
            gl!.deleteFramebuffer(fbo.fbo);
            gl!.deleteTexture(fbo.tex);
        }
        gl!.deleteBuffer(indexBuf);
        gl!.deleteBuffer(idxBuf);
        gl!.deleteBuffer(startBuf);
        gl!.deleteBuffer(endBuf);
        gl!.deleteBuffer(quadBuf);
        gl!.deleteProgram(lineProgram);
        gl!.deleteProgram(blurProgram);
        gl!.deleteProgram(outputProgram);
    }

    return { draw, resize, dispose };
}
