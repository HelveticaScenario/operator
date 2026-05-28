// XY scope shaders. Renders pairs of voltage samples as an additive
// gaussian-line trace on a phosphor-style background. Attribution: line
// geometry/integral adapted from m1el/woscope (MIT,
// https://github.com/m1el/woscope); persistence pass, dual-stage gaussian
// bloom, and exposure-mapped composite adapted from dood.al/oscilloscope by
// Neil Thapen (https://dood.al/oscilloscope/).

// GPU Lanczos upsampler. Per-frame: a 2048×1 RGBA-float texture is uploaded
// with the raw ring (x in .r, y in .g). Each vertex's output sample index
// drives a 15-tap (2a-1) sinc-windowed sum to interpolate `STEPS` outputs per
// input sample. Keeps CPU work to just an interleave + texSubImage2D.
export const vsLine = `\
precision highp float;
#define EPS 1E-6
#define STEPS 6
#define RADIUS 8
#define KERNEL_LEN 48
#define BUFFER_SIZE 2048.0
uniform float uSize;
uniform sampler2D uSamples;
uniform float uKernel[KERNEL_LEN];
uniform vec2 uXRange; // (min, span)
uniform vec2 uYRange;
uniform float uUpsample; // 1.0 = Lanczos, 0.0 = nearest input sample
attribute float aIdx;
varying vec4 uvl;

vec2 fetchSample(float inPos) {
    float clamped = clamp(inPos, 0.0, BUFFER_SIZE - 1.0);
    return texture2D(uSamples, vec2((clamped + 0.5) / BUFFER_SIZE, 0.5)).rg;
}

vec2 lanczosFetch(float outSampleIdx) {
    float inPos = floor(outSampleIdx / float(STEPS));
    int frac = int(mod(outSampleIdx, float(STEPS)));
    if (uUpsample < 0.5) return fetchSample(inPos);
    if (frac == 0) return fetchSample(inPos);
    vec2 sum = vec2(0.0);
    for (int s = -RADIUS + 1; s < RADIUS; s++) {
        vec2 sample = fetchSample(inPos + float(s));
        int kernelPos = -frac + s * STEPS;
        int absK = kernelPos < 0 ? -kernelPos : kernelPos;
        sum += sample * uKernel[absK];
    }
    // Clamp to the 4-sample neighborhood hull. Lanczos' negative side
    // lobes cause big overshoots at fast transitions, which on the XY
    // scope read as stray straight strokes extending past the figure.
    // Clamping preserves curvature inside the neighborhood while
    // killing the runaway overshoots.
    vec2 p0 = fetchSample(inPos - 1.0);
    vec2 p1 = fetchSample(inPos);
    vec2 p2 = fetchSample(inPos + 1.0);
    vec2 p3 = fetchSample(inPos + 2.0);
    vec2 mn = min(min(p0, p1), min(p2, p3));
    vec2 mx = max(max(p0, p1), max(p2, p3));
    return clamp(sum, mn, mx);
}

vec2 voltToClip(vec2 v) {
    return (v - vec2(uXRange.x, uYRange.x))
         / vec2(uXRange.y, uYRange.y) * 2.0 - 1.0;
}

uniform float uIntensity;
uniform float uFadeAmount;
uniform float uNumSamples;

void main () {
    float idx = mod(aIdx, 4.0);
    float outIdx = floor(aIdx / 4.0);

    vec2 startClip = voltToClip(lanczosFetch(outIdx));
    vec2 endClip   = voltToClip(lanczosFetch(outIdx + 1.0));

    vec2 dir = endClip - startClip;
    float len = length(dir);
    if (len > EPS) dir = dir / len;
    else dir = vec2(1.0, 0.0);
    vec2 norm = vec2(-dir.y, dir.x);

    // uvl.xy carries signed distances in clip-space units (x along the
    // segment, y perpendicular). fsLine plugs them straight into the
    // gaussian-line integral without further remapping.
    float tang;
    vec2 current;
    if (idx >= 2.0) {
        current = endClip;
        tang = 1.0;
        uvl.x = -uSize;
    } else {
        current = startClip;
        tang = -1.0;
        uvl.x = len + uSize;
    }
    float side = (mod(idx, 2.0) - 0.5) * 2.0;
    uvl.y = side * uSize;
    uvl.z = len;
    // Per-vertex brightness baked here so the fragment shader only needs
    // to multiply, not recompute the ramp per pixel. Oldest sample in the
    // ring is dimmed to (1 - uFadeAmount) of full intensity; newest is
    // full.
    uvl.w = uIntensity * mix(1.0 - uFadeAmount, 1.0, outIdx / uNumSamples);

    gl_Position = vec4(current + (tang * dir + norm * side) * uSize, 0.0, 1.0);
}
`;

