import { LUMA_HELPER } from './rgba';
import type { VideoModuleDef, VideoParamSpec } from './types';

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

/** Sum of every channel, alpha included, unclipped as in Hydra. */
export const add = binary('add', 'a + b');

/** Product: multiplying by a 0..1 signal darkens, and alphas multiply too. */
export const mult = binary('mult', 'a * b');

/** Absolute difference; the alpha is the larger of the two. */
export const diff = binary('diff', 'vec4f(abs(a.rgb - b.rgb), max(a.a, b.a))');

/** The larger of each channel; with masks this is a union of shapes. */
export const max = binary('max', 'max(a, b)');

/** The smaller of each channel; with masks this is an intersection of shapes. */
export const min = binary('min', 'min(a, b)');

/**
 * Crossfades red, green and blue toward their complement, 1 minus each, by
 * `amount`, as Hydra's `invert` does; the alpha is kept.
 */
export const invert = perChannel(
    'invert',
    ['amount'],
    '(vec3f(1.0) - x) * amount + x * (1.0 - amount)',
);

/** Crossfade from `a` (amount 0) to `b` (amount 1), alpha included. */
export const mix: VideoModuleDef = {
    inputs: { a: 'rgba', b: 'rgba', amount: 'number' },
    output: 'rgba',
    emit: ({ a, b, amount }) => `mix(${a}, ${b}, ${unit(amount)})`,
};

/**
 * Hydra's `blend`: `a` times `1 - amount` plus `b` times `amount`, alpha
 * included, with `amount` unclipped, so past 1 it pushes beyond `b`.
 */
export const blend: VideoModuleDef = {
    inputs: { a: 'rgba', b: 'rgba', amount: 'number' },
    output: 'rgba',
    emit: ({ a, b, amount }) => `mix(${a}, ${b}, ${amount})`,
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

/** Whether a key scales the color with the alpha (`scale`, the default) or keeps it (`keep`). */
const KEY_COLOR: VideoParamSpec = {
    values: ['scale', 'keep'],
    default: 'scale',
};

/**
 * The WGSL that keys color `c` by `k`. Scaling the color too, as in Hydra,
 * makes a keyed-out color add nothing when combined; keeping it leaves that
 * color for whatever reads the signal as a number.
 */
const applyKey = (color: string) =>
    color === 'keep' ? 'vec4f(c.rgb, c.a * k)' : 'vec4f(c.rgb * k, c.a * k)';

/** The suffix that names the `keep` variant of a key function. */
const keySuffix = (color: string) => (color === 'keep' ? '_keep_color' : '');

/**
 * `input` seen through `mask`: its alpha scaled by the mask's luma, and its
 * color too unless `color` is `keep`.
 */
export const mask: VideoModuleDef = {
    inputs: { input: 'rgba', mask: 'number' },
    output: 'rgba',
    params: { color: KEY_COLOR },
    helpers: ({ color }) => [
        `fn video_mask${keySuffix(color)}(c: vec4f, m: f32) -> vec4f {
    let k = clamp(m, 0.0, 1.0);
    return ${applyKey(color)};
}`,
    ],
    emit: ({ input, mask: m }, { color }) =>
        `video_mask${keySuffix(color)}(${input}, ${m})`,
};

/**
 * The function that keys `c` by its own brightness, named `video_<name>` plus
 * the `color` suffix, whose key is the expression `alpha` of the brightness
 * `l`, `threshold` and `band`.
 */
const lumaKey = (name: string, alpha: string, color: string) =>
    `fn video_${name}${keySuffix(color)}(c: vec4f, threshold: f32, tolerance: f32) -> vec4f {
    let l = video_luma(c);
    let band = max(tolerance, 0.0) + 0.0000001;
    let k = ${alpha};
    return ${applyKey(color)};
}`;

/**
 * Keys `input` by its own brightness, as Hydra's `luma` does: alpha rises
 * smoothly from 0 at `threshold - tolerance` to 1 at `threshold + tolerance`.
 * Keeping `below` is the mirror. The key scales the color too unless `color`
 * is `keep`.
 */
export const luma: VideoModuleDef = {
    inputs: { input: 'rgba', threshold: 'number', tolerance: 'number' },
    output: 'rgba',
    params: {
        keep: { values: ['above', 'below'], default: 'above' },
        color: KEY_COLOR,
    },
    helpers: ({ keep, color }) => [
        LUMA_HELPER,
        keep === 'below'
            ? lumaKey(
                  'luma_key_below',
                  '(1.0 - smoothstep(threshold - band, threshold + band, l))',
                  color,
              )
            : lumaKey(
                  'luma_key',
                  'smoothstep(threshold - band, threshold + band, l)',
                  color,
              ),
    ],
    emit: ({ input, threshold, tolerance }, { keep, color }) =>
        `video_luma_key${keep === 'below' ? '_below' : ''}${keySuffix(color)}(${input}, ${threshold}, ${tolerance})`,
};

/**
 * The brightness of `input` as a gray, stepped at `threshold` with a smooth
 * edge `tolerance` wide on each side, as Hydra's `thresh` does; the alpha is
 * kept.
 */
export const thresh: VideoModuleDef = {
    inputs: { input: 'rgba', threshold: 'number', tolerance: 'number' },
    output: 'rgba',
    helpers: [
        LUMA_HELPER,
        `fn video_thresh(c: vec4f, threshold: f32, tolerance: f32) -> vec4f {
    let band = max(tolerance, 0.0) + 0.0000001;
    let level = smoothstep(threshold - band, threshold + band, video_luma(c));
    return vec4f(vec3f(level), c.a);
}`,
    ],
    emit: ({ input, threshold, tolerance }) =>
        `video_thresh(${input}, ${threshold}, ${tolerance})`,
};

/** A color from four levels, unclipped as in Hydra. */
export const solid: VideoModuleDef = {
    inputs: { r: 'number', g: 'number', b: 'number', a: 'number' },
    output: 'rgba',
    emit: ({ r, g, b, a }) => `vec4f(${r}, ${g}, ${b}, ${a})`,
};

/** Final stage before the display. */
export const out: VideoModuleDef = {
    inputs: { input: 'rgba' },
    output: 'rgba',
    emit: ({ input }) => input,
};

/**
 * Hydra's `posterize`: each channel is raised to `gamma`, cut into `bins`
 * steps, and raised back, so the steps crowd toward whichever end `gamma`
 * favors and never quite reach 1. Negative channels count as 0. The alpha is
 * kept.
 */
export const posterize = perChannel(
    'posterize',
    ['bins', 'gamma'],
    'pow(floor(pow(max(x, vec3f(0.0)), vec3f(gamma)) * bins) / bins, vec3f(1.0 / gamma))',
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
