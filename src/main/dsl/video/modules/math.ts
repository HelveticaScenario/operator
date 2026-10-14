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

/**
 * One module from two that differ in which side of a threshold they keep,
 * chosen by its `keep` param: `above` (the default) or `below`.
 */
function keeping(above: VideoModuleDef, below: VideoModuleDef): VideoModuleDef {
    const pick = (keep: string) => (keep === 'below' ? below : above);
    return {
        ...above,
        params: { keep: { values: ['above', 'below'], default: 'above' } },
        helpers: ({ keep }) => pick(keep).helpers as readonly string[],
        emit: (args, params, indices) =>
            pick(params.keep).emit(args, params, indices),
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

/** Maps each channel's 0..1 onto `lo`..`hi`; the alpha is kept. */
export const range = perChannel('range', ['lo', 'hi'], 'lo + x * (hi - lo)');

/**
 * Threshold per channel: 0 below `threshold`, 1 above, with a linear ramp
 * `softness` wide centered on it (a hard edge when softness is 0). Keeping
 * `below` gives 1 below and 0 above.
 */
export const comparator = keeping(
    perChannel(
        'comparator',
        ['threshold', 'softness'],
        'clamp((x - vec3f(threshold)) / max(softness, 0.00001) + vec3f(0.5), vec3f(0.0), vec3f(1.0))',
    ),
    perChannel(
        'comparator_below',
        ['threshold', 'softness'],
        'clamp((vec3f(threshold) - x) / max(softness, 0.00001) + vec3f(0.5), vec3f(0.0), vec3f(1.0))',
    ),
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

/**
 * `input` seen through `mask`: its color and alpha both scaled by the mask's
 * luma, as in Hydra, so a masked-out color adds nothing when combined.
 */
export const mask: VideoModuleDef = {
    inputs: { input: 'rgba', mask: 'number' },
    output: 'rgba',
    helpers: [
        `fn video_mask(c: vec4f, m: f32) -> vec4f {
    let k = clamp(m, 0.0, 1.0);
    return vec4f(c.rgb * k, c.a * k);
}`,
    ],
    emit: ({ input, mask: m }) => `video_mask(${input}, ${m})`,
};

/**
 * Keys `input` by its own brightness through `fn video_<name>`, whose key is
 * the expression `alpha` of the brightness `l`, `threshold` and `band`. The
 * key scales the color as well as the alpha, as in Hydra, so a keyed-out
 * color adds nothing when combined.
 */
const lumaKey = (name: string, alpha: string): VideoModuleDef => ({
    inputs: { input: 'rgba', threshold: 'number', tolerance: 'number' },
    output: 'rgba',
    helpers: [
        LUMA_HELPER,
        `fn video_${name}(c: vec4f, threshold: f32, tolerance: f32) -> vec4f {
    let l = video_luma(c);
    let band = max(tolerance, 0.00001);
    let k = ${alpha};
    return vec4f(c.rgb * k, c.a * k);
}`,
    ],
    emit: ({ input, threshold, tolerance }) =>
        `video_${name}(${input}, ${threshold}, ${tolerance})`,
});

/**
 * Keys `input` by its own brightness: alpha rises from 0 at `threshold -
 * tolerance` to 1 at `threshold`, so a threshold of 1 keeps only white.
 * Keeping `below` is the mirror: opaque up to `threshold`, transparent from
 * `threshold + tolerance`.
 */
export const luma = keeping(
    lumaKey('luma_key', 'smoothstep(threshold - band, threshold, l)'),
    lumaKey(
        'luma_key_below',
        '(1.0 - smoothstep(threshold, threshold + band, l))',
    ),
);

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

const POSTERIZE_LEVELS = 'max(floor(levels), 2.0)';

/** Quantizes each channel to `levels` (rounded down, minimum 2) evenly spaced values between 0 and 1. */
export const posterize = perChannel(
    'posterize',
    ['levels'],
    `min(floor(clamp(x, vec3f(0.0), vec3f(1.0)) * ${POSTERIZE_LEVELS}), vec3f(${POSTERIZE_LEVELS} - 1.0)) / (${POSTERIZE_LEVELS} - 1.0)`,
);

/** Multiplies each channel by `gain`, then keeps the fractional part: a sawtooth of the input. */
export const wrap = perChannel('wrap', ['gain'], 'fract(x * gain)');

/**
 * Multiplies each channel by `gain`, then reflects whatever passes 1 back
 * down: a triangle of the input that matches `input * gain` up to an input of
 * 1 / gain.
 */
export const fold = perChannel(
    'fold',
    ['gain'],
    'vec3f(1.0) - abs(vec3f(1.0) - 2.0 * fract(x * gain * 0.5))',
);
