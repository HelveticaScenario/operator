import { spawn, type ChildProcess } from 'node:child_process';
import path from 'node:path';
import { app, type BrowserWindow } from 'electron';

/**
 * Manages the headless `operator-syphon` helper that captures the Operator
 * window via ScreenCaptureKit and republishes it as a Syphon source.
 *
 * macOS only. The helper is a separate signed executable spawned as a plain
 * child of this process, so macOS attributes the Screen Recording permission to
 * the responsible process (Operator) — one prompt, one "Operator" entry.
 */

export type SyphonStatus =
    | 'stopped'
    | 'starting'
    | 'ready'
    | 'permission_required'
    | 'error';

/** Syphon source name advertised to clients (Resolume, VDMX, MadMapper, …). */
const SERVER_NAME = 'Operator';
const DEFAULT_FPS = 60;
const RESTART_DELAY_MS = 1500;
/**
 * Circuit breaker for automatic crash-restarts: allow at most
 * MAX_RESTARTS_PER_WINDOW within any RESTART_WINDOW_MS rolling window. This
 * bounds both a helper that can never start (its window never becomes shareable)
 * and one that flaps — reaches `ready`, crashes immediately, repeat — neither of
 * which should restart forever. A genuinely healthy helper that crashes only
 * occasionally always has budget, since old restarts age out of the window. The
 * window is cleared on a user-initiated start.
 */
const MAX_RESTARTS_PER_WINDOW = 5;
const RESTART_WINDOW_MS = 60_000;

export class SyphonBridge {
    private child: ChildProcess | null = null;
    private status: SyphonStatus = 'stopped';
    private restartTimer: ReturnType<typeof setTimeout> | null = null;
    private intentionalStop = false;
    /** Timestamps (ms) of recent automatic restarts, for the rolling-window cap. */
    private restartTimes: number[] = [];
    private readonly fps: number;
    private readonly onStatusChange: (status: SyphonStatus) => void;

    constructor(opts: {
        onStatusChange: (status: SyphonStatus) => void;
        fps?: number;
    }) {
        this.onStatusChange = opts.onStatusChange;
        this.fps = opts.fps ?? DEFAULT_FPS;
    }

    get currentStatus(): SyphonStatus {
        return this.status;
    }

    get isActive(): boolean {
        return this.child !== null;
    }

    static get supported(): boolean {
        return process.platform === 'darwin';
    }

    /** Path to the bundled helper binary, dev vs packaged. */
    private helperPath(): string {
        if (app.isPackaged) {
            // Contents/MacOS/operator-syphon, alongside the Electron binary;
            // Syphon.framework sits at Contents/Frameworks (resolved via @rpath).
            return path.join(
                path.dirname(app.getPath('exe')),
                'operator-syphon',
            );
        }
        // Dev: the dist/ mirror produced by scripts/build-syphon-bridge.mjs.
        // __dirname is <root>/.vite/build in the forge+vite dev build.
        return path.join(
            __dirname,
            '..',
            '..',
            'native',
            'syphon-bridge',
            'dist',
            'MacOS',
            'operator-syphon',
        );
    }

    /** Read the target window's CGWindowID from its media source id. */
    private cgWindowId(window: BrowserWindow): string | null {
        if (window.isDestroyed()) return null;
        // "window:<CGWindowID>:<n>" — the middle field is the CGWindowID on macOS.
        // It is -1 until the window has been shown on screen.
        const id = window.getMediaSourceId().split(':')[1];
        return id && id !== '-1' ? id : null;
    }

    /** Begin publishing `window`. No-op if already active. */
    start(window: BrowserWindow): { ok: boolean; reason?: string } {
        if (!SyphonBridge.supported) {
            return { ok: false, reason: 'Syphon output is macOS only.' };
        }
        if (this.child) return { ok: true };

        const windowId = this.cgWindowId(window);
        if (!windowId) {
            return {
                ok: false,
                reason: 'The window is not ready to be captured yet.',
            };
        }

        this.intentionalStop = false;
        // Fresh user action — clear the crash-restart history.
        this.restartTimes = [];
        this.spawn(window, windowId);
        return { ok: true };
    }

