// Beam geometry and the gaussian-line integral are adapted from m1el/woscope
// (MIT, https://github.com/m1el/woscope); the persistence fade, dual-stage
// gaussian bloom and exposure-mapped composite from dood.al/oscilloscope by
// Neil Thapen (https://dood.al/oscilloscope/).
import {
    MAX_SCOPE_TRACES,
    VIDEO_HISTORY_LEN,
} from '../../shared/video/videoGraph';

/** Interpolated points drawn per sample when a scope upsamples. */
export const UPSAMPLE_STEPS = 6;
/** Half-width of the Lanczos window, in samples. */
const UPSAMPLE_RADIUS = 8;
/** Sharpens the window's falloff beyond plain Lanczos. */
const LANCZOS_TWEAK = 1.5;
const KERNEL_LEN = UPSAMPLE_RADIUS * UPSAMPLE_STEPS;

/** Most segments a trace has: every sample of a full history, upsampled. */
export const MAX_SEGMENTS = (VIDEO_HISTORY_LEN - 1) * UPSAMPLE_STEPS;

/**
 * The beam's uniform block, in floats: the volt windows, the beam, the
 * upsampling switch, the Lanczos kernel, each trace's history rows as `u32`
 * pairs, then each trace's first real sample.
 */
export const KERNEL_OFFSET = 12;
export const ROWS_OFFSET = KERNEL_OFFSET + KERNEL_LEN;
export const STARTS_OFFSET = ROWS_OFFSET + MAX_SCOPE_TRACES * 2;
export const BEAM_UNIFORM_FLOATS = STARTS_OFFSET + MAX_SCOPE_TRACES;

/** Lanczos weights at each `1 / UPSAMPLE_STEPS` of a sample from the center. */
export function lanczosKernel(): Float32Array {
    const kernel = new Float32Array(KERNEL_LEN);
    kernel[0] = 1;
    for (let i = 1; i < KERNEL_LEN; i++) {
        const x = (Math.PI * i) / UPSAMPLE_STEPS;
        const window = (UPSAMPLE_RADIUS * Math.sin(x / UPSAMPLE_RADIUS)) / x;
        kernel[i] = (Math.sin(x) / x) * Math.pow(window, LANCZOS_TWEAK);
    }
    return kernel;
}

/**
 * Draws the beam: each segment between consecutive points is a quad lit by
 * the integral of a gaussian along it, added into the trace.
 */
export const BEAM_WGSL = /* wgsl */ `
struct Scope {
    // xMin, xSpan, yMin, ySpan in volts.
    window: vec4f,
    // Beam half-width in clip units of the frame's height, gain, fade per
    // frame, samples.
    beam: vec4f,
    // Interpolated points per sample, segments per trace, frame aspect ratio.
    steps: vec4f,
    kernel: array<vec4f, ${KERNEL_LEN / 4}>,
    rows: array<vec4u, ${MAX_SCOPE_TRACES / 2}>,
    // The index of each trace's first real sample; those before it come
    // from before its signal started.
    starts: array<vec4f, ${MAX_SCOPE_TRACES / 4}>,
}

@group(0) @binding(0) var<uniform> s: Scope;
@group(0) @binding(1) var history_tex: texture_2d<f32>;

const RADIUS: i32 = ${UPSAMPLE_RADIUS};
const EPS: f32 = 1e-6;
const SQRT2: f32 = 1.4142135623730951;

// The first real sample of the trace being drawn.
var<private> first_sample: f32;

fn sample_at(rows: vec2u, index: f32) -> vec2f {
    let i = i32(clamp(index, first_sample, s.beam.w - 1.0));
    return vec2f(
        textureLoad(history_tex, vec2i(i, i32(rows.x)), 0).r,
        textureLoad(history_tex, vec2i(i, i32(rows.y)), 0).r,
    );
}

fn kernel_at(k: i32) -> f32 {
    return s.kernel[k >> 2u][k & 3];
}

// The point a fraction of the way between samples, held inside the box of
// its four nearest samples so the kernel's side lobes cannot overshoot.
fn point_at(rows: vec2u, out_index: f32) -> vec2f {
    let steps = i32(s.steps.x);
    let base = floor(out_index / f32(steps));
    let frac = i32(out_index) % steps;
    if (frac == 0) {
        return sample_at(rows, base);
    }
    var sum = vec2f(0.0);
    for (var k = 1 - RADIUS; k < RADIUS; k++) {
        sum += sample_at(rows, base + f32(k)) * kernel_at(abs(k * steps - frac));
    }
    let p0 = sample_at(rows, base - 1.0);
    let p1 = sample_at(rows, base);
    let p2 = sample_at(rows, base + 1.0);
    let p3 = sample_at(rows, base + 2.0);
    return clamp(sum, min(min(p0, p1), min(p2, p3)), max(max(p0, p1), max(p2, p3)));
}

// A point in clip coordinates stretched to the frame's aspect ratio, so a
// distance means the same on both axes.
fn to_square(v: vec2f) -> vec2f {
    return ((v - s.window.xz) / s.window.yw * 2.0 - 1.0) * vec2f(s.steps.z, 1.0);
}

struct Beam {
    @builtin(position) position: vec4f,
    // Distance along and across the segment, its length, and its brightness.
    @location(0) uvl: vec4f,
}

@vertex
fn line_vs(@builtin(vertex_index) v: u32, @builtin(instance_index) trace: u32) -> Beam {
    let corner = v & 3u;
    let segment = f32(v >> 2u);
    first_sample = s.starts[trace >> 2u][trace & 3u];
    if (segment < first_sample * s.steps.x) {
        // Beyond the far plane, so the segment is clipped away.
        var hidden: Beam;
        hidden.position = vec4f(0.0, 0.0, 2.0, 1.0);
        return hidden;
    }
    let pair = s.rows[trace >> 1u];
    let rows = select(pair.xy, pair.zw, (trace & 1u) == 1u);
    let start = to_square(point_at(rows, segment));
    let end = to_square(point_at(rows, segment + 1.0));
    let size = s.beam.x;

    var dir = end - start;
    let len = length(dir);
    dir = select(vec2f(1.0, 0.0), dir / len, len > EPS);
    let norm = vec2f(-dir.y, dir.x);

    var o: Beam;
    var current = start;
    var tang = -1.0;
    o.uvl.x = len + size;
    if (corner >= 2u) {
        current = end;
        tang = 1.0;
        o.uvl.x = -size;
    }
    let side = f32(corner & 1u) * 2.0 - 1.0;
    o.uvl.y = side * size;
    o.uvl.z = len;
    // The oldest segment is as bright as one frame of fade leaves it, so the
    // trace's tail meets the faded trail without a step.
    o.uvl.w = s.beam.y * mix(1.0 - s.beam.z, 1.0, segment / s.steps.y);
    let corner_at = current + (tang * dir + norm * side) * size;
    o.position = vec4f(corner_at / vec2f(s.steps.z, 1.0), 0.0, 1.0);
    return o;
}

fn erf(x: f32) -> f32 {
    let a = abs(x);
    var t = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * a) * a) * a) * a;
    t *= t;
    return sign(x) * (1.0 - 1.0 / (t * t));
}

@fragment
fn line_fs(beam: Beam) -> @location(0) vec4f {
    let size = s.beam.x;
    let sigma = size / 5.0;
    let len = beam.uvl.z;
    let xy = beam.uvl.xy;
    var light: f32;
    if (len < EPS) {
        light = exp(-dot(xy, xy) / (2.0 * sigma * sigma)) / (2.0 * sqrt(size));
    } else {
        light = erf(xy.x / SQRT2 / sigma) - erf((xy.x - len) / SQRT2 / sigma);
        light *= exp(-xy.y * xy.y / (2.0 * sigma * sigma)) / 2.0 / len;
    }
    return vec4f(light * beam.uvl.w, 0.0, 0.0, 1.0);
}
`;

