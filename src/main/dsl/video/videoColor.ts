import { VideoOutput } from './VideoOutput';
import { type VideoSource, type VideoCore } from './videoBuilderTypes';

/** The `$v` functions that make or adjust colors. */
export function colorMethods(core: VideoCore) {
    /** A node whose `input` is a color (a field becomes gray) and whose other inputs are fields. */
    const adjust = (
        kind: string,
        input: VideoSource,
        fields: Record<string, unknown>,
    ): VideoOutput =>
        core.addNode(kind, 'color', {
            input: core.toColor(`$v.${kind}`, 'input', input).value,
            ...core.fields(`$v.${kind}`, fields),
        });

    return {
        /** Combines three fields into a color. */
        colorize: (
            r: VideoSource,
            g: VideoSource,
            b: VideoSource,
        ): VideoOutput =>
            core.addNode(
                'colorize',
                'color',
                core.fields('$v.colorize', { r, g, b }),
            ),

        /** Color from hue (wraps every 1.0), saturation and value. */
        hsv: (
            h: VideoSource,
            s: VideoSource = 5,
            v: VideoSource = 5,
        ): VideoOutput =>
            core.addNode('hsv', 'color', core.fields('$v.hsv', { h, s, v })),

        /** Turns every hue of `input` by `amount` of a full circle. */
        hueShift: (
            input: VideoOutput,
            amount: VideoSource = 2.5,
        ): VideoOutput => adjust('hueShift', input, { amount }),

        /** Scales each channel's distance from mid-gray by `amount`. */
        contrast: (input: VideoOutput, amount: VideoSource = 8): VideoOutput =>
            adjust('contrast', input, { amount }),

        /** Saturation, then gain and bias, clipped to the displayable range. */
        procAmp: (
            input: VideoSource,
            gain: VideoSource = 5,
            bias: VideoSource = 0,
            saturation: VideoSource = 5,
        ): VideoOutput => adjust('procAmp', input, { gain, bias, saturation }),

        /** Darkens evenly spaced horizontal lines, as a CRT does. */
        scanlines: (
            input: VideoOutput,
            count: VideoSource = 240,
            strength: VideoSource = 2,
        ): VideoOutput => adjust('scanlines', input, { count, strength }),

        /** Darkens toward the corners of the frame. */
        vignette: (
            input: VideoOutput,
            strength: VideoSource = 3,
            radius: VideoSource = 1.5,
        ): VideoOutput => adjust('vignette', input, { strength, radius }),

        /** Adds film grain, redrawn every frame. */
        grain: (input: VideoOutput, amount: VideoSource = 0.5): VideoOutput =>
            adjust('grain', input, { amount }),
    };
}
