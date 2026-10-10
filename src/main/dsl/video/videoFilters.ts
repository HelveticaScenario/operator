import { VideoOutput } from './VideoOutput';
import {
    describe,
    type VideoCore,
    type VideoSource,
} from './videoBuilderTypes';
import { transform } from './videoTransform';

/** The `$v` functions that average or compare a signal with its neighbors. */
export function filterMethods(core: VideoCore) {
    return {
        /**
         * Averages `input` over a disk of `radius`, a fraction of the frame
         * height. The input's whole sub-patch is read at several nearby
         * coordinates, so its cost is paid that many times.
         */
        blur: (input: VideoOutput, radius: VideoSource = 0.05): VideoOutput =>
            transform(core, '$v.blur', 'blur', input, { radius }),

        /**
         * Adds a blurred copy of `input` back on top of it, so bright areas glow.
         * It costs one blur.
         */
        bloom: (
            input: VideoOutput,
            radius: VideoSource = 0.2,
            amount: VideoSource = 5,
        ): VideoOutput => {
            const halo = transform(core, '$v.bloom', 'blur', input, { radius });
            const scaled = core.arith('$v.bloom', 'mult', {
                a: halo,
                b: amount,
            });
            return core.arith('$v.bloom', 'add', { a: input, b: scaled });
        },

        /** Brightness of the steepest change around each pixel, a Sobel filter. */
        edges: (input: VideoOutput, amount: VideoSource = 5): VideoOutput => {
            if (!(input instanceof VideoOutput)) {
                throw new Error(
                    `$v.edges: input must be a video field or color, got ${describe(input)}`,
                );
            }
            return core.addNode(
                input.type === 'color' ? 'edgesColor' : 'edges',
                'field',
                {
                    amount: core.asField('$v.edges', 'amount', amount),
                    input: input.value,
                },
            );
        },
    };
}
