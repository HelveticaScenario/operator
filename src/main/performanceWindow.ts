import { BrowserWindow } from 'electron';
import path from 'node:path';
import { IPC_CHANNELS } from '../shared/ipcTypes';
import type {
    CompiledVideoShader,
    VideoTapSamples,
    VideoUniformUpdate,
} from '../shared/video/videoGraph';

declare const MAIN_WINDOW_VITE_DEV_SERVER_URL: string;
declare const MAIN_WINDOW_VITE_NAME: string;

/** ~60 Hz: often enough that a tap's ring never wraps, rare enough to stay idle. */
const TAP_POLL_MS = 16;

let performanceWindow: BrowserWindow | null = null;
let latestShader: CompiledVideoShader | null = null;
let tapTimer: NodeJS.Timeout | null = null;
let tapSource: TapSource | null = null;
/** How many samples of each tap earlier polls have already taken. */
const tapHeads = new Map<number, number>();

/** Where the engine's audio-signal taps are read from. */
export interface TapSource {
    /** The samples tap `tap` has written since `since`, or its latest few without it. */
    read(tap: number, since?: number): { head: number; samples: number[] };
    sampleRate(): number;
}

export function setVideoTapSource(source: TapSource): void {
    tapSource = source;
}

function sendUniforms(updates: VideoUniformUpdate[]): void {
    if (updates.length === 0) return;
    performanceWindow?.webContents.send(IPC_CHANNELS.VIDEO_ON_UNIFORM, updates);
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
 * Sends the window every audio sample its taps have produced since the last
 * poll. The renderer plays them back against its own clock, so the signals it
 * reads are as smooth as the audio, however the engine's callbacks fall.
 */
function pollTaps(): void {
    if (tapSource === null || performanceWindow === null) return;
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
    if (chunks.length > 0) {
        performanceWindow.webContents.send(
            IPC_CHANNELS.VIDEO_ON_TAP_SAMPLES,
            chunks,
        );
    }
}

/**
 * Polls the engine only while the window is open and the shader reads audio
 * signals, so a patch without them costs no timer.
 */
function syncTapPolling(): void {
    const needed =
        performanceWindow !== null && neededTaps(latestShader).size > 0;
    if (needed && tapTimer === null) {
        tapTimer = setInterval(pollTaps, TAP_POLL_MS);
        pollTaps();
    } else if (!needed && tapTimer !== null) {
        clearInterval(tapTimer);
        tapTimer = null;
    }
}

function createPerformanceWindow(): BrowserWindow {
    const window = new BrowserWindow({
        backgroundColor: '#000000',
        height: 720,
        show: false,
        title: 'Operator Performance',
        webPreferences: {
            // Output keeps rendering while the editor has focus.
            backgroundThrottling: false,
            preload: path.join(__dirname, 'preload.js'),
        },
        width: 1280,
    });

    if (MAIN_WINDOW_VITE_DEV_SERVER_URL) {
        void window.loadURL(MAIN_WINDOW_VITE_DEV_SERVER_URL + '#performance');
    } else {
        void window.loadFile(
            path.join(
                __dirname,
                `../renderer/${MAIN_WINDOW_VITE_NAME}/index.html`,
            ),
            { hash: 'performance' },
        );
    }

    // Shown without focus so the editor keeps the keyboard.
    window.once('ready-to-show', () => window.showInactive());
    window.on('closed', () => {
        performanceWindow = null;
        syncTapPolling();
    });
    return window;
}

/**
 * Records the patch's video shader and delivers it to the performance window,
 * opening the window when a patch first has video output.
 */
export function updateVideoShader(shader: CompiledVideoShader | null): void {
    latestShader = shader;
    if (shader !== null && performanceWindow === null) {
        performanceWindow = createPerformanceWindow();
    }
    performanceWindow?.webContents.send(IPC_CHANNELS.VIDEO_ON_SHADER, shader);
    syncTapPolling();
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

/** Opens the performance window, or closes it when it is already open. */
export function togglePerformanceWindow(): void {
    if (performanceWindow === null) {
        performanceWindow = createPerformanceWindow();
    } else {
        performanceWindow.close();
    }
}
