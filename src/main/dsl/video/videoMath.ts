import type { VideoOutput } from './VideoOutput';
import {
    describe,
    type VideoCore,
    type VideoKeepConfig,
    type VideoLumaConfig,
    type VideoMaskConfig,
    type VideoSource,
} from './videoBuilderTypes';

/** The `$v` functions that combine, shape and composite signals. */
export function mathMethods(core: VideoCore) {
    /** A node of module `kind` whose inputs are `values`, named by their keys. */
    const node = (kind: string, values: Record<string, unknown>) =>
        core.addNode(kind, core.values(`$v.${kind}`, values));

    /** The boolean option `name` of a `$v.<kind>` config, false when absent. */
    const flag = (kind: string, name: string, option: unknown): boolean => {
        const value = option ?? false;
        if (typeof value !== 'boolean') {
            throw new Error(
                `$v.${kind}: ${name} must be true or false, got ${describe(value)}`,
            );
        }
        return value;
    };

    /** A node of module `kind` that keeps the side of its threshold `config` asks for. */
    const keeping = (
        kind: string,
        values: Record<string, unknown>,
        config: VideoKeepConfig | undefined,
    ) =>
        core.addNode(
            kind,
            core.values(`$v.${kind}`, values),
            flag(kind, 'invert', config?.invert)
                ? { params: { keep: 'below' } }
                : undefined,
        );

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
            min: VideoSource = 0,
            max: VideoSource = 1,
        ): VideoOutput => node('range', { input, lo: min, hi: max }),

        /** Crossfade from `a` (amount 0) to `b` (amount 1). */
        mix: (
            a: VideoSource,
            b: VideoSource,
            amount: VideoSource = 0.5,
        ): VideoOutput => node('mix', { a, b, amount }),

        /** Threshold per channel: 0 below `threshold`, 1 above, with a ramp `softness` wide; `config.invert` swaps the sides. */
        comparator: (
            input: VideoSource,
            threshold: VideoSource = 0.5,
            softness: VideoSource = 0,
            config?: VideoKeepConfig,
        ): VideoOutput =>
            keeping('comparator', { input, threshold, softness }, config),

        /** Shows `fg` where `mask` is 1 and `bg` where it is 0. */
        key: (
            fg: VideoSource,
            bg: VideoSource,
            mask: VideoSource,
        ): VideoOutput => node('key', { fg, bg, mask }),

        /** Lays `top` over `base` by the alpha of `top`. */
        layer: (base: VideoSource, top: VideoSource): VideoOutput =>
            node('layer', { a: base, b: top }),

        /** `input` seen through `mask`: its color and alpha scaled by the mask's brightness, or with `config.keepColor` only its alpha. */
        mask: (
            input: VideoSource,
            mask: VideoSource,
            config?: VideoMaskConfig,
        ): VideoOutput =>
            core.addNode(
                'mask',
                core.values('$v.mask', { input, mask }),
                flag('mask', 'keepColor', config?.keepColor)
                    ? { params: { color: 'keep' } }
                    : undefined,
            ),

        /** Keys `input` by its own brightness, color and alpha alike (only alpha with `config.keepColor`): opaque from `threshold` up, or with `config.invert` from `threshold` down. */
        luma: (
            input: VideoSource,
            threshold: VideoSource = 0.5,
            tolerance: VideoSource = 0.1,
            config?: VideoLumaConfig,
        ): VideoOutput => {
            const params: Record<string, string> = {};
            if (flag('luma', 'invert', config?.invert)) params.keep = 'below';
            if (flag('luma', 'keepColor', config?.keepColor))
                params.color = 'keep';
            return core.addNode(
                'luma',
                core.values('$v.luma', { input, threshold, tolerance }),
                Object.keys(params).length > 0 ? { params } : undefined,
            );
        },

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
