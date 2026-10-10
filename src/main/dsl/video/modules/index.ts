import { audioHistory } from './audioHistory';
import { colorize } from './colorize';
import { blur, blurColor, edges, edgesColor } from './filter';
import { feedbackRead, feedbackWrite } from './feedback';
import { contrast, hsv, hueShift, posterize, procAmp } from './color';
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
    range,
} from './math';
import {
    modulateHue,
    modulateKaleid,
    modulatePixelate,
    modulateRepeat,
    modulateRepeatX,
    modulateRepeatY,
    modulateRotate,
    modulateScale,
    modulateScrollX,
    modulateScrollY,
} from './modulate';
import { noise, voronoi } from './noise';
import { osc } from './osc';
import { grain, scanlines, vignette } from './post';
import { out } from './out';
import { ramp } from './ramp';
import { polygon, shape } from './shape';
import { source } from './source';
import { swizzle } from './swizzle';
import type { VideoModuleDef } from './types';
import { channel, displace, kaleid, pixelate, repeat, warp } from './warp';
import { fold, wrap } from './waveshape';

export const VIDEO_MODULES: Record<string, VideoModuleDef> = {
    add,
    addColor,
    audioHistory,
    blur,
    blurColor,
    channel,
    colorize,
    comparator,
    contrast,
    diff,
    diffColor,
    displace: displace.field,
    displaceColor: displace.color,
    edges,
    edgesColor,
    feedbackRead,
    feedbackWrite,
    fold,
    grain,
    hsv,
    hueShift,
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
    modulateHue: modulateHue.field,
    modulateHueColor: modulateHue.color,
    modulateKaleid: modulateKaleid.field,
    modulateKaleidColor: modulateKaleid.color,
    modulatePixelate: modulatePixelate.field,
    modulatePixelateColor: modulatePixelate.color,
    modulateRepeat: modulateRepeat.field,
    modulateRepeatColor: modulateRepeat.color,
    modulateRepeatX: modulateRepeatX.field,
    modulateRepeatXColor: modulateRepeatX.color,
    modulateRepeatY: modulateRepeatY.field,
    modulateRepeatYColor: modulateRepeatY.color,
    modulateRotate: modulateRotate.field,
    modulateRotateColor: modulateRotate.color,
    modulateScale: modulateScale.field,
    modulateScaleColor: modulateScale.color,
    modulateScrollX: modulateScrollX.field,
    modulateScrollXColor: modulateScrollX.color,
    modulateScrollY: modulateScrollY.field,
    modulateScrollYColor: modulateScrollY.color,
    noise,
    osc,
    out,
    pixelate: pixelate.field,
    pixelateColor: pixelate.color,
    polygon,
    posterize,
    procAmp,
    ramp,
    range,
    scanlines,
    swizzle,
    repeat: repeat.field,
    repeatColor: repeat.color,
    shape,
    source,
    vignette,
    voronoi,
    warp: warp.field,
    warpColor: warp.color,
    wrap,
};
