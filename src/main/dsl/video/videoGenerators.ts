import { VideoOutput } from './VideoOutput';
import type {
    VideoCore,
    VideoOscConfig,
    VideoRampConfig,
    VideoShapeConfig,
    VideoSource,
} from './videoBuilderTypes';

/** The `$v` functions that make a field from coordinates. */
export function generatorMethods(core: VideoCore) {
    return {
        /**
         * Scan ramp over the frame, optionally zoomed, rotated and shifted. `h` and
         * `v` run 0 to 5 volts across and up, `d` along the diagonal, `r` is the
         * distance from the center and `a` the angle around it.
         */
        ramp: (
            axis: 'h' | 'v' | 'd' | 'r' | 'a' = 'h',
            config?: VideoRampConfig,
        ): VideoOutput =>
            core.addNode(
                'ramp',
                'field',
                core.fields('$v.ramp', {
                    zoom: config?.zoom ?? 1,
                    rotate: config?.rotate ?? 0,
                    shiftX: config?.shiftX ?? 0,
                    shiftY: config?.shiftY ?? 0,
                }),
                { params: { axis } },
            ),

        /** Periodic shaper: `freq` cycles per unit of `input`, offset by `phase` cycles. */
        osc: (
            input: VideoSource,
            freq: VideoSource,
            phase: VideoSource = 0,
            config?: VideoOscConfig,
        ): VideoOutput =>
            core.addNode(
                'osc',
                'field',
                core.fields('$v.osc', { input, freq, phase }),
                config?.shape === undefined
                    ? undefined
                    : { params: { shape: config.shape } },
            ),

        /** 1 inside a shape centered on (x, y), 0 outside. */
        shape: (
            x: VideoSource,
            y: VideoSource,
            size: VideoSource = 1.25,
            softness: VideoSource = 0.05,
            config?: VideoShapeConfig,
        ): VideoOutput =>
            core.addNode(
                'shape',
                'field',
                core.fields('$v.shape', { x, y, size, softness }),
                config?.shape === undefined
                    ? undefined
                    : { params: { shape: config.shape } },
            ),

        /** 1 inside a regular polygon centered on (x, y), 0 outside. */
        polygon: (
            x: VideoSource,
            y: VideoSource,
            sides: VideoSource = 3,
            size: VideoSource = 1.25,
            softness: VideoSource = 0.05,
        ): VideoOutput =>
            core.addNode(
                'polygon',
                'field',
                core.fields('$v.polygon', { x, y, sides, size, softness }),
            ),

        /** Smooth value noise between 0 and 5; `z` moves through it. */
        noise: (
            x: VideoSource,
            y: VideoSource,
            z: VideoSource = 0,
        ): VideoOutput =>
            core.addNode(
                'noise',
                'field',
                core.fields('$v.noise', { x, y, z }),
            ),

        /** Cellular noise: the distance to the nearest of a scatter of points; `z` moves them. */
        voronoi: (
            x: VideoSource,
            y: VideoSource,
            z: VideoSource = 0,
        ): VideoOutput =>
            core.addNode(
                'voronoi',
                'field',
                core.fields('$v.voronoi', { x, y, z }),
            ),
    };
}
