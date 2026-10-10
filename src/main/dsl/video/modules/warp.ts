import { TRANSFORM_HELPER } from './transform';
import type { VideoModuleDef } from './types';
import type { VideoValueType } from '../../../../shared/video/videoGraph';

/**
 * Builds the field and color variants of a module that evaluates its `input`
 * at coordinates it computes, so the input's whole sub-patch is moved, turned
 * or folded rather than just its output.
 */
function coordinateModule(
    extraInputs: readonly string[],
    natural: readonly string[],
    coordinates: (args: Record<string, string>) => string,
    helpers: readonly string[] = [],
): { field: VideoModuleDef; color: VideoModuleDef } {
    const make = (type: VideoValueType): VideoModuleDef => ({
        inputs: {
            input: type,
            ...Object.fromEntries(extraInputs.map((name) => [name, 'field'])),
        },
        warped: ['input'],
        natural,
        output: type,
        params: {},
        helpers,
        emit: (args) => `${args.input}(${coordinates(args)})`,
    });
    return { field: make('field'), color: make('color') };
}

/** Zooms about the center, turns and shifts everything `input` draws. */
export const warp = coordinateModule(
    ['zoom', 'rotate', 'shiftX', 'shiftY'],
    ['zoom', 'rotate'],
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

/** Mirrors `input` around the center into `sides` wedges. */
export const kaleid = coordinateModule(
    ['sides'],
    ['sides'],
    ({ sides }) => `video_kaleid(uv, ${sides})`,
    [
        `fn video_kaleid(uv: vec2f, sides: f32) -> vec2f {
    let aspect = vec2f(u.resolution.x / u.resolution.y, 1.0);
    let p = (uv - vec2f(0.5)) * aspect;
    let wedge = 6.28318530718 / max(sides, 1.0);
    let turn = ((atan2(p.y, p.x) % wedge) + wedge) % wedge;
    let folded = abs(turn - wedge * 0.5);
    return vec2f(cos(folded), sin(folded)) * length(p) / aspect + vec2f(0.5);
}`,
    ],
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

/** One channel of a color as a field: red, green, blue, or brightness. */
export const channel: VideoModuleDef = {
    inputs: { input: 'color' },
    output: 'field',
    params: {
        channel: { values: ['r', 'g', 'b', 'luma'], default: 'luma' },
    },
    emit: ({ input }, { channel: which }) =>
        which === 'luma'
            ? `dot(${input}, vec3f(0.299, 0.587, 0.114))`
            : `${input}.${which}`,
};