/**
 * Full-frame passes over the trace: the fade, the downsampling copy and blur
 * that make the glow, and the composite of trace and glow into the beam's
 * brightness.
 */
export const FRAME_WGSL = /* wgsl */ `
struct Filter {
    // One texel of the source along the blur's direction, in texture coordinates.
    offset: vec2f,
}

@group(0) @binding(0) var linear: sampler;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var<uniform> f: Filter;

struct Quad {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn quad_vs(@builtin(vertex_index) i: u32) -> Quad {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Quad;
    o.position = vec4f(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2f(p.x, 1.0 - p.y);
    return o;
}

// Drawn with blending that subtracts it, so trails too dim for eight bits to
// scale down still fade out.
@fragment
fn fade_floor_fs() -> @location(0) vec4f {
    return vec4f(2.0 / 255.0);
}

// Drawn with blending that scales what is there by one minus the fade.
@fragment
fn fade_fs() -> @location(0) vec4f {
    return vec4f(0.0);
}

@fragment
fn copy_fs(q: Quad) -> @location(0) vec4f {
    return textureSampleLevel(source, linear, q.uv, 0.0);
}

// A 17-tap gaussian (sigma about 3 texels) along one axis.
@fragment
fn blur_fs(q: Quad) -> @location(0) vec4f {
    var weights = array<f32, 9>(
        0.197420, 0.174670, 0.120980, 0.065592, 0.027835,
        0.009245, 0.002403, 0.000489, 0.000078,
    );
    var sum = textureSampleLevel(source, linear, q.uv, 0.0) * weights[0];
    for (var k = 1; k < 9; k++) {
        let d = f.offset * f32(k);
        sum += (textureSampleLevel(source, linear, q.uv - d, 0.0)
            + textureSampleLevel(source, linear, q.uv + d, 0.0)) * weights[k];
    }
    return sum;
}

@group(0) @binding(3) var tight: texture_2d<f32>;
@group(0) @binding(4) var wide: texture_2d<f32>;

// The trace and its two glows, tonemapped into 0..1, with the brightest
// light washed further toward white as a phosphor saturates.
@fragment
fn composite_fs(q: Quad) -> @location(0) vec4f {
    let light = textureSampleLevel(source, linear, q.uv, 0.0).r
        + 1.5 * textureSampleLevel(tight, linear, q.uv, 0.0).r
        + 0.4 * textureSampleLevel(wide, linear, q.uv, 0.0).r;
    let mapped = 1.0 - exp(-1.2 * light);
    let level = mix(mapped, 1.0, pow(mapped, 4.0) * 0.5);
    return vec4f(level, 0.0, 0.0, 1.0);
}
`;
