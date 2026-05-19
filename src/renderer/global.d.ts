/// <reference types="vite/client" />

/**
 * Global type declarations for Electron IPC API
 */

import type { ElectronAPI } from '../preload/preload';

// tinykeys ships `dist/tinykeys.d.ts` but omits `types` from its package.json
// `exports` map, so TS with `moduleResolution: bundler` refuses to resolve it.
// Re-declare the public surface we use so the package types-check without
// requiring upstream to fix their package.json.
declare module 'tinykeys' {
    export type KeyBindingPress = [mods: string[], key: string | RegExp];
    export interface KeyBindingMap {
        [keybinding: string]: (event: KeyboardEvent) => void;
    }
    export interface KeyBindingHandlerOptions {
        timeout?: number;
    }
    export interface KeyBindingOptions extends KeyBindingHandlerOptions {
        event?: 'keydown' | 'keyup';
        capture?: boolean;
    }
    export function parseKeybinding(str: string): KeyBindingPress[];
    export function matchKeyBindingPress(
        event: KeyboardEvent,
        press: KeyBindingPress,
    ): boolean;
    export function createKeybindingsHandler(
        keyBindingMap: KeyBindingMap,
        options?: KeyBindingHandlerOptions,
    ): EventListener;
    export function tinykeys(
        target: Window | HTMLElement,
        keyBindingMap: KeyBindingMap,
        options?: KeyBindingOptions,
    ): () => void;
}

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
