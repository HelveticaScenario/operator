import { VideoOutput } from './VideoOutput';
import {
    describe,
    type VideoCore,
    type VideoSource,
} from './videoBuilderTypes';

/** The `$v` functions that combine and shape signals. */
export function mathMethods(core: VideoCore) {
    return {
        /** Sum, clipped to 0..5. Colors add per channel. */
        add: (a: VideoSource, b: VideoSource): VideoOutput =>
            core.arith('$v.add', 'add', { a, b }),

        /** Product. Colors multiply per channel. */
        mult: (a: VideoSource, b: VideoSource): VideoOutput =>
            core.arith('$v.mult', 'mult', { a, b }),

        /** Absolute difference. Colors differ per channel. */
        diff: (a: VideoSource, b: VideoSource): VideoOutput =>
            core.arith('$v.diff', 'diff', { a, b }),

        /** The larger of two values; colors take it per channel. */
        max: (a: VideoSource, b: VideoSource): VideoOutput =>
            core.arith('$v.max', 'max', { a, b }),

        /** The smaller of two values; colors take it per channel. */
        min: (a: VideoSource, b: VideoSource): VideoOutput =>
            core.arith('$v.min', 'min', { a, b }),

        /** Complement: 1 - input. */
        invert: (input: VideoSource): VideoOutput =>
            core.arith('$v.invert', 'invert', { input }),

        /** Maps a field's 0 to 5 volts onto `min` to `max` volts. */
        range: (
            input: VideoSource,
            low: VideoSource = 0,
            high: VideoSource = 5,
        ): VideoOutput => {
            if (!(input instanceof VideoOutput) || input.type !== 'field') {
                throw new Error(
                    `$v.range: input must be a video field, got ${describe(input)}`,
                );
            }
            return core.addNode('range', 'field', {
                input: input.value,
                ...core.fields('$v.range', { max: high, min: low }),
            });
        },

        /** Crossfade from `a` (amount 0) to `b` (amount 1). */
        mix: (
            a: VideoSource,
            b: VideoSource,
            amount: VideoSource = 2.5,
        ): VideoOutput => core.arith('$v.mix', 'mix', { a, b }, { amount }),

        /** Threshold: 0 below `threshold`, 1 above, with a ramp `softness` wide. */
        comparator: (
            input: VideoSource,
            threshold: VideoSource = 2.5,
            softness: VideoSource = 0,
        ): VideoOutput =>
            core.addNode(
                'comparator',
                'field',
                core.fields('$v.comparator', { input, threshold, softness }),
            ),

        /** Shows `fg` where `mask` is 1 and `bg` where it is 0. */
        key: (
            fg: VideoSource,
            bg: VideoSource,
            mask: VideoSource,
        ): VideoOutput =>
            core.addNode('key', 'color', {
                fg: core.toColor('$v.key', 'fg', fg).value,
                bg: core.toColor('$v.key', 'bg', bg).value,
                mask: core.asField('$v.key', 'mask', mask),
            }),

        /** Quantizes to `levels` values between 0 and 5. */
        posterize: (input: VideoSource, levels: VideoSource = 4): VideoOutput =>
            core.addNode(
                'posterize',
                'field',
                core.fields('$v.posterize', { input, levels }),
            ),

        /** Multiplies by `gain`, then keeps the fractional part. */
        wrap: (input: VideoSource, gain: VideoSource = 1): VideoOutput =>
            core.addNode(
                'wrap',
                'field',
                core.fields('$v.wrap', { input, gain }),
            ),

        /** Multiplies by `gain`, then reflects whatever passes 1 back down. */
        fold: (input: VideoSource, gain: VideoSource = 1): VideoOutput =>
            core.addNode(
                'fold',
                'field',
                core.fields('$v.fold', { input, gain }),
            ),
    };
}
