import { MAX_FEEDBACK_BUFFERS } from '../../../shared/video/videoGraph';
import { VideoBuffer } from './VideoBuffer';
import { VideoOutput } from './VideoOutput';
import {
    describe,
    type VideoCore,
    type VideoFeedbackConfig,
} from './videoBuilderTypes';

/** The most frames `$v.frameDelay` can hold back: it costs a buffer per frame. */
const MAX_DELAY = MAX_FEEDBACK_BUFFERS;

/**
 * The `$v` functions that keep frames from one moment to the next: feedback
 * loops, frame buffers and frame delays. They share the patch's frame
 * buffers, of which there are {@link MAX_FEEDBACK_BUFFERS}.
 */
export function memoryMethods(core: VideoCore) {
    let bufferCount = 0;
    const written = new Set<number>();

    const allocate = (fn: string): number => {
        if (bufferCount >= MAX_FEEDBACK_BUFFERS) {
            throw new Error(
                `${fn}: a patch can use at most ${MAX_FEEDBACK_BUFFERS} feedback loops, counting frame buffers and frame delays`,
            );
        }
        return bufferCount++;
    };

    /** The previous frame of buffer `index`, resampled through the transform in `config`. */
    const read = (
        index: number,
        config?: VideoFeedbackConfig,
        fn = '$v.buffer',
    ): VideoOutput =>
        core.addNode(
            'feedbackRead',
            'color',
            core.fields(fn, {
                zoom: config?.zoom ?? 1,
                rotate: config?.rotate ?? 0,
                shiftX: config?.shiftX ?? 0,
                shiftY: config?.shiftY ?? 0,
            }),
            {
                buffer: index,
                ...(config?.edge !== undefined && {
                    params: { edge: config.edge },
                }),
            },
        );

    /** Stores `input` in buffer `index` for the next frame, and returns it. */
    const write = (
        index: number,
        input: VideoOutput,
        fn = '$v.buffer',
    ): VideoOutput => {
        if (!(input instanceof VideoOutput)) {
            throw new Error(
                `${fn}: write takes a video field or color, got ${describe(input)}`,
            );
        }
        const color = core.toColor(fn, 'input', input);
        if (written.has(index)) {
            throw new Error(`${fn}: a buffer can be written only once`);
        }
        written.add(index);
        core.addNode(
            'feedbackWrite',
            'color',
            { input: color.value },
            { buffer: index },
        );
        return color;
    };

    return {
        /**
         * Feeds a frame back into itself. `update` receives the previous frame's
         * result, resampled through the transform in `config`, and returns this
         * frame's color; `feedback` returns that color.
         */
        feedback: (
            update: (prev: VideoOutput) => VideoOutput,
            config?: VideoFeedbackConfig,
        ): VideoOutput => {
            if (typeof update !== 'function') {
                throw new Error(
                    `$v.feedback: update must be a function, got ${describe(update)}`,
                );
            }
            const index = allocate('$v.feedback');
            const next = update(read(index, config, '$v.feedback'));
            if (!(next instanceof VideoOutput)) {
                throw new Error(
                    `$v.feedback: update must return a video field or color, got ${describe(next)}`,
                );
            }
            return write(index, next, '$v.feedback');
        },

        /**
         * A frame store: signals write it, and any signal can read what it held on
         * the previous frame, so buffers can feed themselves or each other.
         */
        buffer: (): VideoBuffer =>
            new VideoBuffer(allocate('$v.buffer'), {
                readBuffer: (index, config) => read(index, config),
                writeBuffer: (index, input) => write(index, input),
            }),

        /**
         * `input` as it was `frames` frames ago, as a color; before then it is
         * black. Each frame held back costs one frame buffer.
         */
        frameDelay: (input: VideoOutput, frames = 1): VideoOutput => {
            if (!Number.isInteger(frames) || frames < 1 || frames > MAX_DELAY) {
                throw new Error(
                    `$v.frameDelay: frames must be a whole number from 1 to ${MAX_DELAY}, got ${describe(frames)}`,
                );
            }
            let delayed = core.toColor('$v.frameDelay', 'input', input);
            for (let held = 0; held < frames; held++) {
                const index = allocate('$v.frameDelay');
                write(index, delayed, '$v.frameDelay');
                delayed = read(index, undefined, '$v.frameDelay');
            }
            return delayed;
        },
    };
}
