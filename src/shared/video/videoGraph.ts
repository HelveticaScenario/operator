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

/** Number of audio-signal taps the engine publishes; matches `MAX_VIDEO_TAPS` in Rust. */
export const MAX_VIDEO_TAPS = 64;

/** An input source that feeds one uniform slot, with that source's current value. */
export type VideoUniform = {
    slot: number;
    value: number;
} & (
    | {
          /** A slider or button. */
          kind: 'control';
          /** Module id of the control's backing `$signal`. */
          moduleId: string;
      }
    | {
          /** An audio signal, published by a `_videoTap` module. */
          kind: 'tap';
          /** Index of the tap slot the engine publishes the signal to. */
          tap: number;
      }
);

/** How the editor draws a preview. */
export type VideoPreviewView = 'image' | 'waveform' | 'vectorscope';

/** Editor-side description of one `$v.preview` call. */
export interface VideoPreviewSite {
    /** Position among the shader's previews; frames carry the same index. */
    index: number;
    view: VideoPreviewView;
    /** The call site, as V8 reports it: 1-based, with line-1 columns shifted. */
    sourceLocation?: { line: number; column: number };
}

/** The averages of every CV region in one frame, to write to the audio graph. */
export interface VideoCvValue {
    id: string;
    value: number;
}

/** One drawn preview, as tightly packed RGBA8 rows from the top. */
export interface VideoPreviewFrame {
    /** Position among the shader's previews. */
    index: number;
    width: number;
    height: number;
    data: Uint8Array;
}

/** A new value for one uniform slot. */
export interface VideoUniformUpdate {
    slot: number;
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
    /** Signals drawn as small previews for the editor, in call order. */
    previews: VideoPreview[];
}

/** A region of the frame whose average becomes an audio control signal. */
export interface VideoCvSample {
    /** Module id of the `$signal` the average is written to. */
    id: string;
    /** Center of the region, as fractions of the frame (0, 0 is bottom left). */
    x: number;
    y: number;
    /** Half-width and half-height of the region, as fractions of the frame. */
    size: number;
}

/**
 * A signal drawn into a small target each frame. The editor shows it beside
 * the code, or, when `cv` is set, the renderer averages a region of it into an
 * audio control signal.
 */
export interface VideoPreview {
    value: VideoValue;
    type: VideoValueType;
    cv?: VideoCvSample;
}

export interface CompiledVideoShader {
    wgsl: string;
    /** Total uniform buffer size in floats, a multiple of 4. */
    uniformFloatCount: number;
    uniforms: VideoUniform[];
    /** Feedback buffers the shader reads (bindings 2..) and writes (locations 1..). */
    feedbackBufferCount: number;
    /** Fragment entry points `preview_0`.. that each draw one preview. */
    previewCount: number;
    /** The previews that feed audio control signals, by preview index. */
    cvSamples: ({ index: number } & VideoCvSample)[];
}
