/**
 * Value types a video module input or output carries. A `field` is a scalar
 * f(x, y, t); a `color` is an RGB triple of fields.
 */
export type VideoValueType = 'field' | 'color';

/**
 * Most feedback buffers a graph can use: the screen takes one of the eight
 * color attachments WebGPU guarantees.
 */
export const MAX_FEEDBACK_BUFFERS = 7;

/** Where a module input reads its value from. */
export type VideoValue =
    | { kind: 'node'; id: string }
    | { kind: 'const'; value: number }
    /** Index into the per-frame uniform slots. */
    | { kind: 'uniform'; slot: number }
    /** Seconds since the output window started rendering. */
    | { kind: 'time' };

export interface VideoNode {
    id: string;
    kind: string;
    inputs: Record<string, VideoValue>;
    /** Enumerated module options; omitted entries take the module default. */
    params?: Record<string, string>;
    /**
     * Feedback buffer a `feedbackRead` or `feedbackWrite` node uses. A buffer
     * holds what its write node saw on the previous frame.
     */
    buffer?: number;
}

/** A control (slider or button) whose value feeds one uniform slot. */
export interface VideoUniform {
    slot: number;
    /** Module id of the control's backing `$signal`. */
    moduleId: string;
    /** The control's current value. */
    value: number;
}

/**
 * A video patch. `nodes` is in dependency order: a node only references
 * nodes that precede it. `output` names the `color` node shown on screen.
 */
export interface VideoGraph {
    nodes: VideoNode[];
    output: string;
    /** Slot `i` is `uniforms[i]`. */
    uniforms: VideoUniform[];
}

export interface CompiledVideoShader {
    wgsl: string;
    /** Total uniform buffer size in floats, a multiple of 4. */
    uniformFloatCount: number;
    uniforms: VideoUniform[];
    /** Feedback buffers the shader reads (bindings 2..) and writes (locations 1..). */
    feedbackBufferCount: number;
}
