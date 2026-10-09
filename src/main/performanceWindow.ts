import { BrowserWindow } from 'electron';
import path from 'node:path';
import { IPC_CHANNELS } from '../shared/ipcTypes';
import type { CompiledVideoShader } from '../shared/video/videoGraph';

declare const MAIN_WINDOW_VITE_DEV_SERVER_URL: string;
declare const MAIN_WINDOW_VITE_NAME: string;

let performanceWindow: BrowserWindow | null = null;
let latestShader: CompiledVideoShader | null = null;

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
}

/**
 * Applies a control's new value to the shader input bound to it; a no-op for
 * controls the video graph does not read.
 */
export function setVideoControl(moduleId: string, value: number): void {
    const binding = latestShader?.uniforms.find((u) => u.moduleId === moduleId);
    if (binding === undefined) return;
    binding.value = value;
    performanceWindow?.webContents.send(
        IPC_CHANNELS.VIDEO_ON_UNIFORM,
        binding.slot,
        value,
    );
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
