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
         * `v` run 0 to 1 across and up, `d` along the diagonal, `r` is the distance
         * from the center and `a` the angle around it.
         */
        ramp: (
            axis: 'h' | 'v' | 'd' | 'r' | 'a' = 'h',
            config?: VideoRampConfig,
        ): VideoOutput =>
            core.addNode(
                'ramp',
                'field',
                {
                    zoom: core.asField('$v.ramp', 'zoom', config?.zoom ?? 1),
                    rotate: core.asField(
                        '$v.ramp',
                        'rotate',
                        config?.rotate ?? 0,
                    ),
                    shiftX: core.asField(
                        '$v.ramp',
                        'shiftX',
                        config?.shiftX ?? 0,
                    ),
                    shiftY: core.asField(
                        '$v.ramp',
                        'shiftY',
                        config?.shiftY ?? 0,
                    ),
                },
                { axis },
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
                {
                    input: core.asField('$v.osc', 'input', input),
                    freq: core.asField('$v.osc', 'freq', freq),
                    phase: core.asField('$v.osc', 'phase', phase),
                },
                config?.shape === undefined
                    ? undefined
                    : { shape: config.shape },
            ),

        /** 1 inside a shape centered on (x, y), 0 outside. */
        shape: (
            x: VideoSource,
            y: VideoSource,
            size: VideoSource = 0.25,
            softness: VideoSource = 0.01,
            config?: VideoShapeConfig,
        ): VideoOutput =>
            core.addNode(
                'shape',
                'field',
                {
                    x: core.asField('$v.shape', 'x', x),
                    y: core.asField('$v.shape', 'y', y),
                    size: core.asField('$v.shape', 'size', size),
                    softness: core.asField('$v.shape', 'softness', softness),
                },
                config?.shape === undefined
                    ? undefined
                    : { shape: config.shape },
            ),

        /** 1 inside a regular polygon centered on (x, y), 0 outside. */
        polygon: (
            x: VideoSource,
            y: VideoSource,
            sides: VideoSource = 3,
            size: VideoSource = 0.25,
            softness: VideoSource = 0.01,
        ): VideoOutput =>
            core.addNode('polygon', 'field', {
                sides: core.asField('$v.polygon', 'sides', sides),
                size: core.asField('$v.polygon', 'size', size),
                softness: core.asField('$v.polygon', 'softness', softness),
                x: core.asField('$v.polygon', 'x', x),
                y: core.asField('$v.polygon', 'y', y),
            }),

        /** Smooth value noise between 0 and 1; `z` moves through it. */
        noise: (
            x: VideoSource,
            y: VideoSource,
            z: VideoSource = 0,
        ): VideoOutput =>
            core.addNode('noise', 'field', {
                x: core.asField('$v.noise', 'x', x),
                y: core.asField('$v.noise', 'y', y),
                z: core.asField('$v.noise', 'z', z),
            }),

        /** Cellular noise: the distance to the nearest of a scatter of points; `z` moves them. */
        voronoi: (
            x: VideoSource,
            y: VideoSource,
            z: VideoSource = 0,
        ): VideoOutput =>
            core.addNode('voronoi', 'field', {
                x: core.asField('$v.voronoi', 'x', x),
                y: core.asField('$v.voronoi', 'y', y),
                z: core.asField('$v.voronoi', 'z', z),
            }),
    };
}
