import { audioHistory } from './audioHistory';
import {
    brightness,
    channel,
    color,
    colorama,
    contrast,
    hsv,
    hue,
    procAmp,
    saturate,
    shift,
} from './color';
import { feedbackRead, feedbackWrite } from './feedback';
import { blur, edges } from './filter';
import {
    add,
    blend,
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
    solid,
    thresh,
    wrap,
} from './math';
import * as modulators from './modulate';
import { noise, voronoi } from './noise';
import { osc } from './osc';
import { grain, scanlines, vignette } from './post';
import { ramp } from './ramp';
import { scopeXY } from './scopeXY';
import { polygon, shape } from './shape';
import { source } from './source';
import { swizzle } from './swizzle';
import type { VideoModuleDef } from './types';
import { displace, kaleid, pixelate, repeat, warp } from './warp';

export const VIDEO_MODULES: Record<string, VideoModuleDef> = {
    ...modulators,
    add,
    audioHistory,
    blend,
    blur,
    brightness,
    channel,
    color,
    colorama,
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
    hue,
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
    saturate,
    scanlines,
    scopeXY,
    shape,
    shift,
    solid,
    source,
    swizzle,
    thresh,
    vignette,
    voronoi,
    warp,
    wrap,
};
