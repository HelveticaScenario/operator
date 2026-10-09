import type { VideoModuleDef } from './types';

/**
 * Smooth value noise between 0 and 1. One unit of any coordinate is one
 * lattice cell, so scaling the coordinates sets the grain; `z` moves through
 * the noise, which animates it when fed `$v.time`.
 */
export const noise: VideoModuleDef = {
    inputs: { x: 'field', y: 'field', z: 'field' },
    output: 'field',
    params: {},
    helpers: [
        `fn noise_hash(cell: vec3i) -> f32 {
    var h = u32(cell.x) * 374761393u + u32(cell.y) * 668265263u + u32(cell.z) * 2147483647u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 16777215u) / 16777215.0;
}`,
        `fn noise_value(p: vec3f) -> f32 {
    let base = floor(p);
    let t = fract(p);
    let s = t * t * (3.0 - 2.0 * t);
    let c = vec3i(base);
    let x00 = mix(noise_hash(c), noise_hash(c + vec3i(1, 0, 0)), s.x);
    let x10 = mix(noise_hash(c + vec3i(0, 1, 0)), noise_hash(c + vec3i(1, 1, 0)), s.x);
    let x01 = mix(noise_hash(c + vec3i(0, 0, 1)), noise_hash(c + vec3i(1, 0, 1)), s.x);
    let x11 = mix(noise_hash(c + vec3i(0, 1, 1)), noise_hash(c + vec3i(1, 1, 1)), s.x);
    return mix(mix(x00, x10, s.y), mix(x01, x11, s.y), s.z);
}`,
    ],
    emit: ({ x, y, z }) => `noise_value(vec3f(${x}, ${y}, ${z}))`,
};
