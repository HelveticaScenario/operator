/**
 * Value types a video module input or output carries. A `field` is a scalar
 * f(x, y, t); a `color` is an RGB triple of fields.
 */
export type VideoValueType = 'field' | 'color';

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
}

/**
 * A video patch. `nodes` is in dependency order: a node only references
 * nodes that precede it. `output` names the `color` node shown on screen.
 */
export interface VideoGraph {
    nodes: VideoNode[];
    output: string;
    uniformSlotCount: number;
}

export interface CompiledVideoShader {
    wgsl: string;
    /** Total uniform buffer size in floats, a multiple of 4. */
    uniformFloatCount: number;
}
