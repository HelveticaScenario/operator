import { describe, expect, it, vi } from 'vitest';

vi.mock('electron', () => ({
    BrowserWindow: class {
        webContents = { send: vi.fn() };
        loadURL = vi.fn(() => Promise.resolve());
        once = vi.fn();
        on = vi.fn();
    },
}));
vi.stubGlobal('MAIN_WINDOW_VITE_DEV_SERVER_URL', 'http://localhost:5173');
vi.stubGlobal('MAIN_WINDOW_VITE_NAME', 'main_window');

const { pullVideo, setVideoTapSource, updateVideoShader } =
    await import('../performanceWindow');

const shaderReadingTap = (tap: number) =>
    ({
        histories: [],
        uniforms: [{ kind: 'tap', slot: 0, tap, value: 0 }],
    }) as never;

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
