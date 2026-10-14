import type { WebContents } from 'electron';
import { IPC_CHANNELS } from '../shared/ipcTypes';
import type {
    CompiledVideoShader,
    VideoPull,
    VideoRestart,
    VideoShaderUpdate,
    VideoTapSamples,
    VideoUniformUpdate,
} from '../shared/video/videoGraph';

/** The editor window, which draws the picture. */
let target: (() => WebContents | null) | null = null;
/** The latest shader submitted. */
let latest: VideoShaderUpdate = { shader: null, updateId: 0 };
/** The shader of the latest update the engine reported applying: the one the renderer draws. */
let live: CompiledVideoShader | null = null;
/** The latest update id the engine reported applying. */
let lastApplied = 0;
let tapSource: TapSource | null = null;
/** How many samples of each tap earlier polls have already taken. */
const tapHeads = new Map<number, number>();

/** Where the engine's audio-signal taps are read from. */
export interface TapSource {
    /** The samples tap `tap` has written since `since`, or its latest few without it. */
    read(tap: number, since?: number): { head: number; samples: Float32Array };
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
 * keeps drawing the live shader until the engine applies the latest patch, so
 * both are served.
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
 * Makes the latest shader live once the engine reports applying its update,
 * or any update after an engine whose numbering started over. A queued update
 * that a newer one supersedes is discarded by the engine, so no shader between
 * the live one and the latest can go live.
 */
function track(applied: number): void {
    if (applied >= latest.updateId || applied < lastApplied) {
        live = latest.shader;
    }
    lastApplied = applied;
}

/**
 * Whether the engine is running, and every audio sample the shader's taps have
 * produced since the last pull. The renderer asks once per display frame and
 * plays the samples back against its own clock, so the signals it reads are as
 * smooth as the audio however the engine's callbacks fall. The taps in
 * `restart` start over from the engine's newest samples, as a tap that now
 * carries another signal needs.
 */
export function pullVideo(restart: VideoRestart = []): VideoPull {
    if (restart === 'all') tapHeads.clear();
    else for (const tap of restart) tapHeads.delete(tap);
    if (tapSource === null) {
        return { applied: 0, cancelled: 0, running: false, taps: [] };
    }
    if (tapSource.isStopped()) {
        const updates = tapSource.updates();
        track(updates.applied);
        return { ...updates, running: false, taps: [] };
    }
    const chunks: VideoTapSamples[] = [];
    const sampleRate = tapSource.sampleRate();
    for (const tap of neededTaps(live, latest.shader)) {
        const { head, samples } = tapSource.read(tap, tapHeads.get(tap));
        tapHeads.set(tap, head);
        if (samples.length > 0) {
            chunks.push({
                sampleRate,
                samples,
                tap,
            });
        }
    }
    // Read after the samples: an update that applied while they were read
    // shows up here, and its samples are then not mistaken for the old patch's.
    const updates = tapSource.updates();
    track(updates.applied);
    return { ...updates, running: true, taps: chunks };
}

/** Records the patch's video shader and delivers it to the editor window, which draws it. */
export function updateVideoShader(
    shader: CompiledVideoShader | null,
    updateId: number,
): void {
    latest = { shader, updateId };
    send(IPC_CHANNELS.VIDEO_ON_SHADER, latest);
}

/**
 * Applies a control's new value to every shader that reads it: the live one
 * and the latest, which may still be waiting for its update. The renderer
 * finds the input bound to the control in each of its programs, since each
 * numbers its slots its own way. A no-op for controls neither shader reads.
 */
export function setVideoControl(moduleId: string, value: number): void {
    let read = false;
    for (const shader of new Set([live, latest.shader])) {
        for (const u of shader?.uniforms ?? []) {
            if (u.kind === 'control' && u.moduleId === moduleId) {
                u.value = value;
                read = true;
            }
        }
    }
    if (read) sendUniforms([{ moduleId, value }]);
}

export function getVideoShader(): VideoShaderUpdate {
    return latest;
}
