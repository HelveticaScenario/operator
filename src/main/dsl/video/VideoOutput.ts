import type {
    VideoValue,
    VideoValueType,
} from '../../../shared/video/videoGraph';

/** The graph builder's `$v` functions, which chain methods forward to. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type VideoOps = Record<string, (...args: any[]) => any>;

/**
 * A video signal: a node's output, a constant, or the time field. Besides
 * being passed to `$v` functions, it can be chained from: each method here is
 * the `$v` function of the same name with this signal as its first argument,
 * so `$v.kaleid($v.rotate(x, 0.1), 6)` reads as `x.rotate(0.1).kaleid(6)`.
 */
export class VideoOutput {
    constructor(
        readonly value: VideoValue,
        readonly type: VideoValueType,
        /** The builder that made this signal; chain methods call its functions. */
        readonly ops: object,
    ) {}
}

type ChainCall = (
    ops: VideoOps,
    self: VideoOutput,
    ...args: unknown[]
) => unknown;

/** How each chain method calls the builder, given the signal it is chained from. */
const CHAIN: Record<string, ChainCall> = {
    add: (o, s, b) => o.add(s, b),
    channel: (o, s, which) => o.channel(s, which),
    comparator: (o, s, threshold, softness) =>
        o.comparator(s, threshold, softness),
    diff: (o, s, b) => o.diff(s, b),
    displace: (o, s, dx, dy, amount) => o.displace(s, dx, dy, amount),
    fold: (o, s, gain) => o.fold(s, gain),
    hsv: (o, s, saturation, value) => o.hsv(s, saturation, value),
    invert: (o, s) => o.invert(s),
    kaleid: (o, s, sides) => o.kaleid(s, sides),
    key: (o, s, background, mask) => o.key(s, background, mask),
    max: (o, s, b) => o.max(s, b),
    min: (o, s, b) => o.min(s, b),
    mix: (o, s, b, amount) => o.mix(s, b, amount),
    modulate: (o, s, modulator, amount) => o.modulate(s, modulator, amount),
    mult: (o, s, b) => o.mult(s, b),
    out: (o, s) => o.out(s),
    pixelate: (o, s, x, y) => o.pixelate(s, x, y),
    posterize: (o, s, levels) => o.posterize(s, levels),
    preview: (o, s, config) => o.preview(s, config),
    procAmp: (o, s, gain, bias, saturation) =>
        o.procAmp(s, gain, bias, saturation),
    repeat: (o, s, x, y) => o.repeat(s, x, y),
    rotate: (o, s, turns) => o.warp(s, { rotate: turns }),
    scale: (o, s, zoom) => o.warp(s, { zoom }),
    scroll: (o, s, x = 0, y = 0) => o.warp(s, { shiftX: x, shiftY: y }),
    toCV: (o, s, config) => o.toCV(s, config),
    warp: (o, s, config) => o.warp(s, config),
    wrap: (o, s, gain) => o.wrap(s, gain),
};

for (const [name, call] of Object.entries(CHAIN)) {
    Object.defineProperty(VideoOutput.prototype, name, {
        configurable: true,
        value(this: VideoOutput, ...args: unknown[]) {
            return call(this.ops as VideoOps, this, ...args);
        },
    });
}

/** The names of the methods a video signal can be chained with. */
export const VIDEO_CHAIN_METHODS: readonly string[] = Object.keys(CHAIN);
