import { colorize } from './colorize';
import { osc } from './osc';
import { out } from './out';
import { ramp } from './ramp';
import type { VideoModuleDef } from './types';

export const VIDEO_MODULES: Record<string, VideoModuleDef> = {
    colorize,
    osc,
    out,
    ramp,
};
