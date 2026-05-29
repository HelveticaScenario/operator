// Low-level WebGL helpers for the XY scope pipeline: shader compile/link and
// RGBA-UNSIGNED_BYTE framebuffer objects. Pure functions over a passed `gl`
// context — no pipeline state.

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

export function link(
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

export interface Fbo {
    fb: WebGLFramebuffer;
    tex: WebGLTexture;
    width: number;
    height: number;
}

export function createFbo(
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

export function disposeFbo(gl: WebGLRenderingContext, f: Fbo) {
    gl.deleteFramebuffer(f.fb);
    gl.deleteTexture(f.tex);
}
