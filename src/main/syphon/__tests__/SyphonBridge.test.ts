import { EventEmitter } from 'node:events';
import {
    afterEach,
    beforeEach,
    describe,
    expect,
    test,
    vi,
    type Mock,
} from 'vitest';
import type { BrowserWindow } from 'electron';

// Hoisted so the vi.mock factory below can reference it.
const { spawnMock } = vi.hoisted(() => ({ spawnMock: vi.fn() }));

vi.mock('node:child_process', () => ({ spawn: spawnMock }));
vi.mock('electron', () => ({
    app: {
        isPackaged: false,
        getPath: () => '/Apps/Operator.app/Contents/MacOS/Operator',
    },
}));

import { SyphonBridge, type SyphonStatus } from '../SyphonBridge';

// Keep in sync with the constants in SyphonBridge.ts.
const RESTART_DELAY_MS = 1500;
const RESTART_WINDOW_MS = 60_000;
const SIGKILL_GRACE_MS = 1000;

interface FakeChild extends EventEmitter {
    stdout: EventEmitter;
    stderr: EventEmitter;
    kill: Mock;
    killed: boolean;
}

function makeFakeChild(): FakeChild {
    const child = new EventEmitter() as FakeChild;
    child.stdout = new EventEmitter();
    child.stderr = new EventEmitter();
    child.killed = false;
    child.kill = vi.fn(function (this: FakeChild) {
        this.killed = true;
        return true;
    });
    return child;
}

function makeWindow(mediaSourceId = 'window:42:0'): BrowserWindow {
    return {
        isDestroyed: () => false,
        getMediaSourceId: () => mediaSourceId,
    } as unknown as BrowserWindow;
}

/** A window whose CGWindowID can be flipped to simulate it going away. */
function mutableWindow(initial = 'window:42:0') {
    let id = initial;
    const win = {
        isDestroyed: () => false,
        getMediaSourceId: () => id,
    } as unknown as BrowserWindow;
    return { win, setMediaSourceId: (v: string) => (id = v) };
}

function makeBridge() {
    const statuses: SyphonStatus[] = [];
    const bridge = new SyphonBridge({
        onStatusChange: (s) => statuses.push(s),
    });
    return { bridge, statuses };
}

let children: FakeChild[] = [];
const last = () => children[children.length - 1];

let platformDescriptor: PropertyDescriptor | undefined;

beforeEach(() => {
    vi.useFakeTimers();
    children = [];
    spawnMock.mockReset();
    spawnMock.mockImplementation(() => {
        const c = makeFakeChild();
        children.push(c);
        return c;
    });
    // SyphonBridge.supported / start() gate on darwin.
    platformDescriptor = Object.getOwnPropertyDescriptor(process, 'platform');
    Object.defineProperty(process, 'platform', {
        value: 'darwin',
        configurable: true,
    });
});

afterEach(() => {
    if (platformDescriptor) {
        Object.defineProperty(process, 'platform', platformDescriptor);
    }
    vi.useRealTimers();
});

