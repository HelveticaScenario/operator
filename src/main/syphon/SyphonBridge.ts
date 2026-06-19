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

export class SyphonBridge {
    private child: ChildProcess | null = null;
    private status: SyphonStatus = 'stopped';
    private restartTimer: ReturnType<typeof setTimeout> | null = null;
    private intentionalStop = false;
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
            setTimeout(() => {
                if (!child.killed) child.kill('SIGKILL');
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
        if (this.restartTimer) clearTimeout(this.restartTimer);
        this.child?.kill('SIGTERM');
        this.child = null;
    }

    private spawn(window: BrowserWindow, windowId: string): void {
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
            console.error(
                `[syphon-bridge] failed to launch (${bin}):`,
                err.message,
            );
            this.child = null;
            this.setStatus('error');
        });

        child.on('exit', (code, signal) => {
            this.child = null;
            console.log(
                `[syphon-bridge] exited code=${code ?? '-'} signal=${signal ?? '-'}`,
            );
            if (this.intentionalStop) {
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
            // Otherwise treat as a crash and restart while the feature is on.
            this.setStatus('error');
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
