import { NOISE_HASH } from './noise';
import type { VideoModuleDef } from './types';

/**
 * Darkens evenly spaced horizontal lines, as a CRT's raster does: `count` lines
 * across the frame height, each dimmed by up to `strength` at its darkest.
 */
export const scanlines: VideoModuleDef = {
    inputs: { input: 'rgba', count: 'number', strength: 'number' },
    output: 'rgba',
    helpers: [
        `fn video_scanlines(c: vec4f, uv: vec2f, count: f32, strength: f32) -> vec4f {
    let dim = 1.0 - clamp(strength, 0.0, 1.0) * (0.5 + 0.5 * cos(6.28318530718 * uv.y * count));
    return vec4f(c.rgb * dim, c.a);
}`,
    ],
    emit: ({ input, count, strength }) =>
        `video_scanlines(${input}, uv, ${count}, ${strength})`,
};

/**
 * Darkens toward the corners: nothing within `radius` of the center (as a
 * fraction of the frame height), then up to `strength` by the edge of the frame.
 */
export const vignette: VideoModuleDef = {
    inputs: { input: 'rgba', strength: 'number', radius: 'number' },
    output: 'rgba',
    helpers: [
        `fn vignette_falloff(uv: vec2f, radius: f32) -> f32 {
    let aspect = vec2f(u.resolution.x / u.resolution.y, 1.0);
    let corner = length(vec2f(0.5) * aspect);
    return smoothstep(radius, max(corner, radius + 0.001), length((uv - vec2f(0.5)) * aspect));
}`,
        `fn video_darken(c: vec4f, amount: f32) -> vec4f {
    return vec4f(c.rgb * (1.0 - amount), c.a);
}`,
    ],
    emit: ({ input, strength, radius }) =>
        `video_darken(${input}, clamp(${strength}, 0.0, 1.0) * vignette_falloff(uv, ${radius}))`,
};

/**
 * Adds film grain: a fresh random value per pixel, redrawn sixty times a
 * second, of up to `amount` in either direction.
 */
export const grain: VideoModuleDef = {
    inputs: { input: 'rgba', amount: 'number' },
    output: 'rgba',
    helpers: [
        NOISE_HASH,
        `fn grain_value(uv: vec2f, time: f32) -> f32 {
    let pixel = vec2i(floor(uv * u.resolution));
    return noise_hash(vec3i(pixel, i32(floor(time * 60.0)))) - 0.5;
}`,
        `fn video_grain(c: vec4f, offset: f32) -> vec4f {
    return vec4f(clamp(c.rgb + vec3f(offset), vec3f(0.0), vec3f(1.0)), c.a);
}`,
    ],
    emit: ({ input, amount }) =>
        `video_grain(${input}, grain_value(uv, u.time) * 2.0 * ${amount})`,
};
