import type { WebContents } from 'electron';
import { IPC_CHANNELS } from '../shared/ipcTypes';
import type {
    CompiledVideoShader,
    VideoPull,
    VideoTapSamples,
    VideoUniformUpdate,
} from '../shared/video/videoGraph';

/** The editor window, which draws the picture. */
let target: (() => WebContents | null) | null = null;
let latestShader: CompiledVideoShader | null = null;
let tapSource: TapSource | null = null;
/** How many samples of each tap earlier polls have already taken. */
const tapHeads = new Map<number, number>();

/** Where the engine's audio-signal taps are read from. */
export interface TapSource {
    /** The samples tap `tap` has written since `since`, or its latest few without it. */
    read(tap: number, since?: number): { head: number; samples: number[] };
    sampleRate(): number;
    isStopped(): boolean;
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

/** The engine taps the shader reads, as uniforms or audio history. */
function neededTaps(shader: CompiledVideoShader | null): Set<number> {
    const taps = new Set<number>();
    for (const u of shader?.uniforms ?? []) {
        if (u.kind === 'tap') taps.add(u.tap);
    }
    for (const h of shader?.histories ?? []) taps.add(h.tap);
    return taps;
}

/**
 * Whether the engine is running, and every audio sample the shader's taps have
 * produced since the last pull. The renderer asks once per display frame and
 * plays the samples back against its own clock, so the signals it reads are as
 * smooth as the audio however the engine's callbacks fall.
 */
export function pullVideo(): VideoPull {
    if (tapSource === null || tapSource.isStopped()) {
        return { running: false, taps: [] };
    }
    const chunks: VideoTapSamples[] = [];
    const sampleRate = tapSource.sampleRate();
    for (const tap of neededTaps(latestShader)) {
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
    return { running: true, taps: chunks };
}

/** Records the patch's video shader and delivers it to the editor window, which draws it. */
export function updateVideoShader(shader: CompiledVideoShader | null): void {
    latestShader = shader;
    send(IPC_CHANNELS.VIDEO_ON_SHADER, shader);
}

/**
 * Applies a control's new value to the shader input bound to it; a no-op for
 * controls the video graph does not read.
 */
export function setVideoControl(moduleId: string, value: number): void {
    const binding = latestShader?.uniforms.find(
        (u) => u.kind === 'control' && u.moduleId === moduleId,
    );
    if (binding === undefined) return;
    binding.value = value;
    sendUniforms([{ slot: binding.slot, value }]);
}

export function getVideoShader(): CompiledVideoShader | null {
    return latestShader;
}