export const fsLine = `\
precision highp float;
#define EPS 1E-6
#define SQRT2 1.4142135623730951
uniform float uSize;
uniform vec4 uColor;
varying vec4 uvl;
float erf(float x) {
    float s = sign(x), a = abs(x);
    x = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * a) * a) * a) * a;
    x *= x;
    return s - s / (x * x);
}
void main (void)
{
    float len = uvl.z;
    // uvl.xy is already in clip-space distance units (x along segment,
    // y perpendicular).
    vec2 xy = uvl.xy;
    float alpha;
    // sigma = uSize/5 keeps the beam tight enough that junction overlaps
    // between consecutive quads don't pile excess energy on top of each
    // other; wider sigma reads as a fat smudge.
    float sigma = uSize / 5.0;
    if (len < EPS) {
        alpha = exp(-dot(xy, xy) / (2.0 * sigma * sigma)) / (2.0 * sqrt(uSize));
    } else {
        // Analytic gaussian-line integral: convolution of a unit gaussian
        // with the segment [0, len] along its x axis, evaluated at xy.
        alpha = erf(xy.x / SQRT2 / sigma)
              - erf((xy.x - len) / SQRT2 / sigma);
        alpha *= exp(-xy.y * xy.y / (2.0 * sigma * sigma)) / 2.0 / len;
    }
    // Per-vertex brightness (afterglow × intensity) is already baked into
    // uvl.w by vsLine.
    alpha *= uvl.w;
    gl_FragColor = vec4(vec3(uColor), uColor.a * alpha);
}
`;

// Fullscreen quad — shared by fade, copy, blur, and composite passes.
export const vsQuad = `\
precision highp float;
attribute vec2 aPos;
varying vec2 vTexCoord;
void main (void) {
    gl_Position = vec4(aPos, 0.0, 1.0);
    vTexCoord = 0.5 * aPos + 0.5;
}
`;

// Solid-colour quad. Used to decay lineFbo by alpha-blending a translucent
// dark colour over the previous frame's accumulated trace.
export const fsFade = `\
precision highp float;
uniform vec4 uColor;
void main (void) {
    gl_FragColor = uColor;
}
`;

// Plain bilinear copy. Drives the half-res and eighth-res downsamples.
export const fsCopy = `\
precision highp float;
uniform sampler2D uTexture;
varying vec2 vTexCoord;
void main (void) {
    gl_FragColor = texture2D(uTexture, vTexCoord);
    gl_FragColor.a = 1.0;
}
`;

// 17-tap separable gaussian blur (sigma ≈ 3). Real gaussian weights so the
// bloom halo has circular iso-contours; triangle weights would give the
// halo square corners.
export const fsBlur = `\
precision highp float;
uniform sampler2D uTexture;
uniform vec2 uOffset;
varying vec2 vTexCoord;
void main (void)
{
    vec4 sum = vec4(0.0);
    sum += texture2D(uTexture, vTexCoord - uOffset*8.0) * 0.000078;
    sum += texture2D(uTexture, vTexCoord - uOffset*7.0) * 0.000489;
    sum += texture2D(uTexture, vTexCoord - uOffset*6.0) * 0.002403;
    sum += texture2D(uTexture, vTexCoord - uOffset*5.0) * 0.009245;
    sum += texture2D(uTexture, vTexCoord - uOffset*4.0) * 0.027835;
    sum += texture2D(uTexture, vTexCoord - uOffset*3.0) * 0.065592;
    sum += texture2D(uTexture, vTexCoord - uOffset*2.0) * 0.120980;
    sum += texture2D(uTexture, vTexCoord - uOffset*1.0) * 0.174670;
    sum += texture2D(uTexture, vTexCoord                ) * 0.197420;
    sum += texture2D(uTexture, vTexCoord + uOffset*1.0) * 0.174670;
    sum += texture2D(uTexture, vTexCoord + uOffset*2.0) * 0.120980;
    sum += texture2D(uTexture, vTexCoord + uOffset*3.0) * 0.065592;
    sum += texture2D(uTexture, vTexCoord + uOffset*4.0) * 0.027835;
    sum += texture2D(uTexture, vTexCoord + uOffset*5.0) * 0.009245;
    sum += texture2D(uTexture, vTexCoord + uOffset*6.0) * 0.002403;
    sum += texture2D(uTexture, vTexCoord + uOffset*7.0) * 0.000489;
    sum += texture2D(uTexture, vTexCoord + uOffset*8.0) * 0.000078;
    gl_FragColor = sum;
}
`;

// Final composite: line + tight glow + big glow, tonemapped through
// 1 - exp(-uExposure*L) to compress the unbounded additive accumulation
// back into [0, 1]. Bright pixels mix toward white for the phosphor
// over-saturation feel; uBackground adds the canvas tint.
export const fsComposite = `\
precision highp float;
uniform sampler2D uLine;
uniform sampler2D uTight;
uniform sampler2D uBig;
uniform float uExposure;
uniform vec3 uBackground;
varying vec2 vTexCoord;
void main (void) {
    vec3 line  = texture2D(uLine,  vTexCoord).rgb;
    vec3 tight = texture2D(uTight, vTexCoord).rgb;
    vec3 big   = texture2D(uBig,   vTexCoord).rgb;
    vec3 light = line + 1.5 * tight + 0.4 * big;
    vec3 mapped = vec3(1.0) - exp(-uExposure * light);
    float bright = max(mapped.r, max(mapped.g, mapped.b));
    float wash = pow(bright, 4.0) * 0.5;
    vec3 outColor = mix(mapped, vec3(1.0), wash);
    gl_FragColor = vec4(outColor + uBackground, 1.0);
}
`;
