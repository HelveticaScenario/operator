import type { VideoModuleDef } from './types';

/** Multiplies by `gain`, then keeps the fractional part: a sawtooth of the input. */
export const wrap: VideoModuleDef = {
    inputs: { input: 'field', gain: 'field' },
    output: 'field',
    params: {},
    emit: ({ input, gain }) => `fract(${input} * ${gain})`,
};

/**
 * Multiplies by `gain`, then reflects whatever passes 1 back down: a triangle
 * of the input that matches it up to 1 / gain.
 */
export const fold: VideoModuleDef = {
    inputs: { input: 'field', gain: 'field' },
    output: 'field',
    params: {},
    emit: ({ input, gain }) =>
        `(1.0 - abs(1.0 - 2.0 * fract(${input} * ${gain} * 0.5)))`,
};
