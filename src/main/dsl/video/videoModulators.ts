import { VideoOutput } from './VideoOutput';
import { transform } from './videoTransform';
import {
    describe,
    type VideoCore,
    type VideoSource,
} from './videoBuilderTypes';

/**
 * The `$v` modulator functions: each reads the whole `input` at coordinates
 * that a modulator moves through its red, green and blue channels.
 */
export function modulatorMethods(core: VideoCore) {
    /**
     * Reads the whole `input` at coordinates `modulator` moves, as module
     * `kind`; `values` are its other inputs.
     */
    const modulate = (
        kind: string,
        input: unknown,
        modulator: unknown,
        values: Record<string, unknown>,
    ): VideoOutput => {
        const fn = `$v.${kind}`;
        if (!(modulator instanceof VideoOutput)) {
            throw new Error(
                `${fn}: modulator must be a video signal, got ${describe(modulator)}`,
            );
        }
        return transform(core, fn, kind, input, { m: modulator, ...values });
    };

    return {
        /** Pushes `input` across by the modulator's red channel and up by its green, `amount` of the frame per unit. */
        modulate: (
            input: VideoOutput,
            modulator: VideoOutput,
            amount: VideoSource = 0.1,
        ): VideoOutput => modulate('modulate', input, modulator, { amount }),

        /** Scales `input` about the center by `offset + multiple * channel`, red for x and green for y. */
        modulateScale: (
            input: VideoOutput,
            modulator: VideoOutput,
            multiple: VideoSource = 1,
            offset: VideoSource = 1,
        ): VideoOutput =>
            modulate('modulateScale', input, modulator, {
                multiple,
                offset,
            }),

        /** Turns `input` about the center by `offset + multiple * red`, in turns. */
        modulateRotate: (
            input: VideoOutput,
            modulator: VideoOutput,
            multiple: VideoSource = 1,
            offset: VideoSource = 0,
        ): VideoOutput =>
            modulate('modulateRotate', input, modulator, {
                multiple,
                offset,
            }),

        /** Holds `input` constant over `offset + multiple * channel` cells across and up. */
        modulatePixelate: (
            input: VideoOutput,
            modulator: VideoOutput,
            multiple: VideoSource = 10,
            offset: VideoSource = 3,
        ): VideoOutput =>
            modulate('modulatePixelate', input, modulator, {
                multiple,
                offset,
            }),

        /** Mirrors `input` into `sides` wedges, pushing outward by the red channel. */
        modulateKaleid: (
            input: VideoOutput,
            modulator: VideoOutput,
            sides: VideoSource = 4,
        ): VideoOutput =>
            modulate('modulateKaleid', input, modulator, { sides }),

        /** Pushes `input` by the differences between the modulator's channels, green minus red across and blue minus green up, `amount` pixels for a difference of full scale. */
        modulateHue: (
            input: VideoOutput,
            modulator: VideoOutput,
            amount: VideoSource = 1,
        ): VideoOutput =>
            modulate('modulateHue', input, modulator, {
                amount,
            }),

        /** Tiles `input`, sliding every tile's picture by the red and green channels. */
        modulateRepeat: (
            input: VideoOutput,
            modulator: VideoOutput,
            repeatX: VideoSource = 3,
            repeatY: VideoSource = 3,
            offsetX: VideoSource = 0.5,
            offsetY: VideoSource = 0.5,
        ): VideoOutput =>
            modulate('modulateRepeat', input, modulator, {
                repeatX,
                repeatY,
                offsetX,
                offsetY,
            }),

        /** Tiles `input` across, sliding every tile's picture up by the red channel. */
        modulateRepeatX: (
            input: VideoOutput,
            modulator: VideoOutput,
            reps: VideoSource = 3,
            offset: VideoSource = 0.5,
        ): VideoOutput =>
            modulate('modulateRepeatX', input, modulator, {
                reps,
                offset,
            }),

        /** Tiles `input` up, sliding every tile's picture across by the red channel. */
        modulateRepeatY: (
            input: VideoOutput,
            modulator: VideoOutput,
            reps: VideoSource = 3,
            offset: VideoSource = 0.5,
        ): VideoOutput =>
            modulate('modulateRepeatY', input, modulator, {
                reps,
                offset,
            }),

        /** Scrolls `input` across by the red channel times `scroll`, and by `speed` per second, wrapping. */
        modulateScrollX: (
            input: VideoOutput,
            modulator: VideoOutput,
            scroll: VideoSource = 0.5,
            speed: VideoSource = 0,
        ): VideoOutput =>
            modulate('modulateScrollX', input, modulator, {
                scroll,
                speed,
            }),

        /** Scrolls `input` up by the red channel times `scroll`, and by `speed` per second, wrapping. */
        modulateScrollY: (
            input: VideoOutput,
            modulator: VideoOutput,
            scroll: VideoSource = 0.5,
            speed: VideoSource = 0,
        ): VideoOutput =>
            modulate('modulateScrollY', input, modulator, {
                scroll,
                speed,
            }),
    };
}
