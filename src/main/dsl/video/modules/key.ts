import type { VideoModuleDef } from './types';

/** Shows `fg` where `mask` is 1 and `bg` where it is 0. */
export const key: VideoModuleDef = {
    inputs: { fg: 'color', bg: 'color', mask: 'field' },
    output: 'color',
    params: {},
    emit: ({ fg, bg, mask }) => `mix(${bg}, ${fg}, clamp(${mask}, 0.0, 1.0))`,
};
