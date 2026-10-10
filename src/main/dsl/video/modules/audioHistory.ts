import type { VideoModuleDef } from './types';

/**
 * The recent audio-rate samples of a signal laid along `position`: 0 is the
 * oldest sample in the window and 1 the newest. The texture holds volts as the
 * audio graph produces them and is interpolated between samples; the field is
 * those volts as a fraction of 5.
 */
export const audioHistory: VideoModuleDef = {
    inputs: { position: 'field', samples: 'field' },
    natural: ['samples'],
    history: true,
    output: 'field',
    params: {},
    helpers: [
        `fn history_sample(row: i32, position: f32, count: f32) -> f32 {
    let x = clamp(position, 0.0, 1.0) * max(count - 1.0, 0.0);
    let i = i32(floor(x));
    let last = max(i32(count) - 1, 0);
    let a = textureLoad(history_tex, vec2i(min(i, last), row), 0).r;
    let b = textureLoad(history_tex, vec2i(min(i + 1, last), row), 0).r;
    return mix(a, b, fract(x));
}`,
    ],
    emit: ({ position, samples }, _params, { history }) =>
        `(history_sample(${history}, ${position}, ${samples}) * 0.2)`,
};
