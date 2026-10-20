import type { VideoInputKind } from '../../../../shared/video/videoGraph';

export interface VideoParamSpec {
    values: readonly string[];
    default: string;
}

/** The resources a node uses, by index into each kind. */
export interface VideoNodeIndices {
    buffer: number;
    history: number;
    source: number;
    scope: number;
}

export interface VideoModuleDef {
    inputs: Record<string, VideoInputKind>;
    /**
     * Marks a node that reads (`feedbackRead`) or writes (`feedbackWrite`)
     * the feedback buffer named by its `buffer` field.
     */
    buffer?: 'read' | 'write';
    /**
     * Inputs evaluated at coordinates the module chooses rather than at the
     * pixel being drawn. `emit` receives such an input as the name of a
     * function from coordinates to the input's value, and calls it with the
     * coordinates to sample: `${args.input}(uv + offset)`. A warped input is
     * always `rgba`.
     */
    warped?: readonly string[];
    /** Marks a node that reads the audio history row named by its `history` field. */
    history?: boolean;
    /** Marks a node that samples the media named by its `source` field. */
    source?: boolean;
    /** Marks a node that samples the XY scope named by its `scope` field. */
    scope?: boolean;
    /**
     * What `emit` returns: an RGBA `vec4f`, or for `gray` an `f32` that is
     * shown as an opaque gray of that level.
     */
    output: 'rgba' | 'gray';
    /** Enumerated options; none when absent. */
    params?: Record<string, VideoParamSpec>;
    /**
     * WGSL function declarations the expression calls, one per entry. A
     * declaration shared by several modules is emitted once, so equal text
     * means the same function and distinct functions need distinct names.
     * A function of the resolved params picks the declarations those options
     * need.
     */
    helpers?:
        | readonly string[]
        | ((params: Record<string, string>) => readonly string[]);
    /**
     * WGSL expression for the module's value. `args` holds one WGSL
     * expression per input, `params` the resolved option per param, and
     * `indices` the node's feedback buffer, audio history row, media source
     * and XY scope (0 for modules without one).
     */
    emit(
        args: Record<string, string>,
        params: Record<string, string>,
        indices: VideoNodeIndices,
    ): string;
}
