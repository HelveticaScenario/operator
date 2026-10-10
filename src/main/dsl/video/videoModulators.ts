import { VideoOutput } from './VideoOutput';
import { transform } from './videoTransform';
import {
    describe,
    type VideoCore,
    type VideoSource,
} from './videoBuilderTypes';

/**
 * The `$v` functions of Hydra's modulators: each reads the whole `input` at
 * coordinates that a modulator moves. A color modulates through its red,
 * green and blue channels as Hydra's do; a field supplies its one value for
 * every channel it is asked for.
 */
export function modulatorMethods(core: VideoCore) {
    /**
     * Reads the whole `input` at coordinates the `which` channels of
     * `modulator` move, as module `kind`; `fields` are its other inputs.
     */
    const modulate = (
        kind: string,
        which: readonly ('r' | 'g' | 'b')[],
        input: unknown,
        modulator: unknown,
        fields: Record<string, unknown>,
    ): VideoOutput => {
        const fn = `$v.${kind}`;
        if (!(modulator instanceof VideoOutput)) {
            throw new Error(
                `${fn}: modulator must be a video field or color, got ${describe(modulator)}`,
            );
        }
        const channels = Object.fromEntries(
            which.map((channel) => [
                `m${channel}`,
                core.channel(modulator, channel),
            ]),
        );
        return transform(core, fn, kind, input, { ...channels, ...fields });
    };

    return {
        /** Scales `input` about the center by `offset + multiple * channel`, red for x and green for y. */
        modulateScale: (
            input: VideoOutput,
            modulator: VideoOutput,
            multiple: VideoSource = 1,
            offset: VideoSource = 1,
        ): VideoOutput =>
            modulate('modulateScale', ['r', 'g'], input, modulator, {
                multiple,
                offset,
            }),

        /** Turns `input` about the center by `offset + multiple * red`; 5 is a full turn. */
        modulateRotate: (
            input: VideoOutput,
            modulator: VideoOutput,
            multiple: VideoSource = 5,
            offset: VideoSource = 0,
        ): VideoOutput =>
            modulate('modulateRotate', ['r'], input, modulator, {
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
            modulate('modulatePixelate', ['r', 'g'], input, modulator, {
                multiple,
                offset,
            }),

        /** Mirrors `input` into `sides` wedges, pushing outward by the red channel. */
        modulateKaleid: (
            input: VideoOutput,
            modulator: VideoOutput,
            sides: VideoSource = 4,
        ): VideoOutput =>
            modulate('modulateKaleid', ['r'], input, modulator, { sides }),

        /** Pushes `input` by the differences between the modulator's channels, `amount` pixels at most. */
        modulateHue: (
            input: VideoOutput,
            modulator: VideoOutput,
            amount: VideoSource = 50,
        ): VideoOutput =>
            modulate('modulateHue', ['r', 'g', 'b'], input, modulator, {
                amount,
            }),

        /** Tiles `input` with alternate rows and columns shifted by the red and green channels. */
        modulateRepeat: (
            input: VideoOutput,
            modulator: VideoOutput,
            repeatX: VideoSource = 3,
            repeatY: VideoSource = 3,
            offsetX: VideoSource = 2.5,
            offsetY: VideoSource = 2.5,
        ): VideoOutput =>
            modulate('modulateRepeat', ['r', 'g'], input, modulator, {
                repeatX,
                repeatY,
                offsetX,
                offsetY,
            }),

        /** Tiles `input` across, shifting alternate columns up by the red channel. */
        modulateRepeatX: (
            input: VideoOutput,
            modulator: VideoOutput,
            reps: VideoSource = 3,
            offset: VideoSource = 2.5,
        ): VideoOutput =>
            modulate('modulateRepeatX', ['r'], input, modulator, {
                reps,
                offset,
            }),

        /** Tiles `input` up, shifting alternate rows right by the red channel. */
        modulateRepeatY: (
            input: VideoOutput,
            modulator: VideoOutput,
            reps: VideoSource = 3,
            offset: VideoSource = 2.5,
        ): VideoOutput =>
            modulate('modulateRepeatY', ['r'], input, modulator, {
                reps,
                offset,
            }),

        /** Scrolls `input` across by the red channel times `scroll`, and by `speed` per second, wrapping. */
        modulateScrollX: (
            input: VideoOutput,
            modulator: VideoOutput,
            scroll: VideoSource = 2.5,
            speed: VideoSource = 0,
        ): VideoOutput =>
            modulate('modulateScrollX', ['r'], input, modulator, {
                scroll,
                speed,
            }),

        /** Scrolls `input` up by the red channel times `scroll`, and by `speed` per second, wrapping. */
        modulateScrollY: (
            input: VideoOutput,
            modulator: VideoOutput,
            scroll: VideoSource = 2.5,
            speed: VideoSource = 0,
        ): VideoOutput =>
            modulate('modulateScrollY', ['r'], input, modulator, {
                scroll,
                speed,
            }),
    };
}
