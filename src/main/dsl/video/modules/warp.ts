import { coordinateModule, TRANSFORM_HELPER } from './transform';

/** Zooms about the center, turns and shifts everything `input` draws. */
export const warp = coordinateModule(
    ['zoom', 'rotate', 'shiftX', 'shiftY'],
    ({ zoom, rotate, shiftX, shiftY }) =>
        `video_transform(uv, ${zoom}, ${rotate}, vec2f(${shiftX}, ${shiftY}))`,
    [TRANSFORM_HELPER],
);

/** Reads `input` at positions pushed by `dx` and `dy`, where 0.5 is no push. */
export const displace = coordinateModule(
    ['dx', 'dy', 'amount'],
    ({ dx, dy, amount }) =>
        `uv + (vec2f(${dx}, ${dy}) - vec2f(0.5)) * ${amount}`,
);

/** `video_mod`: `x` modulo `m` rounded down, so negative `x` wraps like positive; WGSL's `%` truncates. */
export const MOD_HELPER = `fn video_mod(x: f32, m: f32) -> f32 {
    return x - m * floor(x / m);
}`;

/**
 * `video_kaleid`: mirrors coordinates around the center into `sides` wedges,
 * with the distance from the center pushed out by `push`.
 */
export const KALEID_HELPER = `fn video_kaleid(uv: vec2f, sides: f32, push: f32) -> vec2f {
    let aspect = vec2f(u.resolution.x / u.resolution.y, 1.0);
    let p = (uv - vec2f(0.5)) * aspect;
    let wedge = 6.28318530718 / max(sides, 1.0);
    let turn = video_mod(atan2(p.y, p.x), wedge);
    let folded = abs(turn - wedge * 0.5);
    return vec2f(cos(folded), sin(folded)) * (length(p) + push) / aspect + vec2f(0.5);
}`;

/** Mirrors `input` around the center into `sides` wedges. */
export const kaleid = coordinateModule(
    ['sides'],
    ({ sides }) => `video_kaleid(uv, ${sides}, 0.0)`,
    [MOD_HELPER, KALEID_HELPER],
);

/** `video_pixelate`: snaps `uv` to the center of its cell in a grid of `cells`, at least one cell each way. */
export const PIXELATE_HELPER = `fn video_pixelate(uv: vec2f, cells: vec2f) -> vec2f {
    let grid = max(cells, vec2f(1.0));
    return (floor(uv * grid) + vec2f(0.5)) / grid;
}`;

/** Holds `input` constant across a grid of `x` by `y` cells. */
export const pixelate = coordinateModule(
    ['x', 'y'],
    ({ x, y }) => `video_pixelate(uv, vec2f(${x}, ${y}))`,
    [PIXELATE_HELPER],
);

/** Tiles `input` `x` by `y` times across the frame. */
export const repeat = coordinateModule(
    ['x', 'y'],
    ({ x, y }) => `fract(uv * vec2f(${x}, ${y}))`,
);
