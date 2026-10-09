import { colorize } from './colorize';
import { feedbackRead, feedbackWrite } from './feedback';
import { hsv, posterize, procAmp } from './color';
import { comparator } from './comparator';
import { key } from './key';
import {
    add,
    addColor,
    diff,
    diffColor,
    invert,
    invertColor,
    mix,
    mixColor,
    mult,
    multColor,
} from './math';
import { osc } from './osc';
import { out } from './out';
import { ramp } from './ramp';
import { shape } from './shape';
import type { VideoModuleDef } from './types';

export const VIDEO_MODULES: Record<string, VideoModuleDef> = {
    add,
    addColor,
    colorize,
    comparator,
    diff,
    diffColor,
    feedbackRead,
    feedbackWrite,
    hsv,
    invert,
    invertColor,
    key,
    mix,
    mixColor,
    mult,
    multColor,
    osc,
    out,
    posterize,
    procAmp,
    ramp,
    shape,
};
