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
    getVuMeterData: () => Promise<any>;
    getVuOutputs: () => any[];
    getAudioHealth: () => Promise<any>;
    isClockRunning: () => boolean;
    newUntitledFile: () => void;
    openEngineHealth: () => void;
    openModuleProfile: () => void;
    setVuPanelVisible: (visible: boolean) => void;
    toggleVuMute: (key: string, codeOnly?: boolean) => void;
    toggleVuSolo: (key: string, codeOnly?: boolean) => void;
}

declare global {
    interface Window {
        electronAPI: ElectronAPI;
        __TEST_API__?: TestAPI;
    }
}

export {};
