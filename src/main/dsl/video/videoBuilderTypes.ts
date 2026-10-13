import type {
    VideoNode,
    VideoPreviewView,
    VideoSourceDef,
    VideoValue,
} from '../../../shared/video/videoGraph';
import {
    BaseCollection,
    type Collection,
    type CollectionWithRange,
    ModuleOutput,
} from '../GraphBuilder';
import { VideoOutput } from './VideoOutput';

/** An audio signal a video input reads: one output, or channels the engine sums to one. */
export type AudioSignal = ModuleOutput | BaseCollection<ModuleOutput>;

/**
 * A constant, a video signal, or an audio signal (a slider, button or any
 * module output), whose live value drives an input.
 */
export type VideoSource = number | VideoOutput | AudioSignal;

/** What the builder needs from the patch it is building inside. */
export interface VideoGraphHost {
    /**
     * Current value of a slider or button's backing module, or undefined if
     * `moduleId` is not a control.
     */
    controlValue(moduleId: string): number | undefined;
    /**
     * Plays a pattern value (`$p(...)`, `$p.s(...)`, `$p.arrange(...)` or a
     * chain of them) through its own `$cycle`, as a signal parameter does.
     */
    playPattern(pattern: unknown): AudioSignal;
    /**
     * Publishes `signal` to tap slot `slot`, as the engine's `_videoTap`
     * module, which sums its channels to one.
     */
    publishTap(signal: AudioSignal, slot: number): void;
    /**
     * A `$signal` module with id `id` carrying 0..1, which the renderer
     * overwrites with a region average of a video signal.
     */
    cvSignal(id: string): CollectionWithRange;
    /**
     * The decoded audio track of a workspace media file, played in a loop at
     * `playback.speed` between the loop points in seconds, as an audio signal.
     */
    mediaAudio(path: string, playback: VideoAudioPlayback): Collection;
    /** Where the patch script is calling from, as V8 reports it. */
    sourceLocation(): { line: number; column: number } | undefined;
    /**
     * Whether a file exists in the workspace folder, for a friendly error when
     * a patch names missing media. Leave it out to skip the check.
     */
    mediaExists?(path: string): boolean;
}

/** How a video's audio plays: the video's speed and loop points. */
export interface VideoAudioPlayback {
    speed: number;
    loopStart: number;
    /** Seconds; the end of the track when absent. */
    loopEnd?: number;
}

export interface VideoAudioConfig {
    /** Samples the window spans, 2 to 4096 (default 512; 48 000 per second). */
    samples?: number;
    /** Start the window at a rising zero crossing so a periodic wave holds still (default true). */
    trigger?: boolean;
}

export interface VideoCvConfig {
    /** Center of the region as a fraction of the frame width (default 0.5). */
    x?: number;
    /** Center of the region as a fraction of the frame height; 0 is the bottom (default 0.5). */
    y?: number;
    /** Half-extent of the region as a fraction of the frame (default 0.5, the whole frame). */
    size?: number;
}

export interface VideoPreviewConfig {
    /** How the editor draws the signal (default 'image'). */
    view?: VideoPreviewView;
}

export interface VideoOscConfig {
    shape?: 'sine' | 'triangle' | 'saw' | 'square';
    /** What the wave runs along (default the horizontal ramp, 0 to 1 across the frame). */
    input?: VideoSource;
}

/** Where a generator is drawn: the coordinates it reads at each pixel. */
export interface VideoPositionConfig {
    /** Horizontal coordinate at each pixel (default the horizontal ramp, 0 to 1 across the frame). */
    mapX?: VideoSource;
    /** Vertical coordinate at each pixel (default the vertical ramp, 0 to 1 up the frame). */
    mapY?: VideoSource;
}

export interface VideoRampConfig {
    /** Magnification about the center (default 1). */
    zoom?: VideoSource;
    /** Turns; positive turns the pattern clockwise (default 0). */
    rotate?: VideoSource;
    /** Horizontal move, as a fraction of frame width (default 0). */
    shiftX?: VideoSource;
    /** Vertical move, as a fraction of frame height (default 0). */
    shiftY?: VideoSource;
}

