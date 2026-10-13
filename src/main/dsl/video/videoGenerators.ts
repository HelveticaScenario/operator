import type { VideoOutput } from './VideoOutput';
import type {
    VideoCore,
    VideoNoiseConfig,
    VideoOscConfig,
    VideoPositionConfig,
    VideoRampConfig,
    VideoShapeConfig,
    VideoSource,
} from './videoBuilderTypes';

/**
 * The `$v` functions that draw a pattern from coordinates. Each reads the
 * position of the pixel being drawn unless its config names other coordinates,
 * so warps and feedback move it like any other signal.
 */
export function generatorMethods(core: VideoCore) {
    /** The `x` and `y` of `config`, or the ramps across and up the frame. */
    const position = (config: VideoPositionConfig | undefined) => ({
        x: config?.x ?? core.ramp('horizontal'),
        y: config?.y ?? core.ramp('vertical'),
    });
    /** The `params` of a node, holding `name` when the config chose one. */
    const option = (name: string, value: string | undefined) =>
        value === undefined ? undefined : { params: { [name]: value } };

    return {
        /**
         * Scan ramp over the frame, optionally zoomed, rotated and shifted.
         * `horizontal` and `vertical` run 0 to 1 across and up, `radial` is
         * the distance from the center and `angular` the angle around it.
         */
        ramp: (
            axis:
                | 'horizontal'
                | 'vertical'
                | 'radial'
                | 'angular' = 'horizontal',
            config?: VideoRampConfig,
        ): VideoOutput =>
            core.addNode(
                'ramp',
                core.values('$v.ramp', {
                    zoom: config?.zoom ?? 1,
                    rotate: config?.rotate ?? 0,
                    shiftX: config?.shiftX ?? 0,
                    shiftY: config?.shiftY ?? 0,
                }),
                { params: { axis } },
            ),

        /**
         * Periodic wave: `freq` cycles across the frame, or per unit of
         * `config.input`, offset by `phase` cycles.
         */
        osc: (
            freq: VideoSource = 10,
            phase: VideoSource = 0,
            config?: VideoOscConfig,
        ): VideoOutput =>
            core.addNode(
                'osc',
                core.values('$v.osc', {
                    input: config?.input ?? core.ramp('horizontal'),
                    freq,
                    phase,
                }),
                option('shape', config?.shape),
            ),

        /** 1 inside a shape centered on the frame, or where `config.x` and `config.y` are 0.5; 0 outside. */
        shape: (
            size: VideoSource = 0.25,
            softness: VideoSource = 0.01,
            config?: VideoShapeConfig,
        ): VideoOutput =>
            core.addNode(
                'shape',
                core.values('$v.shape', {
                    ...position(config),
                    size,
                    softness,
                }),
                option('shape', config?.shape),
            ),

        /** 1 inside a regular polygon centered on the frame, 0 outside. */
        polygon: (
            sides: VideoSource = 3,
            size: VideoSource = 0.25,
            softness: VideoSource = 0.01,
            config?: VideoPositionConfig,
        ): VideoOutput =>
            core.addNode(
                'polygon',
                core.values('$v.polygon', {
                    ...position(config),
                    sides,
                    size,
                    softness,
                }),
            ),

        /** Smooth noise between 0 and 1, about `scale` features across the frame; `z` moves through it. */
        noise: (
            scale: VideoSource = 5,
            z: VideoSource = 0,
            config?: VideoNoiseConfig,
        ): VideoOutput =>
            core.addNode(
                'noise',
                core.values('$v.noise', { ...position(config), z, scale }),
                option('type', config?.type),
            ),

        /** Cellular noise: the distance to the nearest of a scatter of points, about `scale` across the frame. */
        voronoi: (
            scale: VideoSource = 5,
            z: VideoSource = 0,
            config?: VideoPositionConfig,
        ): VideoOutput =>
            core.addNode(
                'voronoi',
                core.values('$v.voronoi', { ...position(config), z, scale }),
            ),
    };
}
