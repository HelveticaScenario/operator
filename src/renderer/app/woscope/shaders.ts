// GLSL ports of m1el/woscope (MIT-licensed).
// https://github.com/m1el/woscope — Gaussian-integral beam, separable bloom,
// additive composite. Comments inline where the math diverges from intuition.

export const vsLine = `\
precision highp float;
attribute vec2 aStart;
attribute vec2 aEnd;
attribute float aIdx;
uniform float uSize;
uniform float uIntensity;
varying vec4 vUvl;

void main () {
    float tang;
    float norm;
    float idx = mod(aIdx, 4.0);
    if (idx == 0.0)      { tang = -1.0; norm = -1.0; }
    else if (idx == 1.0) { tang =  1.0; norm = -1.0; }
    else if (idx == 2.0) { tang = -1.0; norm =  1.0; }
    else                 { tang =  1.0; norm =  1.0; }

    vec2 dir = aEnd - aStart;
    float len = length(dir);
    vec2 t = (len > 0.0) ? (dir / len) : vec2(1.0, 0.0);
    vec2 n = vec2(-t.y, t.x);

    vec2 base = (idx < 2.0) ? aStart : aEnd;
    vec2 pos = base + (tang * t + norm * n) * uSize;
    gl_Position = vec4(pos, 0.0, 1.0);

    // u = signed length along the segment (in beam-size units),
    // v = signed perpendicular offset,
    // l = segment length,
    // w = afterglow weight (intensity scaled by velocity factor).
    vUvl = vec4(
        tang * (len * 0.5 + uSize) / uSize,
        norm,
        len / uSize,
        uIntensity
    );
}
`;

export const fsLine = `\
#extension GL_OES_standard_derivatives : enable
precision highp float;
varying vec4 vUvl;

// Approximation of 0.5 * (erf(x / sqrt(2)) + 1).
// Hastings-style poly — same one woscope uses.
float gaussIntegral(float x) {
    float t = 1.0 / (1.0 + 0.47047 * abs(x));
    float ex = exp(-x * x);
    float poly = t * (0.3480242 + t * (-0.0958798 + t * 0.7478556));
    float r = 1.0 - poly * ex;
    return 0.5 + 0.5 * (x < 0.0 ? -r : r);
}

void main() {
    float len = vUvl.z;
    float u = vUvl.x;
    float v = vUvl.y;
    // Slow beam = bright beam: divide by approximate segment length to
    // give long fast segments lower per-pixel intensity than tight curves.
    float invLen = 1.0 / max(len, 0.0001);
    float gauss = exp(-v * v * 0.5);
    float endsX = max(u + 0.5 * len, 0.0) - max(u - 0.5 * len, 0.0);
    float intensity = vUvl.w * gauss * endsX * invLen;
    gl_FragColor = vec4(intensity, intensity, intensity, 1.0);
}
`;

// Separable 9-tap Gaussian blur (weights 1/2/3/4/5/4/3/2/1, sum = 25).
// `uDirection` flips between horizontal and vertical pass.
export const vsBlur = `\
precision highp float;
attribute vec2 aPos;
varying vec2 vUv;
void main() {
    vUv = 0.5 * (aPos + vec2(1.0, 1.0));
    gl_Position = vec4(aPos, 0.0, 1.0);
}
`;

export const fsBlur = `\
precision highp float;
varying vec2 vUv;
uniform sampler2D uTex;
uniform vec2 uDirection;     // (1/w, 0) or (0, 1/h) — one texel along blur axis
uniform float uIntensity;    // bloom strength

void main() {
    vec4 acc = vec4(0.0);
    acc += texture2D(uTex, vUv - 4.0 * uDirection) * 1.0;
    acc += texture2D(uTex, vUv - 3.0 * uDirection) * 2.0;
    acc += texture2D(uTex, vUv - 2.0 * uDirection) * 3.0;
    acc += texture2D(uTex, vUv - 1.0 * uDirection) * 4.0;
    acc += texture2D(uTex, vUv)                     * 5.0;
    acc += texture2D(uTex, vUv + 1.0 * uDirection) * 4.0;
    acc += texture2D(uTex, vUv + 2.0 * uDirection) * 3.0;
    acc += texture2D(uTex, vUv + 3.0 * uDirection) * 2.0;
    acc += texture2D(uTex, vUv + 4.0 * uDirection) * 1.0;
    gl_FragColor = acc * (uIntensity / 25.0);
}
`;

// Additive composite of line + bloom over a tinted background.
export const vsOutput = `\
precision highp float;
attribute vec2 aPos;
varying vec2 vUv;
void main() {
    vUv = 0.5 * (aPos + vec2(1.0, 1.0));
    gl_Position = vec4(aPos, 0.0, 1.0);
}
`;

export const fsOutput = `\
precision highp float;
varying vec2 vUv;
uniform sampler2D uLine;
uniform sampler2D uBloom;
uniform vec3 uColor;
uniform vec3 uBackground;

void main() {
    float line  = texture2D(uLine, vUv).r;
    float bloom = texture2D(uBloom, vUv).r;
    vec3 lit = (line + bloom) * uColor;
    vec3 col = uBackground + lit;
    gl_FragColor = vec4(col, 1.0);
}
`;
