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
        colorize: (
            r: VideoSource,
            g: VideoSource,
            b: VideoSource,
            a: VideoSource = 1,
        ): VideoOutput =>
            core.addNode(
                'colorize',
                core.values('$v.colorize', { r, g, b, a }),
            ),

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
        hueShift: (
            input: VideoOutput,
            amount: VideoSource = 0.5,
        ): VideoOutput => adjust('hueShift', input, { amount }),

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
