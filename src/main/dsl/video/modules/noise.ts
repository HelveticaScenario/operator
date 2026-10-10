import type { VideoModuleDef } from './types';

/** Integer-lattice hash to 0..1; shared by every module that scatters points. */
export const NOISE_HASH = `fn noise_hash(cell: vec3i) -> f32 {
    var h = u32(cell.x) * 374761393u + u32(cell.y) * 668265263u + u32(cell.z) * 2147483647u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 16777215u) / 16777215.0;
}`;

/**
 * Smooth value noise between 0 and 1. One unit of any coordinate is one
 * lattice cell, so scaling the coordinates sets the grain; `z` moves through
 * the noise, which animates it when fed `$v.time`.
 */
export const noise: VideoModuleDef = {
    inputs: { x: 'field', y: 'field', z: 'field' },
    natural: ['x', 'y', 'z'],
    output: 'field',
    helpers: [
        NOISE_HASH,
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

/**
 * Cellular noise: the distance to the nearest of a scatter of points, one per
 * unit cell of the coordinates, between 0 (on a point) and about 1. `z` moves
 * the points, which animates the cells when fed `$v.time`.
 */
export const voronoi: VideoModuleDef = {
    inputs: { x: 'field', y: 'field', z: 'field' },
    natural: ['x', 'y', 'z'],
    output: 'field',
    helpers: [
        NOISE_HASH,
        `fn voronoi_distance(p: vec2f, z: f32) -> f32 {
    let base = floor(p);
    let local = p - base;
    var nearest = 8.0;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let cell = vec2i(base) + vec2i(i, j);
            let hx = noise_hash(vec3i(cell, 0));
            let hy = noise_hash(vec3i(cell, 1));
            let point = vec2f(f32(i), f32(j)) + vec2f(
                0.5 + 0.5 * sin(z + 6.28318530718 * hx),
                0.5 + 0.5 * sin(z * 0.8 + 6.28318530718 * hy),
            );
            nearest = min(nearest, distance(local, point));
        }
    }
    return clamp(nearest, 0.0, 1.0);
}`,
    ],
    emit: ({ x, y, z }) => `voronoi_distance(vec2f(${x}, ${y}), ${z})`,
};
