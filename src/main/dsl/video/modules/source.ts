import type { VideoModuleDef } from './types';

/** How a source that does not match the frame's aspect ratio is fitted. */
const FIT_CODES: Record<string, number> = { cover: 1, contain: 2, stretch: 0 };

/**
 * A picture or recording from the workspace folder, read at the coordinate
 * being drawn, so warps and feedback move it like any other pattern. `cover`
 * fills the frame and crops the overflow, `contain` fits the whole picture and
 * leaves black bars, and `stretch` ignores the aspect ratio.
 */
export const source: VideoModuleDef = {
    inputs: {},
    source: true,
    output: 'color',
    params: {
        fit: { values: ['cover', 'contain', 'stretch'], default: 'cover' },
    },
    helpers: [
        `fn video_source(media: texture_2d<f32>, uv: vec2f, fit: i32) -> vec3f {
    let size = vec2f(textureDimensions(media));
    let frame = u.resolution.x / u.resolution.y;
    let picture = size.x / size.y;
    var p = uv;
    if (fit == 1) {
        if (picture > frame) {
            p.x = 0.5 + (uv.x - 0.5) * frame / picture;
        } else {
            p.y = 0.5 + (uv.y - 0.5) * picture / frame;
        }
    } else if (fit == 2) {
        if (picture > frame) {
            p.y = 0.5 + (uv.y - 0.5) * picture / frame;
        } else {
            p.x = 0.5 + (uv.x - 0.5) * frame / picture;
        }
        if (p.x < 0.0 || p.x > 1.0 || p.y < 0.0 || p.y > 1.0) {
            return vec3f(0.0);
        }
    }
    return textureSampleLevel(media, fb_sampler, vec2f(p.x, 1.0 - p.y), 0.0).rgb;
}`,
    ],
    emit: (_args, { fit }, indices) =>
        `video_source(src_${indices.source}, uv, ${FIT_CODES[fit]})`,
};
