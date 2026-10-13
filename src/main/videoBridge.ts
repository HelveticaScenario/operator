import type { WebContents } from 'electron';
import { IPC_CHANNELS } from '../shared/ipcTypes';
import type {
    CompiledVideoShader,
    VideoPull,
    VideoShaderUpdate,
    VideoTapSamples,
    VideoUniformUpdate,
} from '../shared/video/videoGraph';

/** The editor window, which draws the picture. */
let target: (() => WebContents | null) | null = null;
/** The latest shader, and the one before it, whose taps the renderer may still be reading. */
let latest: VideoShaderUpdate = { shader: null, updateId: 0 };
let previous: CompiledVideoShader | null = null;
let tapSource: TapSource | null = null;
/** How many samples of each tap earlier polls have already taken. */
const tapHeads = new Map<number, number>();

/** Where the engine's audio-signal taps are read from. */
export interface TapSource {
    /** The samples tap `tap` has written since `since`, or its latest few without it. */
    read(tap: number, since?: number): { head: number; samples: number[] };
    sampleRate(): number;
    isStopped(): boolean;
    /** Ids of the latest patch updates the engine applied and discarded. */
    updates(): { applied: number; cancelled: number };
}

export function setVideoTapSource(source: TapSource): void {
    tapSource = source;
}

/** Names the window that draws the picture. */
export function setVideoTarget(get: () => WebContents | null): void {
    target = get;
}

function send(channel: string, payload: unknown): void {
    const contents = target?.() ?? null;
    if (contents !== null && !contents.isDestroyed()) {
        contents.send(channel, payload);
    }
}

function sendUniforms(updates: VideoUniformUpdate[]): void {
    if (updates.length === 0) return;
    send(IPC_CHANNELS.VIDEO_ON_UNIFORM, updates);
}

/**
 * The engine taps the shaders read, as uniforms or audio history. The renderer
 * keeps drawing the previous shader until the engine applies the latest
 * patch, so both are served.
 */
function neededTaps(...shaders: (CompiledVideoShader | null)[]): Set<number> {
    const taps = new Set<number>();
    for (const shader of shaders) {
        for (const u of shader?.uniforms ?? []) {
            if (u.kind === 'tap') taps.add(u.tap);
        }
        for (const h of shader?.histories ?? []) taps.add(h.tap);
    }
    return taps;
}

/**
 * Whether the engine is running, and every audio sample the shader's taps have
 * produced since the last pull. The renderer asks once per display frame and
 * plays the samples back against its own clock, so the signals it reads are as
 * smooth as the audio however the engine's callbacks fall. A `fresh` pull
 * restarts every tap from the engine's newest samples, as a new shader needs.
 */
export function pullVideo(fresh = false): VideoPull {
    if (fresh) tapHeads.clear();
    if (tapSource === null) {
        return { applied: 0, cancelled: 0, running: false, taps: [] };
    }
    if (tapSource.isStopped()) {
        return { ...tapSource.updates(), running: false, taps: [] };
    }
    const chunks: VideoTapSamples[] = [];
    const sampleRate = tapSource.sampleRate();
    for (const tap of neededTaps(latest.shader, previous)) {
        const { head, samples } = tapSource.read(tap, tapHeads.get(tap));
        tapHeads.set(tap, head);
        if (samples.length > 0) {
            chunks.push({
                sampleRate,
                samples: Float32Array.from(samples),
                tap,
            });
        }
    }
    // Read after the samples: an update that applied while they were read
    // shows up here, and its samples are then not mistaken for the old patch's.
    return { ...tapSource.updates(), running: true, taps: chunks };
}

/** Records the patch's video shader and delivers it to the editor window, which draws it. */
export function updateVideoShader(
    shader: CompiledVideoShader | null,
    updateId: number,
): void {
    previous = latest.shader;
    latest = { shader, updateId };
    send(IPC_CHANNELS.VIDEO_ON_SHADER, latest);
}

/**
 * Applies a control's new value to the shader input bound to it; a no-op for
 * controls the video graph does not read.
 */
export function setVideoControl(moduleId: string, value: number): void {
    const binding = latest.shader?.uniforms.find(
        (u) => u.kind === 'control' && u.moduleId === moduleId,
    );
    if (binding === undefined) return;
    binding.value = value;
    sendUniforms([{ slot: binding.slot, value }]);
}

export function getVideoShader(): VideoShaderUpdate {
    return latest;
}
