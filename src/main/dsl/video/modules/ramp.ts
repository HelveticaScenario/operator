import { TRANSFORM_HELPER } from './transform';
import type { VideoModuleDef } from './types';

const IDENTITY = ['1.0', '0.0', '0.0', '0.0'];

/**
 * Scan ramp over the frame, optionally zoomed, rotated and shifted:
 * `h` and `v` run 0 to 1 across and up, `d` along the diagonal, `r` is the
 * distance from the center (0.5 at the top and bottom edges), and `a` is the
 * angle around the center from 0 to 1.
 */
export const ramp: VideoModuleDef = {
    inputs: {
        zoom: 'field',
        rotate: 'field',
        shiftX: 'field',
        shiftY: 'field',
    },
    natural: ['zoom'],
    output: 'field',
    params: {
        axis: { values: ['h', 'v', 'd', 'r', 'a'], default: 'h' },
    },
    helpers: [
        TRANSFORM_HELPER,
        `fn ramp_radius(q: vec2f) -> f32 {
    return length((q - vec2f(0.5)) * vec2f(u.resolution.x / u.resolution.y, 1.0));
}`,
        `fn ramp_angle(q: vec2f) -> f32 {
    let p = (q - vec2f(0.5)) * vec2f(u.resolution.x / u.resolution.y, 1.0);
    return atan2(p.y, p.x) / 6.28318530718 + 0.5;
}`,
    ],
    emit: ({ zoom, rotate, shiftX, shiftY }, { axis }) => {
        const untouched = [zoom, rotate, shiftX, shiftY].every(
            (arg, i) => arg === IDENTITY[i],
        );
        const q = untouched
            ? 'uv'
            : `video_transform(uv, ${zoom}, ${rotate}, vec2f(${shiftX}, ${shiftY}))`;
        return {
            h: `${q}.x`,
            v: `${q}.y`,
            d: `dot(${q}, vec2f(0.5))`,
            r: `ramp_radius(${q})`,
            a: `ramp_angle(${q})`,
        }[axis]!;
    },
};
