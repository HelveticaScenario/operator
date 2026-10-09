import type { VideoModuleDef } from './types';

/** Scan ramp over the frame: 0 to 1 along the chosen axis. */
export const ramp: VideoModuleDef = {
    inputs: {},
    output: 'field',
    params: { axis: { values: ['h', 'v', 'd'], default: 'h' } },
    emit: (_args, params) =>
        ({
            h: 'uv.x',
            v: 'uv.y',
            d: '(uv.x + uv.y) * 0.5',
        })[params.axis]!,
};
