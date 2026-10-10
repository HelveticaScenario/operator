import type { VideoValueType } from '../../../../shared/video/videoGraph';

export interface VideoParamSpec {
    values: readonly string[];
    default: string;
}

/**
 * A video shader module. Inside `emit`, a field's value is a fraction of 5
 * volts, so 5 volts is 1 and the comments of the modules give values on that
 * scale; the DSL's volts are converted as inputs are resolved.
 */
export interface VideoModuleDef {
    inputs: Record<string, VideoValueType>;
    /**
     * Marks a node that reads (`feedbackRead`) or writes (`feedbackWrite`)
     * the feedback buffer named by its `buffer` field.
     */
    buffer?: 'read' | 'write';
    /**
     * Inputs evaluated at coordinates the module chooses rather than at the
     * pixel being drawn. `emit` receives such an input as the name of a
     * function from coordinates to the input's value, and calls it with the
     * coordinates to sample: `${args.input}(uv + offset)`.
     */
    warped?: readonly string[];
    /** Marks a node that reads the audio history row named by its `history` field. */
    history?: boolean;
    /** Marks a node that samples the media named by its `source` field. */
    source?: boolean;
    /**
     * Field inputs measured in natural units (cycles, turns, a zoom factor, a
     * count) rather than as a fraction of full scale. Every other field input
     * receives its value as a fraction of 5 volts, so `emit` sees 5 volts as 1;
     * a natural input receives the volts themselves.
     */
    natural?: readonly string[];
    output: VideoValueType;
    params: Record<string, VideoParamSpec>;
    /**
     * WGSL function declarations the expression calls, one per entry. A
     * declaration shared by several modules is emitted once, so equal text
     * means the same function and distinct functions need distinct names.
     */
    helpers?: readonly string[];
    /**
     * WGSL expression for the module's value. `args` holds one WGSL
     * expression per input, `params` the resolved option per param, and
     * `indices` the node's feedback buffer and audio history row (0 for
     * modules without one).
     */
    emit(
        args: Record<string, string>,
        params: Record<string, string>,
        indices: { buffer: number; history: number; source: number },
    ): string;
}
