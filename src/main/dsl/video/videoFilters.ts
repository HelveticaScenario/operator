import type { VideoOutput } from './VideoOutput';
import type { VideoCore, VideoSource } from './videoBuilderTypes';
import { transform } from './videoTransform';

/** The `$v` functions that average or compare a signal with its neighbors. */
export function filterMethods(core: VideoCore) {
    return {
        /**
         * Averages `input` over a disk of `radius`, a fraction of the frame
         * height. The input's whole sub-patch is read at several nearby
         * coordinates, so its cost is paid that many times.
         */
        blur: (input: VideoOutput, radius: VideoSource = 0.01): VideoOutput =>
            transform(core, '$v.blur', 'blur', input, { radius }),

        /**
         * Adds a blurred copy of `input` back on top of it, so bright areas glow.
         * It costs one blur.
         */
        bloom: (
            input: VideoOutput,
            radius: VideoSource = 0.04,
            amount: VideoSource = 1,
        ): VideoOutput => {
            const halo = transform(core, '$v.bloom', 'blur', input, { radius });
            const scaled = core.addNode(
                'mult',
                core.values('$v.bloom', { a: halo, b: amount }),
            );
            return core.addNode(
                'add',
                core.values('$v.bloom', { a: input, b: scaled }),
            );
        },

        /** Brightness of the steepest change around each pixel, a Sobel filter. */
        edges: (input: VideoOutput, amount: VideoSource = 1): VideoOutput =>
            transform(core, '$v.edges', 'edges', input, { amount }),
    };
}