    /** Stop publishing. */
    stop(): void {
        this.intentionalStop = true;
        if (this.restartTimer) {
            clearTimeout(this.restartTimer);
            this.restartTimer = null;
        }
        const child = this.child;
        if (child) {
            child.kill('SIGTERM');
            // `child.killed` only reflects that a signal was *sent*, not that the
            // process exited, so it can't gate the force-kill. The exit handler
            // nulls `this.child`; if it's still our current child after the grace
            // period it hasn't exited — force it.
            setTimeout(() => {
                if (this.child === child) child.kill('SIGKILL');
            }, 1000);
        }
        this.setStatus('stopped');
    }

    toggle(window: BrowserWindow): { ok: boolean; reason?: string } {
        if (this.isActive) {
            this.stop();
            return { ok: true };
        }
        return this.start(window);
    }

    /**
     * Stop the helper on app quit. SIGTERM (not SIGKILL) so it gracefully ends
     * the SCStream and retires the Syphon server before exiting; the helper's own
     * 1s backstop + getppid watchdog guarantee it terminates even if we don't wait.
     */
    dispose(): void {
        this.intentionalStop = true;
        if (this.restartTimer) {
            clearTimeout(this.restartTimer);
            this.restartTimer = null;
        }
        this.child?.kill('SIGTERM');
        this.child = null;
    }

    private spawn(window: BrowserWindow, windowId: string): void {
        // A user (re)start can race a pending crash-restart; cancel it so we
        // don't end up with two helpers publishing the same source.
        if (this.restartTimer) {
            clearTimeout(this.restartTimer);
            this.restartTimer = null;
        }
        const bin = this.helperPath();
        this.setStatus('starting');

        const child = spawn(bin, [windowId, SERVER_NAME, String(this.fps)], {
            stdio: ['ignore', 'pipe', 'pipe'],
        });
        this.child = child;

        child.stdout?.on('data', (buf: Buffer) => {
            for (const line of buf.toString().split('\n')) {
                const match = line.match(/^STATUS=(\w+)/);
                if (match) this.handleStatusToken(match[1]);
            }
        });
        child.stderr?.on('data', (buf: Buffer) => {
            console.log('[syphon-bridge]', buf.toString().trimEnd());
        });

        child.on('error', (err) => {
            // Ignore a late event from a child we've already replaced or detached.
            if (this.child !== child) return;
            console.error(
                `[syphon-bridge] failed to launch (${bin}):`,
                err.message,
            );
            this.child = null;
            this.setStatus('error');
        });

        child.on('exit', (code, signal) => {
            // Ignore a late exit from a child we've already replaced or detached;
            // keeps the single-live-child invariant explicit.
            if (this.child !== child) return;
            this.child = null;
            console.log(
                `[syphon-bridge] exited code=${code ?? '-'} signal=${signal ?? '-'}`,
            );
            if (this.intentionalStop) {
                this.setStatus('stopped');
                return;
            }
            // Clean self-exit (code 0): the helper tore itself down on its own —
            // e.g. the captured window closed. Not a crash, so don't restart.
            if (code === 0) {
                this.setStatus('stopped');
                return;
            }
            // Exit code 2 = Screen Recording permission needed; the helper has
            // already triggered the prompt. Don't auto-restart (it would loop on
            // the prompt) — wait for the user to grant and re-enable.
            if (code === 2) {
                this.setStatus('permission_required');
                return;
            }
            // Otherwise treat it as a crash and restart while the feature is on,
            // but only within the rolling restart budget so a helper that can
            // never start — or that flaps — doesn't loop forever.
            this.setStatus('error');
            const now = Date.now();
            this.restartTimes = this.restartTimes.filter(
                (t) => now - t < RESTART_WINDOW_MS,
            );
            if (this.restartTimes.length >= MAX_RESTARTS_PER_WINDOW) {
                console.error(
                    `[syphon-bridge] giving up after ${this.restartTimes.length} restarts within ${RESTART_WINDOW_MS / 1000}s`,
                );
                return;
            }
            this.restartTimes.push(now);
            this.restartTimer = setTimeout(() => {
                this.restartTimer = null;
                if (this.intentionalStop) return;
                const nextId = this.cgWindowId(window);
                if (nextId) this.spawn(window, nextId);
            }, RESTART_DELAY_MS);
        });
    }

    private handleStatusToken(token: string): void {
        switch (token) {
            case 'ready':
                this.setStatus('ready');
                break;
            case 'permission_required':
                this.setStatus('permission_required');
                break;
            // 'stopped' / 'error' are driven by the exit handler.
        }
    }

    private setStatus(status: SyphonStatus): void {
        if (status === this.status) return;
        this.status = status;
        this.onStatusChange(status);
    }
}
