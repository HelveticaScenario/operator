import type { VideoOutput } from './VideoOutput';
import type { VideoCore, VideoSource } from './videoBuilderTypes';

/** The `$v` functions that combine, shape and composite signals. */
export function mathMethods(core: VideoCore) {
    /** A node of module `kind` whose inputs are `values`, named by their keys. */
    const node = (kind: string, values: Record<string, unknown>) =>
        core.addNode(kind, core.values(`$v.${kind}`, values));

    return {
        /** Sum, clipped to 0..1. */
        add: (a: VideoSource, b: VideoSource): VideoOutput =>
            node('add', { a, b }),

        /** Product. */
        mult: (a: VideoSource, b: VideoSource): VideoOutput =>
            node('mult', { a, b }),

        /** Absolute difference. */
        diff: (a: VideoSource, b: VideoSource): VideoOutput =>
            node('diff', { a, b }),

        /** The larger of each channel. */
        max: (a: VideoSource, b: VideoSource): VideoOutput =>
            node('max', { a, b }),

        /** The smaller of each channel. */
        min: (a: VideoSource, b: VideoSource): VideoOutput =>
            node('min', { a, b }),

        /** Complement: 1 minus each of red, green and blue. */
        invert: (input: VideoSource): VideoOutput => node('invert', { input }),

        /** Maps each channel's 0 to 1 onto `min` to `max`. */
        range: (
            input: VideoSource,
            low: VideoSource = 0,
            high: VideoSource = 1,
        ): VideoOutput => node('range', { input, lo: low, hi: high }),

        /** Crossfade from `a` (amount 0) to `b` (amount 1). */
        mix: (
            a: VideoSource,
            b: VideoSource,
            amount: VideoSource = 0.5,
        ): VideoOutput => node('mix', { a, b, amount }),

        /** Threshold per channel: 0 below `threshold`, 1 above, with a ramp `softness` wide. */
        comparator: (
            input: VideoSource,
            threshold: VideoSource = 0.5,
            softness: VideoSource = 0,
        ): VideoOutput => node('comparator', { input, threshold, softness }),

        /** Shows `fg` where `mask` is 1 and `bg` where it is 0. */
        key: (
            fg: VideoSource,
            bg: VideoSource,
            mask: VideoSource,
        ): VideoOutput => node('key', { fg, bg, mask }),

        /** Lays `top` over `base` by the alpha of `top`. */
        layer: (base: VideoSource, top: VideoSource): VideoOutput =>
            node('layer', { a: base, b: top }),

        /** `input` seen through `mask`: its color and alpha scaled by the mask's brightness. */
        mask: (input: VideoSource, mask: VideoSource): VideoOutput =>
            node('mask', { input, mask }),

        /** Keys `input` by its own brightness, giving it an alpha. */
        luma: (
            input: VideoSource,
            threshold: VideoSource = 0.5,
            tolerance: VideoSource = 0.1,
        ): VideoOutput => node('luma', { input, threshold, tolerance }),

        /** Quantizes each channel to `levels` values between 0 and 1. */
        posterize: (input: VideoSource, levels: VideoSource = 4): VideoOutput =>
            node('posterize', { input, levels }),

        /** Multiplies each channel by `gain`, then keeps the fractional part. */
        wrap: (input: VideoSource, gain: VideoSource = 1): VideoOutput =>
            node('wrap', { input, gain }),

        /** Multiplies each channel by `gain`, then reflects whatever passes 1 back down. */
        fold: (input: VideoSource, gain: VideoSource = 1): VideoOutput =>
            node('fold', { input, gain }),
    };
}
