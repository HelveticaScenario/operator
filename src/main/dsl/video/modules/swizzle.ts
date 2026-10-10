import type { VideoModuleDef } from './types';

const CHANNELS = ['r', 'g', 'b'];

const PATTERNS = CHANNELS.flatMap((a) =>
    CHANNELS.flatMap((b) => CHANNELS.map((c) => `${a}${b}${c}`)),
);

/** Reorders a color's channels: each letter of `pattern` names the input channel that fills red, green and blue in turn. */
export const swizzle: VideoModuleDef = {
    inputs: { input: 'color' },
    output: 'color',
    params: {
        pattern: { values: PATTERNS, default: 'rgb' },
    },
    emit: ({ input }, { pattern }) => `${input}.${pattern}`,
};
