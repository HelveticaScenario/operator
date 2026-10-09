import { BrowserWindow } from 'electron';
import path from 'node:path';
import { IPC_CHANNELS } from '../shared/ipcTypes';
import type {
    CompiledVideoShader,
    VideoUniformUpdate,
} from '../shared/video/videoGraph';

declare const MAIN_WINDOW_VITE_DEV_SERVER_URL: string;
declare const MAIN_WINDOW_VITE_NAME: string;

/** ~60 Hz: fast enough for control-rate modulation, slow enough to stay idle. */
const TAP_POLL_MS = 16;

let performanceWindow: BrowserWindow | null = null;
let latestShader: CompiledVideoShader | null = null;
let readTaps: ((count: number) => number[]) | null = null;
let tapTimer: NodeJS.Timeout | null = null;

/** Supplies the engine's latest audio-signal tap values, volts by tap index. */
export function setVideoTapReader(read: (count: number) => number[]): void {
    readTaps = read;
}

function sendUniforms(updates: VideoUniformUpdate[]): void {
    if (updates.length === 0) return;
    performanceWindow?.webContents.send(IPC_CHANNELS.VIDEO_ON_UNIFORM, updates);
}

function pollTaps(): void {
    if (latestShader === null || readTaps === null) return;
    const bound = latestShader.uniforms.filter((u) => u.kind === 'tap');
    if (bound.length === 0) return;
    const values = readTaps(Math.max(...bound.map((u) => u.tap)) + 1);
    const updates: VideoUniformUpdate[] = [];
    for (const u of bound) {
        const value = values[u.tap] ?? 0;
        if (value === u.value) continue;
        u.value = value;
        updates.push({ slot: u.slot, value });
    }
    sendUniforms(updates);
}

/**
 * Polls the engine only while the window is open and the shader reads audio
 * signals, so a patch without them costs no timer.
 */
function syncTapPolling(): void {
    const needed =
        performanceWindow !== null &&
        (latestShader?.uniforms.some((u) => u.kind === 'tap') ?? false);
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
