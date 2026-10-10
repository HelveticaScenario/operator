import type {
    VideoValue,
    VideoValueType,
} from '../../../shared/video/videoGraph';

/** The graph builder's `$v` functions, which chain methods forward to. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type VideoOps = Record<string, (...args: any[]) => any>;

type ChainCall = (
    ops: VideoOps,
    self: VideoOutput,
    ...args: unknown[]
) => unknown;

/** Chainable methods of `.$` and `.$m`, each with the signal as its first argument. */
type ChainProxy = Record<string, (...args: unknown[]) => unknown>;

/**
 * A video signal: a node's output, a constant, or the time field.
 *
 * It chains the way an audio signal does. `.$` is a namespace of the `$v`
 * functions that take a signal first, with the signal supplied for you, so
 * `x.$.rotate(0.1)` is `$v.warp(x, { rotate: 0.1 })`. `.$m` is the same with a
 * leading `mix` argument that crossfades the signal against the result (0 is
 * the signal, 1 the result). `.pipe` and `.pipeMix` apply a function of your
 * own. `out`, `preview`, `toCV` and `write` end a chain and are direct methods.
 */
export class VideoOutput {
    constructor(
        readonly value: VideoValue,
        readonly type: VideoValueType,
        /** The builder that made this signal; chain methods call its functions. */
        readonly ops: object,
    ) {}

    /** The red channel as a field; a field is gray, so each channel is itself. */
    get r(): unknown {
        return (this.ops as VideoOps).channel(this, 'r');
    }

    /** The green channel as a field. */
    get g(): unknown {
        return (this.ops as VideoOps).channel(this, 'g');
    }

    /** The blue channel as a field. */
    get b(): unknown {
        return (this.ops as VideoOps).channel(this, 'b');
    }

    /** The `$v` functions that take a signal first, applied to this signal. */
    get $(): ChainProxy {
        return this.chain(false);
    }

    /** Like {@link $}, with a leading `mix` that crossfades this signal against the result. */
    get $m(): ChainProxy {
        return this.chain(true);
    }

    /**
     * Calls `fn` with this signal. With an array, calls it once per element,
     * as `fn(this, element)`, and returns the results as an array.
     */
    pipe(fn: unknown, array?: unknown[]): unknown {
        if (typeof fn !== 'function') {
            throw new Error('pipe: expects a function');
        }
        if (array === undefined) return fn(this);
        if (!Array.isArray(array)) {
            throw new Error('pipe: the second argument must be an array');
        }
        return array.map((item) => fn(this, item));
    }

    /** Crossfades this signal against `fn(this)`: 0 is this signal, 1 the result. */
    pipeMix(fn: unknown, mix: unknown = 0.5): unknown {
        if (typeof fn !== 'function') {
            throw new Error('pipeMix: expects a function');
        }
        return (this.ops as VideoOps).mix(this, fn(this), mix);
    }

    private chain(withMix: boolean): ChainProxy {
        const ops = this.ops as VideoOps;
        return new Proxy({} as ChainProxy, {
            get: (_target, prop) => {
                const call =
                    typeof prop === 'string' ? PROCESSING[prop] : undefined;
                if (call === undefined) return undefined;
                if (!withMix) {
                    return (...args: unknown[]) => call(ops, this, ...args);
                }
                return (mix: unknown, ...args: unknown[]) =>
                    ops.mix(this, call(ops, this, ...args), mix);
            },
        });
    }
}

/** Functions of `.$` and `.$m`: each returns a new signal from this one. */
const PROCESSING: Record<string, ChainCall> = {
    add: (o, s, b) => o.add(s, b),
    bloom: (o, s, radius, amount) => o.bloom(s, radius, amount),
    blur: (o, s, radius) => o.blur(s, radius),
    channel: (o, s, which) => o.channel(s, which),
    comparator: (o, s, threshold, softness) =>
        o.comparator(s, threshold, softness),
    contrast: (o, s, amount) => o.contrast(s, amount),
    diff: (o, s, b) => o.diff(s, b),
    displace: (o, s, dx, dy, amount) => o.displace(s, dx, dy, amount),
    edges: (o, s, amount) => o.edges(s, amount),
    fold: (o, s, gain) => o.fold(s, gain),
    grain: (o, s, amount) => o.grain(s, amount),
    hsv: (o, s, saturation, value) => o.hsv(s, saturation, value),
    hueShift: (o, s, amount) => o.hueShift(s, amount),
    invert: (o, s) => o.invert(s),
    kaleid: (o, s, sides) => o.kaleid(s, sides),
    key: (o, s, background, mask) => o.key(s, background, mask),
    max: (o, s, b) => o.max(s, b),
    min: (o, s, b) => o.min(s, b),
    mix: (o, s, b, amount) => o.mix(s, b, amount),
    modulate: (o, s, modulator, amount) => o.modulate(s, modulator, amount),
    mult: (o, s, b) => o.mult(s, b),
    pixelate: (o, s, x, y) => o.pixelate(s, x, y),
    posterize: (o, s, levels) => o.posterize(s, levels),
    procAmp: (o, s, gain, bias, saturation) =>
        o.procAmp(s, gain, bias, saturation),
    repeat: (o, s, x, y) => o.repeat(s, x, y),
    rotate: (o, s, turns) => o.warp(s, { rotate: turns }),
    scale: (o, s, zoom) => o.warp(s, { zoom }),
    scanlines: (o, s, count, strength) => o.scanlines(s, count, strength),
    swiz: (o, s, pattern) => o.swiz(s, pattern),
    scroll: (o, s, x = 0, y = 0) => o.warp(s, { shiftX: x, shiftY: y }),
    tint: (o, s, hue, saturation) => o.hsv(hue ?? 0, saturation ?? 1, s),
    vignette: (o, s, strength, radius) => o.vignette(s, strength, radius),
    warp: (o, s, config) => o.warp(s, config),
    wrap: (o, s, gain) => o.wrap(s, gain),
};

/** Direct methods that end a chain or tap it: they do not make a new signal to chain on. */
const DIRECT: Record<string, ChainCall> = {
    out: (o, s) => o.out(s),
    preview: (o, s, config) => o.preview(s, config),
    toCV: (o, s, config) => o.toCV(s, config),
    write: (_o, s, buffer) =>
        (buffer as { write(color: VideoOutput): unknown }).write(s),
};

for (const [name, call] of Object.entries(DIRECT)) {
    Object.defineProperty(VideoOutput.prototype, name, {
        configurable: true,
        value(this: VideoOutput, ...args: unknown[]) {
            return call(this.ops as VideoOps, this, ...args);
        },
    });
}

/** The names of the functions `.$` and `.$m` offer. */
export const VIDEO_CHAIN_METHODS: readonly string[] = Object.keys(PROCESSING);

/** The names of the direct methods that end or tap a chain. */
export const VIDEO_DIRECT_METHODS: readonly string[] = Object.keys(DIRECT);
