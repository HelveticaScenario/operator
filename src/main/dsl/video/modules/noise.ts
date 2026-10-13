import type { VideoModuleDef } from './types';

/**
 * Integer-lattice hashes: `noise_hash_u` to 32 bits and `noise_hash` to 0..1;
 * shared by every module that scatters points or gradients.
 */
export const NOISE_HASH = `fn noise_hash_u(cell: vec3i) -> u32 {
    var h = u32(cell.x) * 374761393u + u32(cell.y) * 668265263u + u32(cell.z) * 2147483647u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    return h ^ (h >> 16u);
}

fn noise_hash(cell: vec3i) -> f32 {
    return f32(noise_hash_u(cell) & 16777215u) / 16777215.0;
}`;

/** Dot product of `d` with one of the 12 cube-edge gradients, chosen by hashing `cell`. */
const NOISE_GRAD = `fn noise_grad(cell: vec3i, d: vec3f) -> f32 {
    let h = noise_hash_u(cell) % 12u;
    let a = select(d.y, d.x, h < 8u);
    let b = select(d.z, d.y, h < 4u);
    return select(-a, a, (h & 1u) == 0u) + select(-b, b, (h & 2u) == 0u);
}`;

/** Value noise: hashed corner values blended with smoothstep, 0..1. */
const NOISE_VALUE = `fn noise_value(p: vec3f) -> f32 {
    let c = vec3i(floor(p));
    let t = fract(p);
    let s = t * t * (3.0 - 2.0 * t);
    let x00 = mix(noise_hash(c), noise_hash(c + vec3i(1, 0, 0)), s.x);
    let x10 = mix(noise_hash(c + vec3i(0, 1, 0)), noise_hash(c + vec3i(1, 1, 0)), s.x);
    let x01 = mix(noise_hash(c + vec3i(0, 0, 1)), noise_hash(c + vec3i(1, 0, 1)), s.x);
    let x11 = mix(noise_hash(c + vec3i(0, 1, 1)), noise_hash(c + vec3i(1, 1, 1)), s.x);
    return mix(mix(x00, x10, s.y), mix(x01, x11, s.y), s.z);
}`;

/** Improved Perlin gradient noise with a quintic fade, about -1..1. */
const NOISE_PERLIN = `fn noise_perlin(p: vec3f) -> f32 {
    let c = vec3i(floor(p));
    let f = fract(p);
    let s = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let x00 = mix(noise_grad(c, f), noise_grad(c + vec3i(1, 0, 0), f - vec3f(1.0, 0.0, 0.0)), s.x);
    let x10 = mix(noise_grad(c + vec3i(0, 1, 0), f - vec3f(0.0, 1.0, 0.0)), noise_grad(c + vec3i(1, 1, 0), f - vec3f(1.0, 1.0, 0.0)), s.x);
    let x01 = mix(noise_grad(c + vec3i(0, 0, 1), f - vec3f(0.0, 0.0, 1.0)), noise_grad(c + vec3i(1, 0, 1), f - vec3f(1.0, 0.0, 1.0)), s.x);
    let x11 = mix(noise_grad(c + vec3i(0, 1, 1), f - vec3f(0.0, 1.0, 1.0)), noise_grad(c + vec3i(1, 1, 1), f - vec3f(1.0, 1.0, 1.0)), s.x);
    return mix(mix(x00, x10, s.y), mix(x01, x11, s.y), s.z);
}`;

/**
 * Simplex noise: gradients at the four corners of the tetrahedron of a skewed
 * lattice that holds `p`, each fading with distance, about -1..1.
 */
const NOISE_SIMPLEX = `fn noise_simplex_corner(cell: vec3i, d: vec3f) -> f32 {
    let t = max(0.6 - dot(d, d), 0.0);
    return t * t * t * t * noise_grad(cell, d);
}

fn noise_simplex(p: vec3f) -> f32 {
    let i = floor(p + (p.x + p.y + p.z) / 3.0);
    let x0 = p - i + (i.x + i.y + i.z) / 6.0;
    let g = step(x0.yzx, x0.xyz);
    let l = 1.0 - g;
    let i1 = min(g, l.zxy);
    let i2 = max(g, l.zxy);
    let c = vec3i(i);
    let n = noise_simplex_corner(c, x0)
        + noise_simplex_corner(c + vec3i(i1), x0 - i1 + 1.0 / 6.0)
        + noise_simplex_corner(c + vec3i(i2), x0 - i2 + 1.0 / 3.0)
        + noise_simplex_corner(c + vec3i(1), x0 - 0.5);
    return 32.0 * n;
}`;