describe('SyphonBridge', () => {
    test('start spawns the helper with parsed window id, name, fps', () => {
        const { bridge, statuses } = makeBridge();
        const res = bridge.start(makeWindow('window:42:0'));

        expect(res).toEqual({ ok: true });
        expect(spawnMock).toHaveBeenCalledTimes(1);
        const [, args] = spawnMock.mock.calls[0];
        expect(args).toEqual(['42', 'Operator', '60']);
        expect(statuses).toContain('starting');
        expect(bridge.isActive).toBe(true);
    });

    test('start is a no-op while already active', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        const res = bridge.start(makeWindow());
        expect(res).toEqual({ ok: true });
        expect(spawnMock).toHaveBeenCalledTimes(1);
    });

    test('start refuses on non-darwin platforms', () => {
        Object.defineProperty(process, 'platform', {
            value: 'linux',
            configurable: true,
        });
        const { bridge } = makeBridge();
        const res = bridge.start(makeWindow());
        expect(res.ok).toBe(false);
        expect(spawnMock).not.toHaveBeenCalled();
    });

    test('start refuses when the window has no CGWindowID yet', () => {
        const { bridge } = makeBridge();
        const res = bridge.start(makeWindow('window:-1:0'));
        expect(res.ok).toBe(false);
        expect(spawnMock).not.toHaveBeenCalled();
    });

    test('STATUS=ready transitions to ready', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        last().stdout.emit('data', Buffer.from('STATUS=ready\n'));
        expect(bridge.currentStatus).toBe('ready');
    });

    test('STATUS=permission_required transitions to permission_required', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        last().stdout.emit('data', Buffer.from('STATUS=permission_required\n'));
        expect(bridge.currentStatus).toBe('permission_required');
    });

    test('multiple STATUS lines in one stdout chunk are all processed', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        last().stdout.emit(
            'data',
            Buffer.from('STATUS=permission_required\nSTATUS=ready\n'),
        );
        expect(bridge.currentStatus).toBe('ready');
    });

    test('exit code 2 marks permission_required and does not restart', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        last().emit('exit', 2, null);
        expect(bridge.currentStatus).toBe('permission_required');

        vi.advanceTimersByTime(10_000);
        expect(spawnMock).toHaveBeenCalledTimes(1);
    });

    test('clean exit (code 0) stops without restarting', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        last().emit('exit', 0, null);
        expect(bridge.currentStatus).toBe('stopped');

        vi.advanceTimersByTime(10_000);
        expect(spawnMock).toHaveBeenCalledTimes(1);
    });

    test('a launch error marks error and does not restart', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        last().emit('error', new Error('ENOENT'));
        expect(bridge.currentStatus).toBe('error');
        expect(bridge.isActive).toBe(false);

        vi.advanceTimersByTime(10_000);
        expect(spawnMock).toHaveBeenCalledTimes(1);
    });

    test('a crash restarts after the delay', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        last().emit('exit', 1, null);
        expect(bridge.currentStatus).toBe('error');
        expect(spawnMock).toHaveBeenCalledTimes(1);

        vi.advanceTimersByTime(RESTART_DELAY_MS);
        expect(spawnMock).toHaveBeenCalledTimes(2);
    });

    test('a restart aborts if the window is no longer capturable', () => {
        const { bridge } = makeBridge();
        const w = mutableWindow();
        bridge.start(w.win); // spawn 1
        w.setMediaSourceId('window:-1:0'); // window goes away
        last().emit('exit', 1, null); // crash → restart scheduled

        vi.advanceTimersByTime(RESTART_DELAY_MS);
        expect(spawnMock).toHaveBeenCalledTimes(1); // no id → no respawn
    });

    test('rapid crashes are capped within the window, then it gives up', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());

        // Crash repeatedly with no time to age out of the window.
        for (let i = 0; i < 8; i++) {
            last().emit('exit', 1, null);
            vi.advanceTimersByTime(RESTART_DELAY_MS);
        }

        // Initial spawn + MAX_RESTARTS_PER_WINDOW (5) restarts = 6 total.
        expect(spawnMock).toHaveBeenCalledTimes(6);
        expect(bridge.currentStatus).toBe('error');
        expect(bridge.isActive).toBe(false);
    });

    test('restart budget recovers as old restarts age out of the window', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow()); // spawn 1

        // Crash 8 times, each spaced > the window apart, so every prior restart
        // has aged out and the budget never fills. A plain consecutive-crash
        // counter would give up after 5; the rolling window keeps restarting.
        for (let i = 0; i < 8; i++) {
            last().emit('exit', 1, null);
            vi.advanceTimersByTime(RESTART_WINDOW_MS + RESTART_DELAY_MS + 500);
        }

        expect(spawnMock).toHaveBeenCalledTimes(9); // 1 + 8 restarts, never capped
        expect(bridge.isActive).toBe(true);
    });

    test('a user restart cancels a pending crash-restart (no double-spawn)', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow()); // spawn 1
        last().emit('exit', 1, null); // crash → restart scheduled
        expect(bridge.isActive).toBe(false);

        bridge.start(makeWindow()); // user re-enable → spawn 2, cancels the pending timer
        expect(spawnMock).toHaveBeenCalledTimes(2);

        vi.advanceTimersByTime(RESTART_DELAY_MS); // the stale restart must not fire
        expect(spawnMock).toHaveBeenCalledTimes(2);
    });

    test('stop force-kills if the helper has not exited within the grace period', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        const child = last();

        bridge.stop();
        expect(child.kill).toHaveBeenCalledWith('SIGTERM');
        expect(bridge.currentStatus).toBe('stopped');

        vi.advanceTimersByTime(SIGKILL_GRACE_MS);
        expect(child.kill).toHaveBeenCalledWith('SIGKILL');
    });

    test('stop does not force-kill once the helper has already exited', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        const child = last();

        bridge.stop(); // SIGTERM
        child.emit('exit', null, 'SIGTERM'); // helper exits → this.child cleared

        vi.advanceTimersByTime(SIGKILL_GRACE_MS);
        expect(child.kill).toHaveBeenCalledTimes(1); // only the SIGTERM
        expect(child.kill).not.toHaveBeenCalledWith('SIGKILL');
    });

    test('toggle starts then stops', () => {
        const { bridge } = makeBridge();
        bridge.toggle(makeWindow());
        expect(bridge.isActive).toBe(true);

        const child = last();
        bridge.toggle(makeWindow());
        expect(child.kill).toHaveBeenCalledWith('SIGTERM');
        expect(bridge.currentStatus).toBe('stopped');
    });

    test('dispose signals SIGTERM and detaches the child', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow());
        const child = last();

        bridge.dispose();
        expect(child.kill).toHaveBeenCalledWith('SIGTERM');
        expect(bridge.isActive).toBe(false);
    });

    test('dispose cancels a pending crash-restart', () => {
        const { bridge } = makeBridge();
        bridge.start(makeWindow()); // spawn 1
        last().emit('exit', 1, null); // crash → restart scheduled

        bridge.dispose();
        vi.advanceTimersByTime(RESTART_DELAY_MS);
        expect(spawnMock).toHaveBeenCalledTimes(1); // the pending restart was cancelled
    });
});
