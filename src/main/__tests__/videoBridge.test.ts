import { describe, expect, it, vi } from 'vitest';
import { IPC_CHANNELS } from '../../shared/ipcTypes';

const {
    pullVideo,
    setVideoControl,
    setVideoTapSource,
    setVideoTarget,
    updateVideoShader,
} = await import('../videoBridge');

const shaderReadingTap = (tap: number) =>
    ({
        histories: [],
        uniforms: [{ kind: 'tap', slot: 0, tap, value: 0 }],
    }) as never;

const window = () => ({ isDestroyed: () => false, send: vi.fn() });

describe('pullVideo', () => {
    it('reports stopped, with no samples, before an engine is attached', () => {
        expect(pullVideo()).toEqual({
            applied: 0,
            cancelled: 0,
            running: false,
            taps: [],
        });
    });

    it('reports stopped and reads nothing while the engine is stopped', () => {
        const read = vi.fn(() => ({ head: 2, samples: [0.5, 0.25] }));
        let stopped = true;
        setVideoTapSource({
            isStopped: () => stopped,
            read,
            updates: () => ({ applied: 1, cancelled: 0 }),
            sampleRate: () => 48000,
        });
        setVideoTarget(() => null);
        updateVideoShader(shaderReadingTap(0), 1);

        expect(pullVideo()).toEqual({
            applied: 1,
            cancelled: 0,
            running: false,
            taps: [],
        });
        expect(read).not.toHaveBeenCalled();

        stopped = false;
        const pulled = pullVideo();
        expect(pulled.running).toBe(true);
        expect(pulled.taps).toHaveLength(1);
        expect(pulled.taps[0].tap).toBe(0);
        expect(pulled.taps[0].sampleRate).toBe(48000);
        expect(Array.from(pulled.taps[0].samples)).toEqual([0.5, 0.25]);
    });

    it('is running with no samples when the shader reads no audio', () => {
        setVideoTapSource({
            isStopped: () => false,
            read: () => ({ head: 0, samples: [] }),
            updates: () => ({ applied: 1, cancelled: 0 }),
            sampleRate: () => 48000,
        });
        updateVideoShader({ histories: [], uniforms: [] } as never, 1);
        expect(pullVideo()).toEqual({
            applied: 1,
            cancelled: 0,
            running: true,
            taps: [],
        });
    });
});

describe('across patch updates', () => {
    it('serves the taps of the shader before the latest while the renderer may still draw it', () => {
        const read = vi.fn((_tap: number, _since?: number) => ({
            head: 1,
            samples: [0.5],
        }));
        setVideoTapSource({
            isStopped: () => false,
            read,
            sampleRate: () => 48000,
            updates: () => ({ applied: 1, cancelled: 0 }),
        });
        setVideoTarget(() => null);
        updateVideoShader(shaderReadingTap(0), 1);
        updateVideoShader(shaderReadingTap(3), 2);

        pullVideo(true);
        const taps = read.mock.calls.map(([tap]) => tap).sort((a, b) => a - b);
        expect(taps).toEqual([0, 3]);
    });

    it('reads which update the engine has applied after the samples, not before', () => {
        const order: string[] = [];
        setVideoTapSource({
            isStopped: () => false,
            read: () => {
                order.push('read');
                return { head: 1, samples: [0.5] };
            },
            sampleRate: () => 48000,
            updates: () => {
                order.push('updates');
                return { applied: 2, cancelled: 0 };
            },
        });
        setVideoTarget(() => null);
        updateVideoShader(shaderReadingTap(0), 2);
        order.length = 0;

        pullVideo(true);
        expect(order.at(-1)).toBe('updates');
        expect(order.slice(0, -1).every((step) => step === 'read')).toBe(true);
    });
});

describe('a fresh pull', () => {
    it("restarts each tap from the engine's newest samples instead of where the last pull left off", () => {
        const read = vi.fn((_tap: number, since?: number) => ({
            head: (since ?? 100) + 10,
            samples: [0.1],
        }));
        setVideoTapSource({
            isStopped: () => false,
            read,
            updates: () => ({ applied: 1, cancelled: 0 }),
            sampleRate: () => 48000,
        });
        setVideoTarget(() => null);
        updateVideoShader(shaderReadingTap(0), 1);

        pullVideo(true);
        expect(read).toHaveBeenLastCalledWith(0, undefined);
        pullVideo();
        expect(read).toHaveBeenLastCalledWith(0, 110);

        pullVideo(true);
        expect(read).toHaveBeenLastCalledWith(0, undefined);
        pullVideo();
        expect(read).toHaveBeenLastCalledWith(0, 110);
    });
});

describe('delivery to the editor window', () => {
    it('sends each new shader to the window that draws it', () => {
        const target = window();
        setVideoTarget(() => target as never);
        const shader = { histories: [], uniforms: [] } as never;
        updateVideoShader(shader, 7);
        expect(target.send).toHaveBeenCalledWith(IPC_CHANNELS.VIDEO_ON_SHADER, {
            shader,
            updateId: 7,
        });
    });

    it('sends a control value to the input bound to it, and ignores other controls', () => {
        const target = window();
        setVideoTarget(() => target as never);
        updateVideoShader(
            {
                histories: [],
                uniforms: [
                    {
                        kind: 'control',
                        moduleId: 'slider-1',
                        slot: 3,
                        value: 0,
                    },
                ],
            } as never,
            1,
        );
        target.send.mockClear();

        setVideoControl('slider-1', 0.75);
        expect(target.send).toHaveBeenCalledWith(
            IPC_CHANNELS.VIDEO_ON_UNIFORM,
            [{ slot: 3, value: 0.75 }],
        );

        target.send.mockClear();
        setVideoControl('other', 1);
        expect(target.send).not.toHaveBeenCalled();
    });

    it('does nothing while there is no window to draw in', () => {
        setVideoTarget(() => null);
        expect(() => updateVideoShader(null, 1)).not.toThrow();
    });
});
