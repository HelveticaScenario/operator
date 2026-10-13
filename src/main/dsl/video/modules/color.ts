import { LUMA_HELPER } from './rgba';
import type { VideoModuleDef } from './types';

const HSV_TO_RGB = `fn hsv_to_rgb(h: f32, s: f32, v: f32) -> vec3f {
    let p = abs(fract(vec3f(h) + vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - vec3f(3.0));
    let rgb = clamp(p - vec3f(1.0), vec3f(0.0), vec3f(1.0));
    return clamp(v, 0.0, 1.0) * mix(vec3f(1.0), rgb, clamp(s, 0.0, 1.0));
}`;

/** An opaque color from hue (wraps every 1.0), saturation and value (both clipped to 0..1). */
export const hsv: VideoModuleDef = {
    inputs: { h: 'number', s: 'number', v: 'number' },
    output: 'rgba',
    helpers: [HSV_TO_RGB],
    emit: ({ h, s, v }) => `vec4f(hsv_to_rgb(${h}, ${s}, ${v}), 1.0)`,
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
export const hueShift: VideoModuleDef = {
    inputs: { input: 'rgba', amount: 'number' },
    output: 'rgba',
    helpers: [
        HSV_TO_RGB,
        `fn rgb_to_hsv(c: vec3f) -> vec3f {
    let k = vec4f(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4f(c.bg, k.wz), vec4f(c.gb, k.xy), step(c.b, c.g));
    let q = mix(vec4f(p.xyw, c.r), vec4f(c.r, p.yzx), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;
    return vec3f(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}`,
        `fn hue_shift(c: vec4f, amount: f32) -> vec4f {
    let hsv = rgb_to_hsv(c.rgb);
    return vec4f(hsv_to_rgb(hsv.x + amount, hsv.y, hsv.z), c.a);
}`,
    ],
    emit: ({ input, amount }) => `hue_shift(${input}, ${amount})`,
};

/** Scales each channel's distance from mid-gray by `amount`; the alpha is kept. */
export const contrast: VideoModuleDef = {
    inputs: { input: 'rgba', amount: 'number' },
    output: 'rgba',
    helpers: [
        `fn video_contrast(c: vec4f, amount: f32) -> vec4f {
    return vec4f(clamp((c.rgb - vec3f(0.5)) * amount + vec3f(0.5), vec3f(0.0), vec3f(1.0)), c.a);
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
