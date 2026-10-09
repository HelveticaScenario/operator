import type {
    VideoPreviewView,
    VideoValue,
    VideoValueType,
} from '../../../shared/video/videoGraph';
import {
    BaseCollection,
    type CollectionWithRange,
    ModuleOutput,
} from '../GraphBuilder';
import { VideoOutput } from './VideoOutput';

/**
 * A constant, a video signal of either type, or an audio signal (a slider,
 * button or any module output), whose live value drives a field input.
 */
export type VideoSource =
    | number
    | VideoOutput
    | ModuleOutput
    | BaseCollection<ModuleOutput>;

/** What the builder needs from the patch it is building inside. */
export interface VideoGraphHost {
    /**
     * Current value of a slider or button's backing module, or undefined if
     * `moduleId` is not a control.
     */
    controlValue(moduleId: string): number | undefined;
    /** Publishes `output` to tap slot `slot`, as the engine's `_videoTap` module. */
    publishTap(output: ModuleOutput, slot: number): void;
    /**
     * A `$signal` module with id `id` carrying 0..1, which the renderer
     * overwrites with a region average of a video signal.
     */
    cvSignal(id: string): CollectionWithRange;
    /** Where the patch script is calling from, as V8 reports it. */
    sourceLocation(): { line: number; column: number } | undefined;
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

export interface VideoShapeConfig {
    shape?: 'circle' | 'box' | 'diamond';
}

export function describe(value: unknown): string {
    if (value instanceof VideoOutput) return `a ${value.type}`;
    if (value instanceof ModuleOutput || value instanceof BaseCollection) {
        return 'an audio signal';
    }
    return String(value);
}

export const isColor = (value: unknown): boolean =>
    value instanceof VideoOutput && value.type === 'color';

/**
 * What the groups of `$v` functions in the `video*.ts` files need from the
 * builder: the primitives that turn arguments into node inputs and add nodes.
 */
export interface VideoCore {
    /** An input that must be a field: a number, a video field, or an audio signal. */
    asField(fn: string, name: string, v: unknown): VideoValue;
    asColor(fn: string, name: string, v: unknown): VideoValue;
    /** A color operand; a field or number becomes the gray of that level. */
    asColorOrGray(fn: string, name: string, v: unknown): VideoValue;
    addNode(
        kind: string,
        type: VideoValueType,
        inputs: Record<string, VideoValue>,
        params?: Record<string, string>,
        buffer?: number,
        history?: number,
    ): VideoOutput;
    /** A math node on fields, or on colors when any operand is a color. */
    arith(
        fn: string,
        kind: string,
        operands: Record<string, unknown>,
        fieldInputs?: Record<string, unknown>,
    ): VideoOutput;
    /** One channel of a color as a field. */
    channel(input: VideoOutput, which?: 'r' | 'g' | 'b' | 'luma'): VideoOutput;
}
