import type { VideoOutput } from './VideoOutput';
import { type VideoSource, type VideoCore } from './videoBuilderTypes';

/** The `$v` functions that make or adjust colors. */
export function colorMethods(core: VideoCore) {
    /** A node of module `kind` that adjusts `input` by `values`. */
    const adjust = (
        kind: string,
        input: VideoSource,
        values: Record<string, unknown>,
    ): VideoOutput =>
        core.addNode(kind, core.values(`$v.${kind}`, { input, ...values }));

    return {
        /** A color from red, green, blue and alpha levels. */
        solid: (
            r: VideoSource = 0,
            g: VideoSource = 0,
            b: VideoSource = 0,
            a: VideoSource = 1,
        ): VideoOutput =>
            core.addNode('solid', core.values('$v.solid', { r, g, b, a })),

        /** Color from hue (wraps every 1.0), saturation and value. */
        hsv: (
            hue: VideoSource,
            saturation: VideoSource = 1,
            value: VideoSource = 1,
        ): VideoOutput =>
            core.addNode(
                'hsv',
                core.values('$v.hsv', { hue, saturation, value }),
            ),

        /** Turns every hue of `input` by `amount` of a full circle. */
        hue: (input: VideoOutput, amount: VideoSource = 0.4): VideoOutput =>
            adjust('hue', input, { amount }),

        /** Tints `input` by a multiplier per channel; a negative one inverts its channel. */
        color: (
            input: VideoOutput,
            r: VideoSource = 1,
            g: VideoSource = 1,
            b: VideoSource = 1,
            a: VideoSource = 1,
        ): VideoOutput => adjust('color', input, { r, g, b, a }),

        /** Adds the fractional part of each amount to its channel. */
        shift: (
            input: VideoOutput,
            r: VideoSource = 0.5,
            g: VideoSource = 0,
            b: VideoSource = 0,
            a: VideoSource = 0,
        ): VideoOutput => adjust('shift', input, { r, g, b, a }),

        /** Adds `amount` to red, green and blue. */
        brightness: (
            input: VideoOutput,
            amount: VideoSource = 0.4,
        ): VideoOutput => adjust('brightness', input, { amount }),

        /** Scales each channel's distance from the color's gray by `amount`. */
        saturate: (input: VideoOutput, amount: VideoSource = 2): VideoOutput =>
            adjust('saturate', input, { amount }),

        /** Shifts hue, saturation and value by `amount` and wraps each channel, cycling colors through bands. */
        colorama: (
            input: VideoOutput,
            amount: VideoSource = 0.005,
        ): VideoOutput => adjust('colorama', input, { amount }),

        /** Scales each channel's distance from mid-gray by `amount`. */
        contrast: (
            input: VideoOutput,
            amount: VideoSource = 1.6,
        ): VideoOutput => adjust('contrast', input, { amount }),

        /** Saturation, then gain and bias, clipped to the displayable range. */
        procAmp: (
            input: VideoSource,
            gain: VideoSource = 1,
            bias: VideoSource = 0,
            saturation: VideoSource = 1,
        ): VideoOutput => adjust('procAmp', input, { gain, bias, saturation }),

        /** Darkens evenly spaced horizontal lines, as a CRT does. */
        scanlines: (
            input: VideoOutput,
            count: VideoSource = 240,
            strength: VideoSource = 0.4,
        ): VideoOutput => adjust('scanlines', input, { count, strength }),

        /** Darkens toward the corners of the frame. */
        vignette: (
            input: VideoOutput,
            strength: VideoSource = 0.6,
            radius: VideoSource = 0.3,
        ): VideoOutput => adjust('vignette', input, { strength, radius }),

        /** Adds film grain, redrawn every frame. */
        grain: (input: VideoOutput, amount: VideoSource = 0.1): VideoOutput =>
            adjust('grain', input, { amount }),
    };
}
