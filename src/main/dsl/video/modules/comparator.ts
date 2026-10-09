import type { VideoModuleDef } from './types';

/**
 * Threshold: 0 below `threshold`, 1 above, with a linear ramp `softness` wide
 * centered on it (a hard edge when softness is 0).
 */
export const comparator: VideoModuleDef = {
    inputs: { input: 'field', threshold: 'field', softness: 'field' },
    output: 'field',
    params: {},
    emit: ({ input, threshold, softness }) =>
        `clamp((${input} - ${threshold}) / max(${softness}, 0.00001) + 0.5, 0.0, 1.0)`,
};
