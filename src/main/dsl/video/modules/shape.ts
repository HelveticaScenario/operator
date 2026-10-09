import type { VideoModuleDef } from './types';

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
        `fn shape_edge(d: f32, size: f32, softness: f32) -> f32 {
    return 1.0 - clamp((d - size) / max(softness, 0.00001) + 0.5, 0.0, 1.0);
}`,
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
