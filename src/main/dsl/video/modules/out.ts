import type { VideoModuleDef } from './types';

/** Final stage before the display; clips to the displayable range. */
export const out: VideoModuleDef = {
    inputs: { input: 'color' },
    output: 'color',
    params: {},
    emit: ({ input }) => `clamp(${input}, vec3f(0.0), vec3f(1.0))`,
};