export interface VideoFeedbackConfig {
    /** Magnification of the previous frame about the center (default 1). */
    zoom?: VideoSource;
    /** Turns per frame (default 0). */
    rotate?: VideoSource;
    /** Horizontal move, as a fraction of frame width per frame (default 0). */
    shiftX?: VideoSource;
    /** Vertical move, as a fraction of frame height per frame (default 0). */
    shiftY?: VideoSource;
    /** What lies beyond the frame border (default 'clamp'). */
    edge?: 'clamp' | 'repeat' | 'mirror';
}

export interface VideoNoiseConfig extends VideoPositionConfig {
    /** The noise algorithm (default 'simplex'). */
    type?: 'simplex' | 'perlin' | 'value' | 'fbm' | 'ridged';
}

export interface VideoKeepConfig {
    /** Keep the side below the threshold instead of the side above (default false). */
    invert?: boolean;
}

export interface VideoEdgeConfig extends VideoPositionConfig {
    /** The curve the soft edge follows from inside to outside (default 'linear'). */
    falloff?: 'linear' | 'smooth' | 'smoother' | 'in' | 'out';
}

export interface VideoShapeConfig extends VideoEdgeConfig {
    shape?: 'circle' | 'box' | 'diamond';
}

export type VideoInputs = Record<string, VideoValue>;

/** A node's options and the buffer, audio history row or media it uses. */
export type VideoNodeExtra = Pick<
    VideoNode,
    'params' | 'buffer' | 'history' | 'source'
>;

export function describe(value: unknown): string {
    if (value instanceof VideoOutput) return 'a video signal';
    if (value instanceof ModuleOutput || value instanceof BaseCollection) {
        return 'an audio signal';
    }
    return String(value);
}

/**
 * What the groups of `$v` functions in the `video*.ts` files need from the
 * builder: the primitives that turn arguments into node inputs and add nodes.
 */
export interface VideoCore {
    /** An input: a number, a video signal, or an audio signal. */
    value(fn: string, name: string, v: unknown): VideoValue;
    /** Each of `values` as an input, named by its key. */
    values(fn: string, values: Record<string, unknown>): VideoInputs;
    addNode(
        kind: string,
        inputs: VideoInputs,
        extra?: VideoNodeExtra,
    ): VideoOutput;
    /** The index of `def` among the media the graph samples, adding it on first use. */
    sourceIndex(def: VideoSourceDef): number;
    /** Whether a workspace file exists. */
    mediaExists(path: string): boolean;
    /** The audio of a workspace video, as an audio signal. */
    mediaAudio(path: string, playback: VideoAudioPlayback): Collection;
    /** One channel of a signal as a gray signal. */
    channel(
        input: VideoOutput,
        which?: 'r' | 'g' | 'b' | 'a' | 'luma',
    ): VideoOutput;
    /** A ramp across the frame: `horizontal` 0 to 1 across, `vertical` 0 to 1 up. */
    ramp(axis: 'horizontal' | 'vertical'): VideoOutput;
}

export interface VideoMediaConfig {
    /** How a picture of another shape fills the frame (default 'cover'). */
    fit?: 'cover' | 'contain' | 'stretch';
}

export interface VideoCameraConfig extends VideoMediaConfig {
    /** Part of the camera's name, matched without regard to case; the first camera by default. */
    device?: string;
}

export interface VideoScreenConfig extends VideoMediaConfig {
    /** Which display to show, counting from 1 (default 1). */
    display?: number;
}

export interface VideoVideoConfig extends VideoMediaConfig {
    /** Playback rate: 1 is normal speed, 0 holds the current frame (default 1). */
    speed?: number;
    /** Seconds the loop plays between, `[start, end]`; `end` defaults to the end of the file. */
    loop?: [start: number, end?: number];
}
