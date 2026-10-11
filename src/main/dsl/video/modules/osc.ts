import type { VideoModuleDef } from './types';

/** Periodic shaper: `freq` cycles per unit of `input`, offset by `phase` cycles. */
export const osc: VideoModuleDef = {
    inputs: { input: 'field', freq: 'field', phase: 'field' },
    output: 'field',
    params: {
        shape: {
            values: ['sine', 'triangle', 'saw', 'square'],
            default: 'sine',
        },
    },
    emit: ({ input, freq, phase }, { shape }) => {
        const p = `fract(${input} * ${freq} + ${phase})`;
        return {
            sine: `(0.5 + 0.5 * sin(6.28318530718 * ${p}))`,
            triangle: `abs(2.0 * ${p} - 1.0)`,
            saw: p,
            square: `step(0.5, ${p})`,
        }[shape]!;
    },
};
