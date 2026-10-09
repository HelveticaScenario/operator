import { BrowserWindow } from 'electron';
import path from 'node:path';
import { IPC_CHANNELS } from '../shared/ipcTypes';
import type { CompiledVideoShader } from '../shared/video/videoGraph';

declare const MAIN_WINDOW_VITE_DEV_SERVER_URL: string;
declare const MAIN_WINDOW_VITE_NAME: string;

let videoWindow: BrowserWindow | null = null;
let latestShader: CompiledVideoShader | null = null;

function createVideoWindow(): BrowserWindow {
    const window = new BrowserWindow({
        backgroundColor: '#000000',
        height: 720,
        show: false,
        title: 'Operator Video',
        webPreferences: {
            // Output keeps rendering while the editor has focus.
            backgroundThrottling: false,
            preload: path.join(__dirname, 'preload.js'),
        },
        width: 1280,
    });

    if (MAIN_WINDOW_VITE_DEV_SERVER_URL) {
        void window.loadURL(MAIN_WINDOW_VITE_DEV_SERVER_URL + '#video');
    } else {
        void window.loadFile(
            path.join(
                __dirname,
                `../renderer/${MAIN_WINDOW_VITE_NAME}/index.html`,
            ),
            { hash: 'video' },
        );
    }

    // Shown without focus so the editor keeps the keyboard.
    window.once('ready-to-show', () => window.showInactive());
    window.on('closed', () => {
        videoWindow = null;
    });
    return window;
}

/**
 * Records the patch's video shader and delivers it to the output window,
 * opening the window when a patch first has video output.
 */
export function updateVideoShader(shader: CompiledVideoShader | null): void {
    latestShader = shader;
    if (shader !== null && videoWindow === null) {
        videoWindow = createVideoWindow();
    }
    videoWindow?.webContents.send(IPC_CHANNELS.VIDEO_ON_SHADER, shader);
}

export function getVideoShader(): CompiledVideoShader | null {
    return latestShader;
}
