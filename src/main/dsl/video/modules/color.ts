import { LUMA_HELPER } from './rgba';
import type { VideoModuleDef } from './types';

const HSV_TO_RGB = `fn hsv_to_rgb(h: f32, s: f32, v: f32) -> vec3f {
    let p = abs(fract(vec3f(h) + vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - vec3f(3.0));
    let rgb = clamp(p - vec3f(1.0), vec3f(0.0), vec3f(1.0));
    return clamp(v, 0.0, 1.0) * mix(vec3f(1.0), rgb, clamp(s, 0.0, 1.0));
}`;

const RGB_TO_HSV = `fn rgb_to_hsv(c: vec3f) -> vec3f {
    let k = vec4f(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4f(c.bg, k.wz), vec4f(c.gb, k.xy), step(c.b, c.g));
    let q = mix(vec4f(p.xyw, c.r), vec4f(c.r, p.yzx), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;
    return vec3f(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}`;

/** An opaque color from hue (wraps every 1.0), saturation and value (both clipped to 0..1). */
export const hsv: VideoModuleDef = {
    inputs: { hue: 'number', saturation: 'number', value: 'number' },
    output: 'rgba',
    helpers: [HSV_TO_RGB],
    emit: ({ hue, saturation, value }) =>
        `vec4f(hsv_to_rgb(${hue}, ${saturation}, ${value}), 1.0)`,
};

/** Saturation, then gain and bias, clipped to the displayable range; the alpha is kept. */
export const procAmp: VideoModuleDef = {
    inputs: {
        input: 'rgba',
        gain: 'number',
        bias: 'number',
        saturation: 'number',
    },
    output: 'rgba',
    helpers: [
        LUMA_HELPER,
        `fn procamp(c: vec4f, gain: f32, bias: f32, saturation: f32) -> vec4f {
    let saturated = mix(vec3f(video_luma(c)), c.rgb, saturation);
    return vec4f(clamp(saturated * gain + vec3f(bias), vec3f(0.0), vec3f(1.0)), c.a);
}`,
    ],
    emit: ({ input, gain, bias, saturation }) =>
        `procamp(${input}, ${gain}, ${bias}, ${saturation})`,
};

/** Turns every hue by `amount` of a full circle, keeping saturation, brightness and alpha. */
export const hue: VideoModuleDef = {
    inputs: { input: 'rgba', amount: 'number' },
    output: 'rgba',
    helpers: [
        HSV_TO_RGB,
        RGB_TO_HSV,
        `fn video_hue(c: vec4f, amount: f32) -> vec4f {
    let hsv = rgb_to_hsv(c.rgb);
    return vec4f(hsv_to_rgb(hsv.x + amount, hsv.y, hsv.z), c.a);
}`,
    ],
    emit: ({ input, amount }) => `video_hue(${input}, ${amount})`,
};

/** Adds `amount` to red, green and blue, as Hydra's `brightness` does; the alpha is kept. */
export const brightness: VideoModuleDef = {
    inputs: { input: 'rgba', amount: 'number' },
    output: 'rgba',
    helpers: [
        `fn video_brightness(c: vec4f, amount: f32) -> vec4f {
    return vec4f(c.rgb + vec3f(amount), c.a);
}`,
    ],
    emit: ({ input, amount }) => `video_brightness(${input}, ${amount})`,
};

/**
 * Scales each channel's distance from the color's gray by `amount`, weighting
 * the gray as Hydra's `saturate` does; the alpha is kept.
 */
export const saturate: VideoModuleDef = {
    inputs: { input: 'rgba', amount: 'number' },
    output: 'rgba',
    helpers: [
        `fn video_saturate(c: vec4f, amount: f32) -> vec4f {
    let gray = vec3f(dot(c.rgb, vec3f(0.2125, 0.7154, 0.0721)));
    return vec4f(mix(gray, c.rgb, amount), c.a);
}`,
    ],
    emit: ({ input, amount }) => `video_saturate(${input}, ${amount})`,
};

/**
 * Hydra's `colorama`: adds `amount` to the hue, saturation and value, turns the
 * result back into a color without clipping it, and wraps each channel into
 * 0..1, so colors cycle through bands. The alpha is kept.
 */
export const colorama: VideoModuleDef = {
    inputs: { input: 'rgba', amount: 'number' },
    output: 'rgba',
    helpers: [
        RGB_TO_HSV,
        `fn video_colorama(c: vec4f, amount: f32) -> vec4f {
    let hsv = rgb_to_hsv(c.rgb) + vec3f(amount);
    let p = abs(fract(vec3f(hsv.x) + vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - vec3f(3.0));
    let rgb = hsv.z * mix(vec3f(1.0), clamp(p - vec3f(1.0), vec3f(0.0), vec3f(1.0)), hsv.y);
    return vec4f(fract(rgb), c.a);
}`,
    ],
    emit: ({ input, amount }) => `video_colorama(${input}, ${amount})`,
};

/**
 * Hydra's `color`: tints `input` by multiplying each channel, alpha included,
 * by its amount; a negative amount multiplies the channel's complement by the
 * amount's size instead, so it inverts that channel.
 */
export const color: VideoModuleDef = {
    inputs: {
        input: 'rgba',
        r: 'number',
        g: 'number',
        b: 'number',
        a: 'number',
    },
    output: 'rgba',
    helpers: [
        `fn video_color(c: vec4f, amount: vec4f) -> vec4f {
    return mix((vec4f(1.0) - c) * abs(amount), amount * c, step(vec4f(0.0), amount));
}`,
    ],
    emit: ({ input, r, g, b, a }) =>
        `video_color(${input}, vec4f(${r}, ${g}, ${b}, ${a}))`,
};

/**
 * Hydra's `shift`: adds the fractional part of each amount to its channel,
 * alpha included. The result is not wrapped.
 */
export const shift: VideoModuleDef = {
    inputs: {
        input: 'rgba',
        r: 'number',
        g: 'number',
        b: 'number',
        a: 'number',
    },
    output: 'rgba',
    helpers: [
        `fn video_shift(c: vec4f, amount: vec4f) -> vec4f {
    return c + fract(amount);
}`,
    ],
    emit: ({ input, r, g, b, a }) =>
        `video_shift(${input}, vec4f(${r}, ${g}, ${b}, ${a}))`,
};

/** Scales each channel's distance from mid-gray by `amount`; the alpha is kept. */
export const contrast: VideoModuleDef = {
    inputs: { input: 'rgba', amount: 'number' },
    output: 'rgba',
    helpers: [
        `fn video_contrast(c: vec4f, amount: f32) -> vec4f {
    return vec4f((c.rgb - vec3f(0.5)) * amount + vec3f(0.5), c.a);
}`,
    ],
    emit: ({ input, amount }) => `video_contrast(${input}, ${amount})`,
};

/** One channel of a signal, red, green, blue, alpha or brightness, as a gray. */
export const channel: VideoModuleDef = {
    inputs: { input: 'rgba' },
    output: 'gray',
    params: {
        channel: { values: ['r', 'g', 'b', 'a', 'luma'], default: 'luma' },
    },
    helpers: ({ channel: which }) => (which === 'luma' ? [LUMA_HELPER] : []),
    emit: ({ input }, { channel: which }) =>
        which === 'luma' ? `video_luma(${input})` : `${input}.${which}`,
};
