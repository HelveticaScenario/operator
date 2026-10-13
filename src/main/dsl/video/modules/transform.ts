import type { VideoModuleDef } from './types';

/**
 * `video_transform`: maps frame coordinates `uv` through a zoom about the
 * center, a rotation in turns (positive is clockwise) and a shift, correcting
 * for the window's aspect ratio so rotation never shears. A negative zoom
 * mirrors through the center; its size is kept off zero. Used by ramps to move
 * their pattern and by feedback to resample the previous frame.
 */
export const TRANSFORM_HELPER = `fn video_transform(uv: vec2f, zoom: f32, rotate: f32, shift: vec2f) -> vec2f {
    let aspect = vec2f(u.resolution.x / u.resolution.y, 1.0);
    let z = select(-1.0, 1.0, zoom >= 0.0) * max(abs(zoom), 0.00001);
    let p = (uv - vec2f(0.5) - shift) * aspect / z;
    let a = rotate * 6.28318530718;
    let c = cos(a);
    let s = sin(a);
    return vec2f(c * p.x - s * p.y, s * p.x + c * p.y) / aspect + vec2f(0.5);
}`;

/**
 * A module that evaluates its `input` at coordinates it computes, so the
 * input's whole sub-patch is moved, turned or folded rather than just its
 * output. `numbers` are its further inputs, read as numbers, and `rgba` those
 * read whole.
 */
export function coordinateModule(
    numbers: readonly string[],
    coordinates: (args: Record<string, string>) => string,
    helpers: readonly string[] = [],
    rgba: readonly string[] = [],
): VideoModuleDef {
    return {
        inputs: {
            input: 'rgba',
            ...Object.fromEntries(numbers.map((name) => [name, 'number'])),
            ...Object.fromEntries(rgba.map((name) => [name, 'rgba'])),
        },
        warped: ['input'],
        output: 'rgba',
        helpers,
        emit: (args) => `${args.input}(${coordinates(args)})`,
    };
}
