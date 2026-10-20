import type { VideoModuleDef } from './types';

const CHANNELS = ['r', 'g', 'b', 'a'];

const PATTERNS = CHANNELS.flatMap((a) =>
    CHANNELS.flatMap((b) => CHANNELS.map((c) => `${a}${b}${c}`)),
);

/**
 * Reorders a signal's channels: each letter of `pattern` names the input
 * channel that fills red, green and blue in turn; the alpha is kept.
 */
export const swizzle: VideoModuleDef = {
    inputs: { input: 'rgba' },
    output: 'rgba',
    params: {
        pattern: { values: PATTERNS, default: 'rgb' },
    },
    helpers: ({ pattern }) => [
        `fn video_swizzle_${pattern}(c: vec4f) -> vec4f {\n    return vec4f(c.${pattern}, c.a);\n}`,
    ],
    emit: ({ input }, { pattern }) => `video_swizzle_${pattern}(${input})`,
};
