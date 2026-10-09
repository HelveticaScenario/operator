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
    max,
    maxColor,
    min,
    minColor,
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
import { fold, wrap } from './waveshape';

export const VIDEO_MODULES: Record<string, VideoModuleDef> = {
    add,
    addColor,
    colorize,
    comparator,
    diff,
    diffColor,
    feedbackRead,
    feedbackWrite,
    fold,
    hsv,
    invert,
    invertColor,
    key,
    max,
    maxColor,
    min,
    minColor,
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
    wrap,
};
