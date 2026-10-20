import { EventEmitter } from 'node:events';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BrowserWindow } from 'electron';
import { PERFORMANCE_WINDOW_NAME } from '../../shared/video/performanceWindowName';

const display = { bounds: { height: 1080, width: 1920, x: 0, y: 0 }, id: 7 };
vi.mock('electron', () => ({
    screen: {
        getAllDisplays: () => [display],
        getPrimaryDisplay: () => display,
    },
}));

/** A stand-in for the window the editor opens, recording fullscreen changes. */
function fakeWindow() {
    const events = new EventEmitter();
    let fullscreen = false;
    const win = {
        getBounds: () => ({ height: 720, width: 1280, x: 0, y: 0 }),
        getContentSize: () => [1280, 720],
        isDestroyed: () => false,
        isFullScreen: () => fullscreen,
        isSimpleFullScreen: () => fullscreen,
        on: (event: string, listener: () => void) => events.on(event, listener),
        setAspectRatio: vi.fn(),
        setBounds: vi.fn(),
        setContentSize: vi.fn(),
        setFullScreen: (on: boolean) => {
            fullscreen = on;
        },
        setSimpleFullScreen: (on: boolean) => {
            fullscreen = on;
        },
    };
    return { events, isFullscreen: () => fullscreen, win };
}

async function setup() {
    vi.resetModules();
    const module = await import('../performanceWindow');
    const editor = { webContents: new EventEmitter() };
    module.trackPerformanceWindow(
        editor as unknown as BrowserWindow,
        () => undefined,
    );
    const create = () => {
        const created = fakeWindow();
        editor.webContents.emit('did-create-window', created.win, {
            frameName: PERFORMANCE_WINDOW_NAME,
        });
        return created;
    };
    return { create, module };
}

describe('performance window opening', () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it('asks the editor once while an open is under way', async () => {
        const { module } = await setup();
        const toggle = vi.fn();
        module.fullscreenPerformanceWindow(display.id, toggle);
        module.fullscreenPerformanceWindow(display.id, toggle);
        module.togglePerformanceWindow(toggle);
        expect(toggle).toHaveBeenCalledTimes(1);
    });

    it('goes fullscreen when the asked-for window opens', async () => {
        const { create, module } = await setup();
        module.fullscreenPerformanceWindow(display.id, vi.fn());
        expect(create().isFullscreen()).toBe(true);
    });

    it('opens a later window plainly when the asked-for one never came', async () => {
        const { create, module } = await setup();
        const toggle = vi.fn();
        module.fullscreenPerformanceWindow(display.id, toggle);
        vi.runAllTimers();
        expect(create().isFullscreen()).toBe(false);
        module.togglePerformanceWindow(toggle);
        expect(toggle).toHaveBeenCalledTimes(2);
    });

    it('closes the window through the toggle once it is open', async () => {
        const { create, module } = await setup();
        const toggle = vi.fn();
        module.togglePerformanceWindow(toggle);
        create();
        module.togglePerformanceWindow(toggle);
        expect(toggle).toHaveBeenCalledTimes(2);
    });
});
