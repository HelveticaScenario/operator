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
                max: core.asField('$v.range', 'max', high),
                min: core.asField('$v.range', 'min', low),
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
            core.addNode('comparator', 'field', {
                input: core.asField('$v.comparator', 'input', input),
                threshold: core.asField(
                    '$v.comparator',
                    'threshold',
                    threshold,
                ),
                softness: core.asField('$v.comparator', 'softness', softness),
            }),

        /** Shows `fg` where `mask` is 1 and `bg` where it is 0. */
        key: (
            fg: VideoSource,
            bg: VideoSource,
            mask: VideoSource,
        ): VideoOutput =>
            core.addNode('key', 'color', {
                fg: core.asColorOrGray('$v.key', 'fg', fg),
                bg: core.asColorOrGray('$v.key', 'bg', bg),
                mask: core.asField('$v.key', 'mask', mask),
            }),

        /** Quantizes to `levels` values between 0 and 5. */
        posterize: (input: VideoSource, levels: VideoSource = 4): VideoOutput =>
            core.addNode('posterize', 'field', {
                input: core.asField('$v.posterize', 'input', input),
                levels: core.asField('$v.posterize', 'levels', levels),
            }),

        /** Multiplies by `gain`, then keeps the fractional part. */
        wrap: (input: VideoSource, gain: VideoSource = 1): VideoOutput =>
            core.addNode('wrap', 'field', {
                input: core.asField('$v.wrap', 'input', input),
                gain: core.asField('$v.wrap', 'gain', gain),
            }),

        /** Multiplies by `gain`, then reflects whatever passes 1 back down. */
        fold: (input: VideoSource, gain: VideoSource = 1): VideoOutput =>
            core.addNode('fold', 'field', {
                input: core.asField('$v.fold', 'input', input),
                gain: core.asField('$v.fold', 'gain', gain),
            }),
    };
}
