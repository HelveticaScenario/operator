// Ports of m1el/woscope (MIT) and dood.al/oscilloscope by Neil Thapen.
//   woscope: https://github.com/m1el/woscope
//   dood.al: https://dood.al/oscilloscope/
//
// The line geometry/integral is woscope's; persistence, dual-stage bloom,
// 17-tap real-gaussian blur, and exposure-mapped composite are from dood.al.
// We drop dood.al's noise-jpg screen texture and its hue control (the beam
// colour comes from the host theme instead).

export const vsLine = `\
precision highp float;
#define EPS 1E-6
uniform float uSize;
attribute vec2 aStart, aEnd;
attribute float aIdx;
varying vec4 uvl;
void main () {
    float tang;
    vec2 current;
    float idx = mod(aIdx,4.0);
    if (idx >= 2.0) {
        current = aEnd;
        tang = 1.0;
    } else {
        current = aStart;
        tang = -1.0;
    }
    float side = (mod(idx, 2.0)-0.5)*2.0;
    uvl.xy = vec2(tang, side);
    uvl.w = floor(aIdx / 4.0 + 0.5);

    vec2 dir = aEnd-aStart;
    uvl.z = length(dir);
    if (uvl.z > EPS) {
        dir = dir / uvl.z;
    } else {
        dir = vec2(1.0, 0.0);
    }
    vec2 norm = vec2(-dir.y, dir.x);
    gl_Position = vec4(current+(tang*dir+norm*side)*uSize,0.0,1.0);
}
`;

export const fsLine = `\
precision highp float;
#define EPS 1E-6
#define SQRT2 1.4142135623730951
uniform float uSize;
uniform float uIntensity;
uniform float uFadeAmount;
uniform float uNumSamples;
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
    vec2 xy = vec2((len/2.0+uSize)*uvl.x+len/2.0, uSize*uvl.y);
    float alpha;

    float sigma = uSize/4.0;
    if (len < EPS) {
        alpha = exp(-pow(length(xy),2.0)/(2.0*sigma*sigma))/2.0/sqrt(uSize);
    } else {
        alpha = erf((len-xy.x)/SQRT2/sigma) + erf(xy.x/SQRT2/sigma);
        alpha *= exp(-xy.y*xy.y/(2.0*sigma*sigma))/2.0/len*uSize;
    }
    // dood.al-style intra-frame gradient: oldest samples in the ring fade
    // toward (1 - uFadeAmount) so the trailing edge dims smoothly even
    // before the persistence pass kicks in.
    float afterglow = mix(1.0 - uFadeAmount, 1.0, uvl.w / uNumSamples);
    alpha *= afterglow * uIntensity;
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

// dood.al's 17-tap separable Gaussian (sigma ≈ 3). Real gaussian weights
// (not the triangle 1/2/3/4/5 woscope ships with) so the bloom has circular
// iso-contours.
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

// Final composite: line + tight glow + big glow tonemapped via 1-exp(-uExp*L)
// (dood.al's curve). Bright pixels mix toward white for the phosphor over-
// saturation feel; uBackground adds the canvas tint.
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
