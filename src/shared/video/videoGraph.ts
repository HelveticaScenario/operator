/**
 * How a video module input reads its value. Every signal is an RGBA color per
 * pixel: an `rgba` input takes it whole, and a `number` input takes its luma.
 * A number given to an `rgba` input is an opaque gray of that level.
 */
export type VideoInputKind = 'rgba' | 'number';

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
    /** Seconds since the audio engine last started running. */
    | { kind: 'time' };

export interface VideoNode {
    id: string;
    kind: string;
    inputs: Record<string, VideoValue>;
    /** Enumerated module options; omitted entries take the module default. */
    params?: Record<string, string>;
    /** Row of the audio history texture an `audioHistory` node reads. */
    history?: number;
    /** Index into the graph's `sources` of the media a `source` node samples. */
    source?: number;
    /**
     * Feedback buffer a `feedbackRead` or `feedbackWrite` node uses. A buffer
     * holds what its write node saw on the previous frame.
     */
    buffer?: number;
    /** Index into the graph's `scopes` of the XY scope a `scopeXY` node shows. */
    scope?: number;
}

/** Samples per row of the audio history texture; matches `VIDEO_HISTORY_LEN` in Rust. */
export const VIDEO_HISTORY_LEN = 4096;

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
          /** An audio signal, played back from its engine tap each frame. */
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

/**
 * One drawn preview, as tightly packed RGBA8 rows from the top. The alpha is
 * the signal's own and the color is not laid over anything, so a transparent
 * pixel is distinct from black.
 */
export interface VideoPreviewFrame {
    /** Position among the shader's previews. */
    index: number;
    width: number;
    height: number;
    data: Uint8Array<ArrayBuffer>;
}

/** A tap's recent audio-rate samples that the shader reads along a scanline. */
export interface VideoHistory {
    /** The engine tap that carries the signal. */
    tap: number;
    /** Samples the shader spans; at most {@link VIDEO_HISTORY_LEN}. */
    samples: number;
    /** Start the window at a rising zero crossing, so a periodic wave holds still. */
    trigger: boolean;
}

/** New audio-rate samples of one engine tap, oldest first, in volts. */
export interface VideoTapSamples {
    tap: number;
    samples: Float32Array;
    sampleRate: number;
}

/**
 * A patch's video shader and the audio engine update it belongs to. The
 * numbering of the audio taps changes with every patch, so the renderer
 * switches shader only once the engine reports that update applied.
 */
export interface VideoShaderUpdate {
    shader: CompiledVideoShader | null;
    updateId: number;
}

/**
 * The taps a pull reads afresh from the engine's newest samples, rather than
 * from where the previous pull left off: every tap, or the listed slots.
 */
export type VideoRestart = 'all' | number[];

/** What the video renderer asks the main process for once per frame. */
export interface VideoPull {
    /** False while the engine is stopped; the picture holds still until it runs again. */
    running: boolean;
    /** Id of the latest patch update the audio engine has applied. */
    applied: number;
    /** Id of the latest queued patch update the audio engine discarded. */
    cancelled: number;
    /** The audio taps' samples since the last pull; empty while stopped. */
    taps: VideoTapSamples[];
}

/**
 * A new value for a control the video graph reads. Each shader binds the
 * control to its own uniform slot, so the renderer looks the slot up in every
 * program it holds.
 */
export interface VideoUniformUpdate {
    /** Module id of the control's backing `$signal`. */
    moduleId: string;
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
    /** Row `i` of the audio history texture is `histories[i]`. */
    histories: VideoHistory[];
    /** Media the picture samples; `sources[i]` is what `source` nodes with `source: i` read. */
    sources: VideoSourceDef[];
    /** XY scopes the picture samples; `scopes[i]` is what `scopeXY` nodes with `scope: i` read. */
    scopes: VideoScope[];
    /** True when the patch calls `$v.out`; without it the picture is black and only previews draw. */
    hasOutput: boolean;
}

/**
 * An XY scope: pairs of audio signals drawn as a glowing beam, x across and y
 * up, into a texture the size of the frame that `scopeXY` nodes sample. The
 * beam fades by `persistence` each frame, so it leaves a trail.
 */
export interface VideoScope {
    /** Each trace's horizontal and vertical signal, as rows of `histories`. */
    traces: { x: number; y: number }[];
    /** Samples of each signal drawn per frame, newest last; at most {@link VIDEO_HISTORY_LEN}. */
    samples: number;
    /** Volts at the left and right edges. */
    xRange: [number, number];
    /** Volts at the bottom and top edges. */
    yRange: [number, number];
    /** Brightness of the beam, 0 to 1. */
    intensity: VideoScopeSetting;
    /** How much of the trail survives each frame, 0 (none) to 1 (all). */
    persistence: VideoScopeSetting;
    /** Thickness of the beam relative to the standard width, which is 1. */
    lineWidth: VideoScopeSetting;
    /** Draw a smooth curve between samples instead of straight segments. */
    upsample: boolean;
}

/**
 * A scope setting: a constant, or a uniform slot read once per frame, held
 * within its range in {@link SCOPE_SETTING_RANGES}.
 */
export type VideoScopeSetting = Extract<
    VideoValue,
    { kind: 'const' } | { kind: 'uniform' }
>;

/** The range each scope setting is held in, and its default. */
export const SCOPE_SETTING_RANGES = {
    intensity: { default: 0.6, max: 1, min: 0 },
    lineWidth: { default: 1, max: 5, min: 0.1 },
    persistence: { default: 0.6, max: 1, min: 0 },
} as const;

/** Most traces one XY scope draws. */
export const MAX_SCOPE_TRACES = 32;

/** A picture, recording, camera or screen that a patch draws from. */
export interface VideoSourceDef {
    kind: 'image' | 'video' | 'camera' | 'screen';
    /** Path relative to the workspace folder; empty for a camera or a screen. */
    path: string;
    /** For a camera, part of its name, matched without regard to case; the first camera when absent. */
    device?: string;
    /** For a screen, which display, counting from 1; the first when absent. */
    display?: number;
    /** Playback rate of a video, 1 being normal speed and 0 paused. */
    speed?: number;
    /** Seconds into a video where playback loops back to; 0 when absent. */
    loopStart?: number;
    /** Seconds into a video where playback loops back to `loopStart`; the end of the file when absent. */
    loopEnd?: number;
}

/** Whether two definitions name the same media with the same settings. */
export const sameSource = (a: VideoSourceDef, b: VideoSourceDef): boolean =>
    a.kind === b.kind &&
    a.path === b.path &&
    a.device === b.device &&
    a.display === b.display &&
    a.speed === b.speed &&
    a.loopStart === b.loopStart &&
    a.loopEnd === b.loopEnd;

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
    /** Audio history rows the shader reads; see `bindingSlots` for where they bind. */
    histories: VideoHistory[];
    /** Media the shader samples, one texture each; see `bindingSlots`. */
    sources: VideoSourceDef[];
    /** XY scopes drawn before the picture, one texture each; see `bindingSlots`. */
    scopes: VideoScope[];
    /** True when the patch calls `$v.out`, so the picture is worth showing. */
    hasOutput: boolean;
    /**
     * A stamp per tap slot, set by the main process: a slot whose stamp is the
     * same in two shaders carries the same signal, unbroken, through the patch
     * update between them; 0 marks a slot the shader does not use.
     */
    tapStamps?: number[];
}
