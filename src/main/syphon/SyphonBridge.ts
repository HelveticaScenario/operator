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
    /**
     * User intent: true from a user start() until the matching stop()/dispose()
     * or until the helper gives up on its own (clean exit, permission denial, or
     * exhausting the restart budget). This is the source of truth for the toggle
     * and the menu checkbox — it stays steady across the transient windows where
     * `child` is momentarily null (crash backoff) or momentarily lingering (the
     * post-stop SIGTERM grace), which process presence alone cannot represent.
     */
    private enabled = false;
    /** A start() that raced a still-draining child queues the respawn here. */
    private pendingStart: { window: BrowserWindow } | null = null;
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

    /** Whether the feature is turned on (user intent), independent of the helper
     *  process's transient presence during crash backoff or stop teardown. */
    get isEnabled(): boolean {
        return this.enabled;
    }

    static get supported(): boolean {
        if (process.platform !== 'darwin') return false;
        // ScreenCaptureKit's desktop-independent window capture and the helper's
        // deployment target both require macOS 14+; below that the helper can't
        // launch, so gate the whole feature rather than crash-loop the helper.
        return SyphonBridge.macOSMajor() >= 14;
    }

    /** Reason the feature is unavailable, for surfacing in the UI. */
    static get unsupportedReason(): string {
        if (process.platform !== 'darwin')
            return 'Syphon output is macOS only.';
        return 'Syphon output requires macOS 14 or later.';
    }

    /**
     * macOS major version. Returns Infinity when it can't be determined (e.g. the
     * non-Electron unit-test runtime, where `process.getSystemVersion` is absent),
     * so the platform check alone governs there.
     */
    private static macOSMajor(): number {
        const getSystemVersion = (
            process as NodeJS.Process & { getSystemVersion?: () => string }
        ).getSystemVersion;
        if (typeof getSystemVersion !== 'function') {
            return Number.POSITIVE_INFINITY;
        }
        const major = parseInt(getSystemVersion().split('.')[0] ?? '', 10);
        return Number.isNaN(major) ? Number.POSITIVE_INFINITY : major;
    }

    /** Path to the bundled helper binary, dev vs packaged. */
    private helperPath(): string {
        if (app.isPackaged) {
            // Contents/Resources/operator-syphon (NOT Contents/MacOS — see the
            // staging hook in forge.config.ts for why a second Mach-O beside the
            // main executable breaks codesign). Syphon.framework sits at
            // Contents/Frameworks, resolved via @rpath = @executable_path/../Frameworks,
            // which points there from Resources just as it did from MacOS.
            return path.join(process.resourcesPath, 'operator-syphon');
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

    /** Begin publishing `window`. No-op if already on and running. */
    start(window: BrowserWindow): { ok: boolean; reason?: string } {
        if (!SyphonBridge.supported) {
            return { ok: false, reason: SyphonBridge.unsupportedReason };
        }

        const windowId = this.cgWindowId(window);
        if (!windowId) {
            return {
                ok: false,
                reason: 'The window is not ready to be captured yet.',
            };
        }

        const wasEnabled = this.enabled;
        this.enabled = true;

        if (this.child) {
            // Already on and running — nothing to do.
            if (wasEnabled) return { ok: true };
            // A previous child is still draining from a just-issued stop(). Don't
            // launch a second helper on the same Syphon name; queue the respawn so
            // the exit handler starts it once the old one is gone.
            this.pendingStart = { window };
            this.setStatus('starting');
            return { ok: true };
        }

        // Fresh user action — clear the crash-restart history.
        this.restartTimes = [];
        this.spawn(window, windowId);
        return { ok: true };
    }

    /** Stop publishing. */
    stop(): void {
        this.enabled = false;
        this.pendingStart = null;
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
        // Keyed on intent, not child presence, so a click during crash backoff (no
        // child) turns the feature OFF rather than re-arming the restart loop.
        if (this.enabled) {
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
        this.enabled = false;
        this.pendingStart = null;
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

        // stdout is a byte stream: a `STATUS=…` line can split across `data`
        // events, so carry the trailing partial line over to the next chunk.
        let stdoutResidual = '';
        child.stdout?.on('data', (buf: Buffer) => {
            stdoutResidual += buf.toString();
            const lines = stdoutResidual.split('\n');
            stdoutResidual = lines.pop() ?? '';
            for (const line of lines) {
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
            this.enabled = false;
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

            // A start() that raced this child's teardown queued a respawn; honor it
            // now the old helper is gone (whatever its exit reason was). Handle it
            // entirely here — never fall through to crash handling, which would
            // restart against the stale spawn-closure window instead of the queued
            // one.
            const pending = this.pendingStart;
            this.pendingStart = null;
            if (pending && this.enabled) {
                const nextId = this.cgWindowId(pending.window);
                if (nextId) {
                    this.restartTimes = [];
                    this.spawn(pending.window, nextId);
                } else {
                    // The window isn't capturable right now; settle into a clean
                    // stopped state so the user can re-enable once it is.
                    this.enabled = false;
                    this.setStatus('stopped');
                }
                return;
            }

            // User/app turned the feature off (stop/dispose).
            if (!this.enabled) {
                this.setStatus('stopped');
                return;
            }
            // Clean self-exit (code 0): the helper tore itself down on its own —
            // e.g. the captured window closed. Not a crash, so don't restart.
            if (code === 0) {
                this.enabled = false;
                this.setStatus('stopped');
                return;
            }
            // Exit code 2 = Screen Recording permission needed; the helper has
            // already triggered the prompt. Don't auto-restart (it would loop on
            // the prompt) — wait for the user to grant and re-enable.
            if (code === 2) {
                this.enabled = false;
                this.setStatus('permission_required');
                return;
            }
            // Otherwise treat it as a crash and restart while the feature is on,
            // but only within the rolling restart budget so a helper that can
            // never start — or that flaps — doesn't loop forever.
            const now = Date.now();
            this.restartTimes = this.restartTimes.filter(
                (t) => now - t < RESTART_WINDOW_MS,
            );
            if (this.restartTimes.length >= MAX_RESTARTS_PER_WINDOW) {
                console.error(
                    `[syphon-bridge] giving up after ${this.restartTimes.length} restarts within ${RESTART_WINDOW_MS / 1000}s`,
                );
                // Stopped trying — drop intent BEFORE the status fires so the
                // listener unchecks the menu and restores throttling; the next
                // click is then a fresh start rather than a stop.
                this.enabled = false;
                this.setStatus('error');
                return;
            }
            // Transient error before the scheduled restart — intent stays on.
            this.setStatus('error');
            this.restartTimes.push(now);
            this.restartTimer = setTimeout(() => {
                this.restartTimer = null;
                if (!this.enabled) return;
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
