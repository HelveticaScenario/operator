import type { VideoModuleDef } from './types';

/** 1 inside `size` and 0 outside, with a linear edge `softness` wide. */
const SHAPE_EDGE = `fn shape_edge(d: f32, size: f32, softness: f32) -> f32 {
    return 1.0 - clamp((d - size) / max(softness, 0.00001) + 0.5, 0.0, 1.0);
}`;

/**
 * 1 inside a shape centered on the frame, 0 outside, with a linear edge
 * `softness` wide. `x` and `y` place the shape (0.5, 0.5 is the center);
 * `size` is the half-extent as a fraction of frame height. Distances are
 * aspect-corrected, so circles stay round.
 */
export const shape: VideoModuleDef = {
    inputs: { x: 'field', y: 'field', size: 'field', softness: 'field' },
    output: 'field',
    params: {
        shape: { values: ['circle', 'box', 'diamond'], default: 'circle' },
    },
    helpers: [
        SHAPE_EDGE,
        `fn shape_offset(x: f32, y: f32) -> vec2f {
    return vec2f((x - 0.5) * u.resolution.x / u.resolution.y, y - 0.5);
}`,
    ],
    emit: ({ x, y, size, softness }, { shape: kind }) => {
        const p = `shape_offset(${x}, ${y})`;
        const distance = {
            circle: `length(${p})`,
            box: `max(abs(${p}.x), abs(${p}.y))`,
            diamond: `(abs(${p}.x) + abs(${p}.y))`,
        }[kind]!;
        return `shape_edge(${distance}, ${size}, ${softness})`;
    },
};

/**
 * 1 inside a regular polygon centered on (x, y) with one point up, 0 outside,
 * with a linear edge `softness` wide. `size` is the distance from the center to
 * the middle of a side, as a fraction of the frame height; distances are
 * aspect-corrected.
 */
export const polygon: VideoModuleDef = {
    inputs: {
        x: 'field',
        y: 'field',
        sides: 'field',
        size: 'field',
        softness: 'field',
    },
    natural: ['sides'],
    output: 'field',
    helpers: [
        SHAPE_EDGE,
        `fn polygon_distance(x: f32, y: f32, sides: f32) -> f32 {
    let p = vec2f((x - 0.5) * u.resolution.x / u.resolution.y, y - 0.5);
    let wedge = 6.28318530718 / max(sides, 3.0);
    let turn = atan2(p.x, p.y) + 3.14159265359;
    return cos(floor(0.5 + turn / wedge) * wedge - turn) * length(p);
}`,
    ],
    emit: ({ x, y, sides, size, softness }) =>
        `shape_edge(polygon_distance(${x}, ${y}, ${sides}), ${size}, ${softness})`,
};
