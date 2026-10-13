import type { VideoModuleDef } from './types';

/** 1 inside `size` and 0 outside, with a linear edge `softness` wide. */
const SHAPE_EDGE = `fn shape_edge(d: f32, size: f32, softness: f32) -> f32 {
    return 1.0 - clamp((d - size) / max(softness, 0.00001) + 0.5, 0.0, 1.0);
}`;

/**
 * The curve an edge follows from inside (1) to outside (0), as a function of
 * the linear edge `t`: `smooth` and `smoother` flatten at both ends so the
 * edge meets the inside and outside without a crease, `in` stays low longer
 * and rises sharply at the inside, and `out` falls sharply at the outside.
 */
const FALLOFFS: Record<string, string> = {
    linear: 't',
    smooth: 't * t * (3.0 - 2.0 * t)',
    smoother: 't * t * t * (t * (6.0 * t - 15.0) + 10.0)',
    in: 't * t',
    out: '1.0 - (1.0 - t) * (1.0 - t)',
};

const FALLOFF = { values: Object.keys(FALLOFFS), default: 'linear' };

/** The helpers an edge with `falloff` needs. */
const edgeHelpers = (falloff: string) => [
    SHAPE_EDGE,
    ...(falloff === 'linear'
        ? []
        : [
              `fn shape_falloff_${falloff}(t: f32) -> f32 {\n    return ${FALLOFFS[falloff]};\n}`,
          ]),
];

/** The edge `shape_edge(args)` shaped by `falloff`. */
const edge = (args: string, falloff: string) =>
    falloff === 'linear'
        ? `shape_edge(${args})`
        : `shape_falloff_${falloff}(shape_edge(${args}))`;

/**
 * 1 inside a shape centered on the frame, 0 outside, with an edge `softness`
 * wide following the curve `falloff` names. `mapX` and `mapY` place the shape (0.5, 0.5 is the center);
 * `size` is the half-extent as a fraction of frame height. Distances are
 * aspect-corrected, so circles stay round.
 */
export const shape: VideoModuleDef = {
    inputs: {
        mapX: 'number',
        mapY: 'number',
        size: 'number',
        softness: 'number',
    },
    output: 'gray',
    params: {
        shape: { values: ['circle', 'box', 'diamond'], default: 'circle' },
        falloff: FALLOFF,
    },
    helpers: ({ falloff }) => [
        ...edgeHelpers(falloff),
        `fn shape_offset(x: f32, y: f32) -> vec2f {
    return vec2f((x - 0.5) * u.resolution.x / u.resolution.y, y - 0.5);
}`,
    ],
    emit: ({ mapX, mapY, size, softness }, { shape: kind, falloff }) => {
        const p = `shape_offset(${mapX}, ${mapY})`;
        const distance = {
            circle: `length(${p})`,
            box: `max(abs(${p}.x), abs(${p}.y))`,
            diamond: `(abs(${p}.x) + abs(${p}.y))`,
        }[kind]!;
        return edge(`${distance}, ${size}, ${softness}`, falloff);
    },
};

/**
 * 1 inside a regular polygon centered where `mapX` and `mapY` are 0.5, with
 * one point up, 0 outside, with an edge `softness` wide following `falloff`. `size` is the
 * distance from the center to the middle of a side, as a fraction of the frame
 * height; distances are aspect-corrected.
 */
export const polygon: VideoModuleDef = {
    inputs: {
        mapX: 'number',
        mapY: 'number',
        sides: 'number',
        size: 'number',
        softness: 'number',
    },
    output: 'gray',
    params: { falloff: FALLOFF },
    helpers: ({ falloff }) => [
        ...edgeHelpers(falloff),
        `fn polygon_distance(x: f32, y: f32, sides: f32) -> f32 {
    let p = vec2f((x - 0.5) * u.resolution.x / u.resolution.y, y - 0.5);
    let wedge = 6.28318530718 / max(sides, 3.0);
    let turn = atan2(p.x, p.y) + 3.14159265359;
    return cos(floor(0.5 + turn / wedge) * wedge - turn) * length(p);
}`,
    ],
    emit: ({ mapX, mapY, sides, size, softness }, { falloff }) =>
        edge(
            `polygon_distance(${mapX}, ${mapY}, ${sides}), ${size}, ${softness}`,
            falloff,
        ),
};
