import { LUMA_HELPER } from './rgba';
import type { VideoModuleDef } from './types';

const unit = (x: string) => `clamp(${x}, 0.0, 1.0)`;

/**
 * A module of two colors through `fn video_<name>(a: vec4f, b: vec4f)`, whose
 * `body` is the function's return expression.
 */
function binary(name: string, body: string): VideoModuleDef {
    return {
        inputs: { a: 'rgba', b: 'rgba' },
        output: 'rgba',
        helpers: [
            `fn video_${name}(a: vec4f, b: vec4f) -> vec4f {\n    return ${body};\n}`,
        ],
        emit: ({ a, b }) => `video_${name}(${a}, ${b})`,
    };
}

/**
 * A module that reshapes each of the red, green and blue of `input` through
 * `fn video_<name>(x: vec3f, ...)` and keeps its alpha. `numbers` are the
 * further inputs, passed after the channels in order.
 */
function perChannel(
    name: string,
    numbers: readonly string[],
    body: string,
): VideoModuleDef {
    const params = numbers.map((n) => `, ${n}: f32`).join('');
    return {
        inputs: {
            input: 'rgba',
            ...Object.fromEntries(numbers.map((n) => [n, 'number'])),
        },
        output: 'rgba',
        helpers: [
            `fn video_${name}(c: vec4f${params}) -> vec4f {\n    let x = c.rgb;\n    return vec4f(${body}, c.a);\n}`,
        ],
        emit: (args) =>
            `video_${name}(${[args.input, ...numbers.map((n) => args[n])].join(', ')})`,
    };
}

/** Sum, clipped to 0..1; the alpha is the larger of the two. */
export const add = binary(
    'add',
    'vec4f(clamp(a.rgb + b.rgb, vec3f(0.0), vec3f(1.0)), max(a.a, b.a))',
);

/** Product: multiplying by a 0..1 signal darkens, and alphas multiply too. */
export const mult = binary('mult', 'a * b');

/** Absolute difference; the alpha is the larger of the two. */
export const diff = binary('diff', 'vec4f(abs(a.rgb - b.rgb), max(a.a, b.a))');

/** The larger of each channel; with masks this is a union of shapes. */
export const max = binary('max', 'max(a, b)');

/** The smaller of each channel; with masks this is an intersection of shapes. */
export const min = binary('min', 'min(a, b)');

/** Complement of red, green and blue: 1 minus each; the alpha is kept. */
export const invert = perChannel(
    'invert',
    [],
    'vec3f(1.0) - clamp(x, vec3f(0.0), vec3f(1.0))',
);

/** Crossfade from `a` (amount 0) to `b` (amount 1), alpha included. */
export const mix: VideoModuleDef = {
    inputs: { a: 'rgba', b: 'rgba', amount: 'number' },
    output: 'rgba',
    emit: ({ a, b, amount }) => `mix(${a}, ${b}, ${unit(amount)})`,
};

/** Maps each channel's 0..1 onto `min`..`max`. */
export const range = perChannel('range', ['lo', 'hi'], 'lo + x * (hi - lo)');

/**
 * Threshold per channel: 0 below `threshold`, 1 above, with a linear ramp
 * `softness` wide centered on it (a hard edge when softness is 0).
 */
export const comparator = perChannel(
    'comparator',
    ['threshold', 'softness'],
    'clamp((x - vec3f(threshold)) / max(softness, 0.00001) + vec3f(0.5), vec3f(0.0), vec3f(1.0))',
);

/** Shows `fg` where `mask` is 1 and `bg` where it is 0. */
export const key: VideoModuleDef = {
    inputs: { fg: 'rgba', bg: 'rgba', mask: 'number' },
    output: 'rgba',
    emit: ({ fg, bg, mask }) => `mix(${bg}, ${fg}, ${unit(mask)})`,
};

/** `top` laid over `base` by the alpha of `top`. */
export const layer = binary(
    'layer',
    'vec4f(mix(a.rgb, b.rgb, clamp(b.a, 0.0, 1.0)), clamp(a.a + b.a, 0.0, 1.0))',
);

/** `input` seen through `mask`: its alpha scaled by the mask's luma. */
export const mask: VideoModuleDef = {
    inputs: { input: 'rgba', mask: 'number' },
    output: 'rgba',
    helpers: [
        `fn video_mask(c: vec4f, m: f32) -> vec4f {
    return vec4f(c.rgb, c.a * clamp(m, 0.0, 1.0));
}`,
    ],
    emit: ({ input, mask: m }) => `video_mask(${input}, ${m})`,
};

/**
 * Keys `input` by its own brightness: alpha rises from 0 at `threshold -
 * tolerance` to 1 at `threshold`, so a threshold of 1 keeps only white.
 */
export const luma: VideoModuleDef = {
    inputs: { input: 'rgba', threshold: 'number', tolerance: 'number' },
    output: 'rgba',
    helpers: [
        LUMA_HELPER,
        `fn video_luma_key(c: vec4f, threshold: f32, tolerance: f32) -> vec4f {
    let a = smoothstep(threshold - max(tolerance, 0.00001), threshold, video_luma(c));
    return vec4f(c.rgb, c.a * a);
}`,
    ],
    emit: ({ input, threshold, tolerance }) =>
        `video_luma_key(${input}, ${threshold}, ${tolerance})`,
};

/** A color from four levels, each clipped to 0..1. */
export const colorize: VideoModuleDef = {
    inputs: { r: 'number', g: 'number', b: 'number', a: 'number' },
    output: 'rgba',
    emit: ({ r, g, b, a }) =>
        `clamp(vec4f(${r}, ${g}, ${b}, ${a}), vec4f(0.0), vec4f(1.0))`,
};

/** Final stage before the display. */
export const out: VideoModuleDef = {
    inputs: { input: 'rgba' },
    output: 'rgba',
    emit: ({ input }) => input,
};

/** Quantizes each channel to `levels` evenly spaced values between 0 and 1 (minimum 2). */
export const posterize = perChannel(
    'posterize',
    ['levels'],
    'min(floor(clamp(x, vec3f(0.0), vec3f(1.0)) * max(levels, 2.0)), vec3f(max(levels, 2.0) - 1.0)) / (max(levels, 2.0) - 1.0)',
);

/** Multiplies each channel by `gain`, then keeps the fractional part: a sawtooth of the input. */
export const wrap = perChannel('wrap', ['gain'], 'fract(x * gain)');

/**
 * Multiplies each channel by `gain`, then reflects whatever passes 1 back
 * down: a triangle of the input that matches it up to 1 / gain.
 */
export const fold = perChannel(
    'fold',
    ['gain'],
    'vec3f(1.0) - abs(vec3f(1.0) - 2.0 * fract(x * gain * 0.5))',
);
