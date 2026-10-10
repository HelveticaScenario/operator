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
        expect(pullVideo()).toEqual({ running: false, taps: [] });
    });

    it('reports stopped and reads nothing while the engine is stopped', () => {
        const read = vi.fn(() => ({ head: 2, samples: [0.5, 0.25] }));
        let stopped = true;
        setVideoTapSource({
            isStopped: () => stopped,
            read,
            sampleRate: () => 48000,
        });
        setVideoTarget(() => null);
        updateVideoShader(shaderReadingTap(0));

        expect(pullVideo()).toEqual({ running: false, taps: [] });
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
            sampleRate: () => 48000,
        });
        updateVideoShader({ histories: [], uniforms: [] } as never);
        expect(pullVideo()).toEqual({ running: true, taps: [] });
    });
});

describe('delivery to the editor window', () => {
    it('sends each new shader to the window that draws it', () => {
        const target = window();
        setVideoTarget(() => target as never);
        const shader = { histories: [], uniforms: [] } as never;
        updateVideoShader(shader);
        expect(target.send).toHaveBeenCalledWith(
            IPC_CHANNELS.VIDEO_ON_SHADER,
            shader,
        );
    });

    it('sends a control value to the input bound to it, and ignores other controls', () => {
        const target = window();
        setVideoTarget(() => target as never);
        updateVideoShader({
            histories: [],
            uniforms: [
                { kind: 'control', moduleId: 'slider-1', slot: 3, value: 0 },
            ],
        } as never);
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
        expect(() => updateVideoShader(null)).not.toThrow();
    });
});
