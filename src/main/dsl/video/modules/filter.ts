import type { VideoModuleDef } from './types';
import type { VideoValueType } from '../../../../shared/video/videoGraph';

/** Samples per blur; the taps are spread over a disk in a golden-angle spiral. */
const BLUR_TAPS = 16;

const GOLDEN_ANGLE = 2.399963229728653;

const BLUR_OFFSETS = Array.from({ length: BLUR_TAPS }, (_, i) => {
    const radius = Math.sqrt((i + 0.5) / BLUR_TAPS);
    const angle = i * GOLDEN_ANGLE;
    return [radius * Math.cos(angle), radius * Math.sin(angle)] as const;
});

const wgsl = (value: number) => value.toFixed(6);

/**
 * Averages `input` over a disk of `radius` (a fraction of the frame height) by
 * reading the whole input at {@link BLUR_TAPS} nearby coordinates. The tap
 * pattern is turned by a per-pixel hash, so the gaps between taps show as fine
 * grain rather than as visible copies of the input.
 */
function blurModule(type: VideoValueType): VideoModuleDef {
    return {
        inputs: { input: type, radius: 'field' },
        warped: ['input'],
        output: type,
        helpers: [
            `fn blur_offset(tap: vec2f, uv: vec2f, radius: f32) -> vec2f {
    let pixel = floor(uv * u.resolution);
    let turn = 6.28318530718 * fract(52.9829189 * fract(dot(pixel, vec2f(0.06711056, 0.00583715))));
    let c = cos(turn);
    let s = sin(turn);
    let rotated = vec2f(tap.x * c - tap.y * s, tap.x * s + tap.y * c);
    return rotated * vec2f(radius * u.resolution.y / u.resolution.x, radius);
}`,
        ],
        emit: ({ input, radius }) => {
            const taps = BLUR_OFFSETS.map(
                ([x, y]) =>
                    `${input}(uv + blur_offset(vec2f(${wgsl(x)}, ${wgsl(y)}), uv, ${radius}))`,
            );
            return `(${taps.join(' + ')}) / ${BLUR_TAPS}.0`;
        },
    };
}

export const blur = { field: blurModule('field'), color: blurModule('color') };

const LUMA = 'vec3f(0.299, 0.587, 0.114)';

/**
 * Brightness of the steepest change around each pixel (a Sobel filter): 0 on
 * flat areas, rising at edges. Reads `input` at its eight neighbors one pixel
 * away, so it sees the same pixel grid whatever the window size.
 */
function edgesModule(type: VideoValueType): VideoModuleDef {
    const brightness = (call: string) =>
        type === 'color' ? `dot(${call}, ${LUMA})` : call;
    return {
        inputs: { input: type, amount: 'field' },
        warped: ['input'],
        output: 'field',
        helpers: [
            `fn sobel_magnitude(tl: f32, t: f32, tr: f32, l: f32, r: f32, bl: f32, b: f32, br: f32) -> f32 {
    let gx = -tl - 2.0 * l - bl + tr + 2.0 * r + br;
    let gy = -tl - 2.0 * t - tr + bl + 2.0 * b + br;
    return sqrt(gx * gx + gy * gy);
}`,
        ],
        emit: ({ input, amount }) => {
            const at = (dx: number, dy: number) =>
                brightness(
                    `${input}(uv + vec2f(${dx}.0, ${dy}.0) / u.resolution)`,
                );
            return `clamp(sobel_magnitude(${at(-1, 1)}, ${at(0, 1)}, ${at(1, 1)}, ${at(-1, 0)}, ${at(1, 0)}, ${at(-1, -1)}, ${at(0, -1)}, ${at(1, -1)}) * ${amount}, 0.0, 1.0)`;
        },
    };
}

export const edges = {
    field: edgesModule('field'),
    color: edgesModule('color'),
};
