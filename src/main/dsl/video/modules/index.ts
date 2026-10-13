import { audioHistory } from './audioHistory';
import { channel, contrast, hsv, hueShift, procAmp } from './color';
import { feedbackRead, feedbackWrite } from './feedback';
import { blur, edges } from './filter';
import {
    add,
    colorize,
    comparator,
    diff,
    fold,
    invert,
    key,
    layer,
    luma,
    mask,
    max,
    min,
    mix,
    mult,
    out,
    posterize,
    range,
    wrap,
} from './math';
import * as modulators from './modulate';
import { noise, voronoi } from './noise';
import { osc } from './osc';
import { grain, scanlines, vignette } from './post';
import { ramp } from './ramp';
import { polygon, shape } from './shape';
import { source } from './source';
import { swizzle } from './swizzle';
import type { VideoModuleDef } from './types';
import { displace, kaleid, pixelate, repeat, warp } from './warp';

export const VIDEO_MODULES: Record<string, VideoModuleDef> = {
    ...modulators,
    add,
    audioHistory,
    blur,
    channel,
    colorize,
    comparator,
    contrast,
    diff,
    displace,
    edges,
    feedbackRead,
    feedbackWrite,
    fold,
    grain,
    hsv,
    hueShift,
    invert,
    kaleid,
    key,
    layer,
    luma,
    mask,
    max,
    min,
    mix,
    mult,
    noise,
    osc,
    out,
    pixelate,
    polygon,
    posterize,
    procAmp,
    ramp,
    range,
    repeat,
    scanlines,
    shape,
    source,
    swizzle,
    vignette,
    voronoi,
    warp,
    wrap,
};
