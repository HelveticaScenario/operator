import type { VideoModuleDef } from './types';

/** Combines three fields into a color, clipping each channel to 0..1. */
export const colorize: VideoModuleDef = {
    inputs: { r: 'field', g: 'field', b: 'field' },
    output: 'color',
    params: {},
    emit: ({ r, g, b }) =>
        `clamp(vec3f(${r}, ${g}, ${b}), vec3f(0.0), vec3f(1.0))`,
};
