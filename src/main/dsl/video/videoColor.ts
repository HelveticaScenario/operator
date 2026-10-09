import { VideoOutput } from './VideoOutput';
import {
    describe,
    type VideoSource,
    type VideoCore,
} from './videoBuilderTypes';

/** The `$v` functions that make or adjust colors. */
export function colorMethods(core: VideoCore) {
    return {
        /** Combines three fields into a color. */
        colorize: (
            r: VideoSource,
            g: VideoSource,
            b: VideoSource,
        ): VideoOutput =>
            core.addNode('colorize', 'color', {
                r: core.asField('$v.colorize', 'r', r),
                g: core.asField('$v.colorize', 'g', g),
                b: core.asField('$v.colorize', 'b', b),
            }),

        /** Color from hue (wraps every 1.0), saturation and value. */
        hsv: (
            h: VideoSource,
            s: VideoSource = 1,
            v: VideoSource = 1,
        ): VideoOutput =>
            core.addNode('hsv', 'color', {
                h: core.asField('$v.hsv', 'h', h),
                s: core.asField('$v.hsv', 's', s),
                v: core.asField('$v.hsv', 'v', v),
            }),

        /** Turns every hue of `input` by `amount` of a full circle. */
        hueShift: (
            input: VideoOutput,
            amount: VideoSource = 0.5,
        ): VideoOutput => {
            if (!(input instanceof VideoOutput) || input.type !== 'color') {
                throw new Error(
                    `$v.hueShift: input must be a video color, got ${describe(input)}`,
                );
            }
            return core.addNode('hueShift', 'color', {
                amount: core.asField('$v.hueShift', 'amount', amount),
                input: input.value,
            });
        },

        /** Scales each channel's distance from mid-gray by `amount`. */
        contrast: (
            input: VideoOutput,
            amount: VideoSource = 1.6,
        ): VideoOutput => {
            if (!(input instanceof VideoOutput) || input.type !== 'color') {
                throw new Error(
                    `$v.contrast: input must be a video color, got ${describe(input)}`,
                );
            }
            return core.addNode('contrast', 'color', {
                amount: core.asField('$v.contrast', 'amount', amount),
                input: input.value,
            });
        },

        /** Saturation, then gain and bias, clipped to the displayable range. */
        procAmp: (
            input: VideoSource,
            gain: VideoSource = 1,
            bias: VideoSource = 0,
            saturation: VideoSource = 1,
        ): VideoOutput =>
            core.addNode('procAmp', 'color', {
                input: core.asColorOrGray('$v.procAmp', 'input', input),
                gain: core.asField('$v.procAmp', 'gain', gain),
                bias: core.asField('$v.procAmp', 'bias', bias),
                saturation: core.asField(
                    '$v.procAmp',
                    'saturation',
                    saturation,
                ),
            }),
    };
}