/**
 * Four octaves of simplex noise, each twice the frequency and half the weight
 * of the one before, offset so they do not line up. `fbm` sums them, `ridged`
 * sums sharp creases where each octave crosses zero; both are 0..1.
 */
const NOISE_OCTAVES = `fn noise_fbm(p: vec3f) -> f32 {
    var sum = 0.0;
    var weight = 0.5;
    var q = p;
    for (var i = 0; i < 4; i++) {
        sum += weight * noise_simplex(q);
        q = q * 2.0 + vec3f(17.1, 31.7, 7.3);
        weight *= 0.5;
    }
    return 0.5 + sum / 0.9375 * 0.5;
}

fn noise_ridged(p: vec3f) -> f32 {
    var sum = 0.0;
    var weight = 0.5;
    var q = p;
    for (var i = 0; i < 4; i++) {
        let ridge = 1.0 - abs(noise_simplex(q));
        sum += weight * ridge * ridge;
        q = q * 2.0 + vec3f(17.1, 31.7, 7.3);
        weight *= 0.5;
    }
    return sum / 0.9375;
}`;

const NOISE_TYPES: Record<
    string,
    { helpers: readonly string[]; emit: (p: string) => string }
> = {
    simplex: {
        helpers: [NOISE_HASH, NOISE_GRAD, NOISE_SIMPLEX],
        emit: (p) => `(0.5 + 0.5 * noise_simplex(${p}))`,
    },
    perlin: {
        helpers: [NOISE_HASH, NOISE_GRAD, NOISE_PERLIN],
        emit: (p) => `(0.5 + 0.5 * noise_perlin(${p}))`,
    },
    value: {
        helpers: [NOISE_HASH, NOISE_VALUE],
        emit: (p) => `noise_value(${p})`,
    },
    fbm: {
        helpers: [NOISE_HASH, NOISE_GRAD, NOISE_SIMPLEX, NOISE_OCTAVES],
        emit: (p) => `noise_fbm(${p})`,
    },
    ridged: {
        helpers: [NOISE_HASH, NOISE_GRAD, NOISE_SIMPLEX, NOISE_OCTAVES],
        emit: (p) => `noise_ridged(${p})`,
    },
};

/**
 * Smooth noise between 0 and 1 of the algorithm `type` names, read at
 * (`x`, `y`) times `scale`: one unit of the scaled coordinates is about one
 * feature of the noise. `z` moves through the noise, which animates it when
 * fed `$v.time`.
 */
export const noise: VideoModuleDef = {
    inputs: { x: 'number', y: 'number', z: 'number', scale: 'number' },
    output: 'gray',
    params: {
        type: { values: Object.keys(NOISE_TYPES), default: 'simplex' },
    },
    helpers: ({ type }) => NOISE_TYPES[type].helpers,
    emit: ({ x, y, z, scale }, { type }) =>
        `clamp(${NOISE_TYPES[type].emit(`vec3f(vec2f(${x}, ${y}) * ${scale}, ${z})`)}, 0.0, 1.0)`,
};

/**
 * Cellular noise: the distance to the nearest of a scatter of points, one per
 * unit cell of (`x`, `y`) times `scale`, between 0 (on a point) and about 1.
 * `z` moves the points, which animates the cells when fed `$v.time`.
 */
export const voronoi: VideoModuleDef = {
    inputs: { x: 'number', y: 'number', z: 'number', scale: 'number' },
    output: 'gray',
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
    emit: ({ x, y, z, scale }) =>
        `voronoi_distance(vec2f(${x}, ${y}) * ${scale}, ${z})`,
};
