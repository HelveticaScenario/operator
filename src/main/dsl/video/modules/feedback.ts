import type { VideoModuleDef } from './types';

/**
 * The previous frame of a feedback buffer, resampled through a transform:
 * `zoom` magnifies about the center, `rotate` turns by that many turns, and
 * `shiftX`/`shiftY` move the picture by that fraction of the frame. `edge`
 * decides what lies beyond the frame border.
 */
export const feedbackRead: VideoModuleDef = {
    inputs: {
        zoom: 'field',
        rotate: 'field',
        shiftX: 'field',
        shiftY: 'field',
    },
    buffer: 'read',
    output: 'color',
    params: {
        edge: { values: ['clamp', 'repeat', 'mirror'], default: 'clamp' },
    },
    helpers: `fn feedback_coord(uv: vec2f, zoom: f32, rotate: f32, shift: vec2f) -> vec2f {
    let aspect = vec2f(u.resolution.x / u.resolution.y, 1.0);
    let p = (uv - vec2f(0.5) - shift) * aspect / max(zoom, 0.00001);
    let a = rotate * 6.28318530718;
    let c = cos(a);
    let s = sin(a);
    return vec2f(c * p.x - s * p.y, s * p.x + c * p.y) / aspect + vec2f(0.5);
}

fn feedback_clamp(q: vec2f) -> vec2f {
    return clamp(q, vec2f(0.0), vec2f(1.0));
}

fn feedback_repeat(q: vec2f) -> vec2f {
    return fract(q);
}

fn feedback_mirror(q: vec2f) -> vec2f {
    return vec2f(1.0) - abs(vec2f(1.0) - 2.0 * fract(q * 0.5));
}

fn feedback_texcoord(q: vec2f) -> vec2f {
    return vec2f(q.x, 1.0 - q.y);
}`,
    emit: ({ zoom, rotate, shiftX, shiftY }, { edge }, buffer) =>
        `textureSampleLevel(fb_${buffer}, fb_sampler, feedback_texcoord(feedback_${edge}(feedback_coord(uv, ${zoom}, ${rotate}, vec2f(${shiftX}, ${shiftY})))), 0.0).rgb`,
};

/** Stores its input in the feedback buffer for the next frame to read. */
export const feedbackWrite: VideoModuleDef = {
    inputs: { input: 'color' },
    buffer: 'write',
    output: 'color',
    params: {},
    emit: ({ input }) => input,
};
