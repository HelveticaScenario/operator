import type { VideoModuleDef } from './types';

/** Color from hue (wraps every 1.0), saturation and value (both clipped to 0..1). */
export const hsv: VideoModuleDef = {
    inputs: { h: 'field', s: 'field', v: 'field' },
    output: 'color',
    params: {},
    helpers: `fn hsv_to_rgb(h: f32, s: f32, v: f32) -> vec3f {
    let p = abs(fract(vec3f(h) + vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - vec3f(3.0));
    let rgb = clamp(p - vec3f(1.0), vec3f(0.0), vec3f(1.0));
    return clamp(v, 0.0, 1.0) * mix(vec3f(1.0), rgb, clamp(s, 0.0, 1.0));
}`,
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
    helpers: `fn procamp(c: vec3f, gain: f32, bias: f32, saturation: f32) -> vec3f {
    let luma = vec3f(dot(c, vec3f(0.299, 0.587, 0.114)));
    let saturated = mix(luma, c, saturation);
    return clamp(saturated * gain + vec3f(bias), vec3f(0.0), vec3f(1.0));
}`,
    emit: ({ input, gain, bias, saturation }) =>
        `procamp(${input}, ${gain}, ${bias}, ${saturation})`,
};

/** Quantizes to `levels` evenly spaced values between 0 and 1 (minimum 2). */
export const posterize: VideoModuleDef = {
    inputs: { input: 'field', levels: 'field' },
    output: 'field',
    params: {},
    helpers: `fn posterize_levels(x: f32, levels: f32) -> f32 {
    let n = max(levels, 2.0);
    return min(floor(clamp(x, 0.0, 1.0) * n), n - 1.0) / (n - 1.0);
}`,
    emit: ({ input, levels }) => `posterize_levels(${input}, ${levels})`,
};
