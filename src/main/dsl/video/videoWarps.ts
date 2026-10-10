import { VideoOutput } from './VideoOutput';
import { transform } from './videoTransform';
import {
    describe,
    type VideoCore,
    type VideoRampConfig,
    type VideoSource,
} from './videoBuilderTypes';

/** The `$v` functions that move a whole sub-patch by changing the coordinates it is read at. */
export function warpMethods(core: VideoCore) {
    const methods = {
        /** Zooms, turns and shifts everything `input` draws. */
        warp: (input: VideoOutput, config?: VideoRampConfig): VideoOutput =>
            transform(core, '$v.warp', 'warp', input, {
                rotate: config?.rotate ?? 0,
                shiftX: config?.shiftX ?? 0,
                shiftY: config?.shiftY ?? 0,
                zoom: config?.zoom ?? 1,
            }),

        /** Reads `input` at positions pushed by `dx` and `dy`; 0.5 is no push. */
        displace: (
            input: VideoOutput,
            dx: VideoSource,
            dy: VideoSource = 0.5,
            amount: VideoSource = 0.1,
        ): VideoOutput =>
            transform(core, '$v.displace', 'displace', input, {
                amount,
                dx,
                dy,
            }),

        /**
         * Pushes `input` around by another signal: a color moves it by its red and
         * green channels, a field by its value in both directions.
         */
        modulate: (
            input: VideoOutput,
            modulator: VideoOutput,
            amount: VideoSource = 0.1,
        ): VideoOutput => {
            if (!(modulator instanceof VideoOutput)) {
                throw new Error(
                    `$v.modulate: modulator must be a video field or color, got ${describe(modulator)}`,
                );
            }
            if (modulator.type === 'field') {
                return methods.displace(input, modulator, modulator, amount);
            }
            return methods.displace(
                input,
                core.channel(modulator, 'r'),
                core.channel(modulator, 'g'),
                amount,
            );
        },

        /** Mirrors `input` around the center into `sides` wedges. */
        kaleid: (input: VideoOutput, sides: VideoSource = 4): VideoOutput =>
            transform(core, '$v.kaleid', 'kaleid', input, { sides }),

        /** Holds `input` constant across a grid of `x` by `y` cells. */
        pixelate: (
            input: VideoOutput,
            x: VideoSource = 20,
            y: VideoSource = x,
        ): VideoOutput =>
            transform(core, '$v.pixelate', 'pixelate', input, { x, y }),

        /** Tiles `input` `x` by `y` times across the frame. */
        repeat: (
            input: VideoOutput,
            x: VideoSource = 3,
            y: VideoSource = x,
        ): VideoOutput =>
            transform(core, '$v.repeat', 'repeat', input, { x, y }),
    };
    return methods;
}
