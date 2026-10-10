import { coordinateModule, TRANSFORM_HELPER } from './transform';

/** Zooms about the center, turns and shifts everything `input` draws. */
export const warp = coordinateModule(
    ['zoom', 'rotate', 'shiftX', 'shiftY'],
    ['zoom'],
    ({ zoom, rotate, shiftX, shiftY }) =>
        `video_transform(uv, ${zoom}, ${rotate}, vec2f(${shiftX}, ${shiftY}))`,
    [TRANSFORM_HELPER],
);

/** Reads `input` at positions pushed by `dx` and `dy`, where 0.5 is no push. */
export const displace = coordinateModule(
    ['dx', 'dy', 'amount'],
    [],
    ({ dx, dy, amount }) =>
        `uv + (vec2f(${dx}, ${dy}) - vec2f(0.5)) * ${amount}`,
);

/**
 * `video_kaleid`: mirrors coordinates around the center into `sides` wedges,
 * with the distance from the center pushed out by `push`.
 */
export const KALEID_HELPER = `fn video_kaleid(uv: vec2f, sides: f32, push: f32) -> vec2f {
    let aspect = vec2f(u.resolution.x / u.resolution.y, 1.0);
    let p = (uv - vec2f(0.5)) * aspect;
    let wedge = 6.28318530718 / max(sides, 1.0);
    let turn = ((atan2(p.y, p.x) % wedge) + wedge) % wedge;
    let folded = abs(turn - wedge * 0.5);
    return vec2f(cos(folded), sin(folded)) * (length(p) + push) / aspect + vec2f(0.5);
}`;

/** Mirrors `input` around the center into `sides` wedges. */
export const kaleid = coordinateModule(
    ['sides'],
    ['sides'],
    ({ sides }) => `video_kaleid(uv, ${sides}, 0.0)`,
    [KALEID_HELPER],
);

/** Holds `input` constant across a grid of `x` by `y` cells. */
export const pixelate = coordinateModule(
    ['x', 'y'],
    ['x', 'y'],
    ({ x, y }) => `video_pixelate(uv, vec2f(${x}, ${y}))`,
    [
        `fn video_pixelate(uv: vec2f, cells: vec2f) -> vec2f {
    let grid = max(cells, vec2f(1.0));
    return (floor(uv * grid) + vec2f(0.5)) / grid;
}`,
    ],
);

/** Tiles `input` `x` by `y` times across the frame. */
export const repeat = coordinateModule(
    ['x', 'y'],
    ['x', 'y'],
    ({ x, y }) => `fract(uv * vec2f(${x}, ${y}))`,
);
