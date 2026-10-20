import type { VideoModuleDef } from './types';

/**
 * An XY scope's beam, read at the coordinate being drawn: white, with the
 * beam's brightness as its alpha, so it shows that brightness over black and
 * the background is transparent.
 */
export const scopeXY: VideoModuleDef = {
    inputs: {},
    scope: true,
    output: 'rgba',
    helpers: [
        `fn video_scope(beam: texture_2d<f32>, uv: vec2f) -> vec4f {
    let level = textureSampleLevel(beam, fb_sampler, vec2f(uv.x, 1.0 - uv.y), 0.0).r;
    return vec4f(1.0, 1.0, 1.0, level);
}`,
    ],
    emit: (_args, _params, indices) =>
        `video_scope(scope_${indices.scope}, uv)`,
};
