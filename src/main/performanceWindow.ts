import {
    screen,
    type BrowserWindow,
    type MenuItemConstructorOptions,
} from 'electron';
import { PERFORMANCE_WINDOW_NAME } from '../shared/video/performanceWindowName';

export type PerformanceAspect = 'free' | '16:9' | '4:3' | '1:1' | '9:16';

/** Width over height of each fixed aspect; `free` has none. */
const RATIOS: Record<PerformanceAspect, number> = {
    '1:1': 1,
    '16:9': 16 / 9,
    '4:3': 4 / 3,
    '9:16': 9 / 16,
    free: 0,
};

/** The width and height that give `size` the ratio of `aspect`, keeping its width. */
export function sizeForAspect(
    size: { height: number; width: number },
    aspect: PerformanceAspect,
): { height: number; width: number } {
    const ratio = RATIOS[aspect];
    return ratio === 0
        ? size
        : { height: Math.round(size.width / ratio), width: size.width };
}

let window: BrowserWindow | null = null;
let aspect: PerformanceAspect = 'free';
/** Where to put the window in fullscreen once the editor has opened it. */
let pendingFullscreenDisplay: number | null = null;
/** The window's bounds before it went fullscreen, to return to. */
let restoreBounds: Electron.Rectangle | null = null;

/** The performance window, if it is open. */
export function getPerformanceWindow(): BrowserWindow | null {
    return window !== null && !window.isDestroyed() ? window : null;
}

const isFullscreen = (win: BrowserWindow): boolean =>
    process.platform === 'darwin'
        ? win.isSimpleFullScreen()
        : win.isFullScreen();

function applyAspect(win: BrowserWindow): void {
    if (isFullscreen(win)) return;
    const ratio = RATIOS[aspect];
    win.setAspectRatio(ratio);
    if (ratio !== 0) {
        const [width, height] = win.getContentSize();
        const fitted = sizeForAspect({ height, width }, aspect);
        win.setContentSize(fitted.width, fitted.height);
    }
}

/** Sets the shape of the performance window, now and for windows opened later. */
export function setPerformanceAspect(next: PerformanceAspect): void {
    aspect = next;
    const win = getPerformanceWindow();
    if (win !== null) applyAspect(win);
}

function enterFullscreen(win: BrowserWindow, displayId: number): void {
    const display =
        screen.getAllDisplays().find((d) => d.id === displayId) ??
        screen.getPrimaryDisplay();
    if (!isFullscreen(win)) restoreBounds = win.getBounds();
    win.setAspectRatio(0);
    win.setBounds(display.bounds);
    if (process.platform === 'darwin') win.setSimpleFullScreen(true);
    else win.setFullScreen(true);
}

/** Leaves fullscreen and returns the window to where it was. */
export function exitPerformanceFullscreen(): void {
    pendingFullscreenDisplay = null;
    const win = getPerformanceWindow();
    if (win === null || !isFullscreen(win)) return;
    if (process.platform === 'darwin') win.setSimpleFullScreen(false);
    else win.setFullScreen(false);
    if (restoreBounds !== null) win.setBounds(restoreBounds);
    restoreBounds = null;
    applyAspect(win);
}

/**
 * Puts the performance window fullscreen on display `displayId`. When the
 * window is not open, `open` asks the editor to open it and the window goes
 * fullscreen as soon as it exists.
 */
export function fullscreenPerformanceWindow(
    displayId: number,
    open: () => void,
): void {
    const win = getPerformanceWindow();
    if (win === null) {
        pendingFullscreenDisplay = displayId;
        open();
        return;
    }
    enterFullscreen(win, displayId);
}

/**
 * Follows the performance window the editor opens with `window.open`, which
 * main sees as a created window of the editor.
 */
export function trackPerformanceWindow(
    editor: BrowserWindow,
    onClosed: () => void,
): void {
    editor.webContents.on('did-create-window', (created, details) => {
        if (details.frameName !== PERFORMANCE_WINDOW_NAME) return;
        window = created;
        created.on('closed', () => {
            window = null;
            restoreBounds = null;
            onClosed();
        });
        if (pendingFullscreenDisplay !== null) {
            enterFullscreen(created, pendingFullscreenDisplay);
            pendingFullscreenDisplay = null;
        } else {
            applyAspect(created);
        }
    });
}

/** View-menu entries that put the performance window fullscreen on each display. */
export function performanceFullscreenMenu(
    open: () => void,
): MenuItemConstructorOptions[] {
    const displays = screen.getAllDisplays();
    return [
        ...displays.map((display, index) => ({
            click: () => fullscreenPerformanceWindow(display.id, open),
            label: `Fullscreen on ${index + 1}: ${display.label || 'Display'} (${display.size.width}×${display.size.height})`,
        })),
        { type: 'separator' as const },
        { click: exitPerformanceFullscreen, label: 'Exit Fullscreen' },
    ];
}
