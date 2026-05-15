/// <reference types="vite/client" />

/**
 * Global type declarations for Electron IPC API
 */

import type { ElectronAPI } from '../preload/preload';

interface TestAPI {
    getEditorValue: () => string;
    setEditorValue: (code: string) => void;
    executePatch: () => Promise<void>;
    getLastPatchResult: () => any;
    getScopeData: () => Promise<any>;
    getAudioHealth: () => Promise<any>;
    isClockRunning: () => boolean;
    openEngineHealth: () => void;
}

declare global {
    interface Window {
        electronAPI: ElectronAPI;
        __TEST_API__?: TestAPI;
    }
}

export {};
