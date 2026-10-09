import type { VideoValueType } from '../../../../shared/video/videoGraph';

export interface VideoParamSpec {
    values: readonly string[];
    default: string;
}

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
        indices: { buffer: number; history: number },
    ): string;
}
