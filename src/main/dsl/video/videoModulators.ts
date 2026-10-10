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
    const channels = (
        fn: string,
        modulator: unknown,
        which: readonly ('r' | 'g' | 'b')[],
    ): VideoOutput[] => {
        if (!(modulator instanceof VideoOutput)) {
            throw new Error(
                `${fn}: modulator must be a video field or color, got ${describe(modulator)}`,
            );
        }
        return which.map((channel) => core.channel(modulator, channel));
    };

    return {
        /** Scales `input` about the center by `offset + multiple * channel`, red for x and green for y. */
        modulateScale: (
            input: VideoOutput,
            modulator: VideoOutput,
            multiple: VideoSource = 1,
            offset: VideoSource = 1,
        ): VideoOutput => {
            const [mr, mg] = channels('$v.modulateScale', modulator, [
                'r',
                'g',
            ]);
            return transform(core, '$v.modulateScale', 'modulateScale', input, {
                mg,
                mr,
                multiple,
                offset,
            });
        },

        /** Turns `input` about the center by `offset + multiple * red`, in turns. */
        modulateRotate: (
            input: VideoOutput,
            modulator: VideoOutput,
            multiple: VideoSource = 1,
            offset: VideoSource = 0,
        ): VideoOutput => {
            const [mr] = channels('$v.modulateRotate', modulator, ['r']);
            return transform(
                core,
                '$v.modulateRotate',
                'modulateRotate',
                input,
                { mr, multiple, offset },
            );
        },

        /** Holds `input` constant over `offset + multiple * channel` cells across and up. */
        modulatePixelate: (
            input: VideoOutput,
            modulator: VideoOutput,
            multiple: VideoSource = 10,
            offset: VideoSource = 3,
        ): VideoOutput => {
            const [mr, mg] = channels('$v.modulatePixelate', modulator, [
                'r',
                'g',
            ]);
            return transform(
                core,
                '$v.modulatePixelate',
                'modulatePixelate',
                input,
                { mg, mr, multiple, offset },
            );
        },

        /** Mirrors `input` into `sides` wedges, pushing outward by the red channel. */
        modulateKaleid: (
            input: VideoOutput,
            modulator: VideoOutput,
            sides: VideoSource = 4,
        ): VideoOutput => {
            const [mr] = channels('$v.modulateKaleid', modulator, ['r']);
            return transform(
                core,
                '$v.modulateKaleid',
                'modulateKaleid',
                input,
                { mr, sides },
            );
        },

        /** Pushes `input` by the differences between the modulator's channels, `amount` pixels at most. */
        modulateHue: (
            input: VideoOutput,
            modulator: VideoOutput,
            amount: VideoSource = 50,
        ): VideoOutput => {
            const [mr, mg, mb] = channels('$v.modulateHue', modulator, [
                'r',
                'g',
                'b',
            ]);
            return transform(core, '$v.modulateHue', 'modulateHue', input, {
                amount,
                mb,
                mg,
                mr,
            });
        },

        /** Tiles `input` with alternate rows and columns shifted by the red and green channels. */
        modulateRepeat: (
            input: VideoOutput,
            modulator: VideoOutput,
            repeatX: VideoSource = 3,
            repeatY: VideoSource = 3,
            offsetX: VideoSource = 2.5,
            offsetY: VideoSource = 2.5,
        ): VideoOutput => {
            const [mr, mg] = channels('$v.modulateRepeat', modulator, [
                'r',
                'g',
            ]);
            return transform(
                core,
                '$v.modulateRepeat',
                'modulateRepeat',
                input,
                { mg, mr, offsetX, offsetY, repeatX, repeatY },
            );
        },

        /** Tiles `input` across, shifting alternate columns up by the red channel. */
        modulateRepeatX: (
            input: VideoOutput,
            modulator: VideoOutput,
            reps: VideoSource = 3,
            offset: VideoSource = 2.5,
        ): VideoOutput => {
            const [mr] = channels('$v.modulateRepeatX', modulator, ['r']);
            return transform(
                core,
                '$v.modulateRepeatX',
                'modulateRepeatX',
                input,
                { mr, offset, reps },
            );
        },

        /** Tiles `input` up, shifting alternate rows right by the red channel. */
        modulateRepeatY: (
            input: VideoOutput,
            modulator: VideoOutput,
            reps: VideoSource = 3,
            offset: VideoSource = 2.5,
        ): VideoOutput => {
            const [mr] = channels('$v.modulateRepeatY', modulator, ['r']);
            return transform(
                core,
                '$v.modulateRepeatY',
                'modulateRepeatY',
                input,
                { mr, offset, reps },
            );
        },

        /** Scrolls `input` across by the red channel times `scroll`, and by `speed` per second, wrapping. */
        modulateScrollX: (
            input: VideoOutput,
            modulator: VideoOutput,
            scroll: VideoSource = 2.5,
            speed: VideoSource = 0,
        ): VideoOutput => {
            const [mr] = channels('$v.modulateScrollX', modulator, ['r']);
            return transform(
                core,
                '$v.modulateScrollX',
                'modulateScrollX',
                input,
                { mr, scroll, speed },
            );
        },

        /** Scrolls `input` up by the red channel times `scroll`, and by `speed` per second, wrapping. */
        modulateScrollY: (
            input: VideoOutput,
            modulator: VideoOutput,
            scroll: VideoSource = 2.5,
            speed: VideoSource = 0,
        ): VideoOutput => {
            const [mr] = channels('$v.modulateScrollY', modulator, ['r']);
            return transform(
                core,
                '$v.modulateScrollY',
                'modulateScrollY',
                input,
                { mr, scroll, speed },
            );
        },
    };
}
