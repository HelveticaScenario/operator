import { audioHistory } from './audioHistory';
import { channel, contrast, hsv, hueShift, posterize, procAmp } from './color';
import { feedbackRead, feedbackWrite } from './feedback';
import { blur, edges } from './filter';
import {
    add,
    colorize,
    comparator,
    diff,
    invert,
    key,
    max,
    min,
    mix,
    mult,
    out,
    range,
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
import { fold, wrap } from './waveshape';

type Variants = { field: VideoModuleDef; color: VideoModuleDef };

/** Registers each module with variants as `<name>` for fields and `<name>Color` for colors. */
function withVariants(
    modules: Record<string, Variants>,
): Record<string, VideoModuleDef> {
    return Object.fromEntries(
        Object.entries(modules).flatMap(([name, { field, color }]) => [
            [name, field],
            [`${name}Color`, color],
        ]),
    );
}

export const VIDEO_MODULES: Record<string, VideoModuleDef> = {
    ...withVariants({
        add,
        blur,
        diff,
        displace,
        edges,
        invert,
        kaleid,
        max,
        min,
        mix,
        mult,
        pixelate,
        repeat,
        warp,
        ...modulators,
    }),
    audioHistory,
    channel,
    colorize,
    comparator,
    contrast,
    feedbackRead,
    feedbackWrite,
    fold,
    grain,
    hsv,
    hueShift,
    key,
    noise,
    osc,
    out,
    polygon,
    posterize,
    procAmp,
    ramp,
    range,
    scanlines,
    shape,
    source,
    swizzle,
    vignette,
    voronoi,
    wrap,
};
