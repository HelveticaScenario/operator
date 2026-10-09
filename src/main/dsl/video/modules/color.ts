import type { VideoModuleDef } from './types';

export const HSV_TO_RGB = `fn hsv_to_rgb(h: f32, s: f32, v: f32) -> vec3f {
    let p = abs(fract(vec3f(h) + vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - vec3f(3.0));
    let rgb = clamp(p - vec3f(1.0), vec3f(0.0), vec3f(1.0));
    return clamp(v, 0.0, 1.0) * mix(vec3f(1.0), rgb, clamp(s, 0.0, 1.0));
}`;

/** Color from hue (wraps every 1.0), saturation and value (both clipped to 0..1). */
export const hsv: VideoModuleDef = {
    inputs: { h: 'field', s: 'field', v: 'field' },
    output: 'color',
    params: {},
    helpers: [HSV_TO_RGB],
    emit: ({ h, s, v }) => `hsv_to_rgb(${h}, ${s}, ${v})`,
};

/** Saturation, then gain and bias, clipped to the displayable range. */
export const procAmp: VideoModuleDef = {
    inputs: {
        input: 'color',
        gain: 'field',
        bias: 'field',
        saturation: 'field',
    },
    output: 'color',
    params: {},
    helpers: [
        `fn procamp(c: vec3f, gain: f32, bias: f32, saturation: f32) -> vec3f {
    let luma = vec3f(dot(c, vec3f(0.299, 0.587, 0.114)));
    let saturated = mix(luma, c, saturation);
    return clamp(saturated * gain + vec3f(bias), vec3f(0.0), vec3f(1.0));
}`,
    ],
    emit: ({ input, gain, bias, saturation }) =>
        `procamp(${input}, ${gain}, ${bias}, ${saturation})`,
};

/** Quantizes to `levels` evenly spaced values between 0 and 1 (minimum 2). */
export const posterize: VideoModuleDef = {
    inputs: { input: 'field', levels: 'field' },
    output: 'field',
    params: {},
    helpers: [
        `fn posterize_levels(x: f32, levels: f32) -> f32 {
    let n = max(levels, 2.0);
    return min(floor(clamp(x, 0.0, 1.0) * n), n - 1.0) / (n - 1.0);
}`,
    ],
    emit: ({ input, levels }) => `posterize_levels(${input}, ${levels})`,
};

/** Turns every hue by `amount` of a full circle, keeping saturation and brightness. */
export const hueShift: VideoModuleDef = {
    inputs: { input: 'color', amount: 'field' },
    output: 'color',
    params: {},
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
        `fn hue_shift(c: vec3f, amount: f32) -> vec3f {
    let hsv = rgb_to_hsv(c);
    return hsv_to_rgb(hsv.x + amount, hsv.y, hsv.z);
}`,
    ],
    emit: ({ input, amount }) => `hue_shift(${input}, ${amount})`,
};

/** Scales each channel's distance from mid-gray by `amount`. */
export const contrast: VideoModuleDef = {
    inputs: { input: 'color', amount: 'field' },
    output: 'color',
    params: {},
    emit: ({ input, amount }) =>
        `clamp((${input} - vec3f(0.5)) * ${amount} + vec3f(0.5), vec3f(0.0), vec3f(1.0))`,
};
