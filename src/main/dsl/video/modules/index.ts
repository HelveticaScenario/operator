import { audioHistory } from './audioHistory';
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
import { noise } from './noise';
import { osc } from './osc';
import { out } from './out';
import { ramp } from './ramp';
import { shape } from './shape';
import type { VideoModuleDef } from './types';
import { channel, displace, kaleid, pixelate, repeat, warp } from './warp';
import { fold, wrap } from './waveshape';

export const VIDEO_MODULES: Record<string, VideoModuleDef> = {
    add,
    addColor,
    audioHistory,
    channel,
    colorize,
    comparator,
    diff,
    diffColor,
    displace: displace.field,
    displaceColor: displace.color,
    feedbackRead,
    feedbackWrite,
    fold,
    hsv,
    invert,
    invertColor,
    kaleid: kaleid.field,
    kaleidColor: kaleid.color,
    key,
    max,
    maxColor,
    min,
    minColor,
    mix,
    mixColor,
    mult,
    multColor,
    noise,
    osc,
    out,
    pixelate: pixelate.field,
    pixelateColor: pixelate.color,
    posterize,
    procAmp,
    ramp,
    repeat: repeat.field,
    repeatColor: repeat.color,
    shape,
    warp: warp.field,
    warpColor: warp.color,
    wrap,
};
