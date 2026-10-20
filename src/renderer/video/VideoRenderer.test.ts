// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
    CompiledVideoShader,
    VideoPreviewFrame,
    VideoPull,
    VideoRestart,
} from '../../shared/video/videoGraph';

const gpu = vi.hoisted(() => ({
    builds: [] as {
        resolve: (program: unknown) => void;
        reject: (error: Error) => void;
    }[],
    previewFrame: null as
        | ((frame: VideoPreviewFrame, drawnBy: number) => void)
        | null,
    delivered: [] as number[],
    /** What the audio inputs forget on the next reset. */
    forgotten: [] as number[],
    /** Each push of samples into the audio inputs, by tap. */
    pushed: [] as number[][],
    /** Audio input and scope trail clears. */
    clears: 0,
    trailClears: 0,
}));

vi.mock('./ShaderProgram', () => ({
    ShaderProgram: {
        build: () =>
            new Promise((resolve, reject) => {
                gpu.builds.push({ reject, resolve });
            }),
    },
}));
vi.mock('./SourceTextures', () => ({
    SourceTextures: class {
        sync() {}
        update() {}
        restart() {}
        pause() {}
        destroy() {}
    },
}));
vi.mock('./ScopeBeams', () => ({
    ScopeBeams: class {
        sync() {}
        resize() {}
        draw() {}
        clearTrails() {
            gpu.trailClears++;
        }
        destroy() {}
    },
}));
vi.mock('./FeedbackBuffers', () => ({
    FeedbackBuffers: class {
        currentParity = 0;
        resize() {}
        writeView() {
            return {};
        }
        flip() {}
        destroy() {}
    },
}));
vi.mock('./AudioInputs', () => ({
    AudioInputs: class {
        history = {};
        reset() {
            return gpu.forgotten;
        }
        push(chunks: { tap: number }[]) {
            gpu.pushed.push(chunks.map(({ tap }) => tap));
        }
        clear() {
            gpu.clears++;
        }
        ready() {
            return true;
        }
        update() {}
        destroy() {}
    },
}));
vi.mock('./PreviewCapture', () => ({
    PreviewCapture: class {
        constructor(
            _device: unknown,
            onFrame: (frame: VideoPreviewFrame, drawnBy: number) => void,
        ) {
            gpu.previewFrame = onFrame;
        }
        draw() {
            return [0];
        }
        deliver(indices: number[], drawnBy: number) {
            if (indices.length > 0) gpu.delivered.push(drawnBy);
        }
        resize() {}
        destroy() {}
    },
}));

const { VideoRenderer } = await import('./VideoRenderer');

/** A built shader that records the controls set on it. */
function fakeProgram(previewCount = 0) {
    return {
        bindGroups: () => ({ main: [{}, {}], preview: [{}, {}] }),
        bufferCount: 0,
        cvSamples: new Map(),
        destroy: vi.fn(),
        hasOutput: true,
        histories: [],
        pipeline: {},
        previewCount,
        setControl: vi.fn(),
        sources: [],
        scopes: [],
        tapSlots: [],
        tapStamps: undefined as number[] | undefined,
        uniformBuffer: {},
        uniforms: new Float32Array(8),
    };
}

function fakeDevice() {
    const listeners = new Map<string, (event: unknown) => void>();
    let lose: (info: { message: string; reason: string }) => void = () =>
        undefined;
    const pass = {
        draw() {},
        end() {},
        setBindGroup() {},
        setPipeline() {},
    };
    const device = {
        addEventListener: (type: string, listener: (event: unknown) => void) =>
            listeners.set(type, listener),
        createCommandEncoder: () => ({
            beginRenderPass: () => pass,
            finish: () => ({}),
        }),
        createSampler: () => ({}),
        destroy: vi.fn(),
        limits: { maxTextureDimension2D: 8192 },
        lost: new Promise((resolve) => {
            lose = resolve;
        }),
        queue: { submit() {}, writeBuffer() {} },
    };
    return {
        device,
        emit: (type: string, event: unknown) => listeners.get(type)?.(event),
        lose: (message: string) => lose({ message, reason: 'unknown' }),
    };
}

const shader = { hasOutput: true } as CompiledVideoShader;
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('VideoRenderer', () => {
    let nextFrame: (() => void) | null;
    let gpuDevice: ReturnType<typeof fakeDevice>;
    let engine: VideoPull;
    let canvas: HTMLCanvasElement;

    beforeEach(() => {
        gpu.builds = [];
        gpu.delivered = [];
        gpu.forgotten = [];
        gpu.pushed = [];
        gpu.clears = 0;
        gpu.trailClears = 0;
        nextFrame = null;
        engine = { applied: 0, cancelled: 0, running: true, taps: [] };
        gpuDevice = fakeDevice();
        vi.stubGlobal('requestAnimationFrame', (callback: () => void) => {
            nextFrame = callback;
            return 1;
        });
        vi.stubGlobal('cancelAnimationFrame', () => {
            nextFrame = null;
        });
        Object.defineProperty(navigator, 'gpu', {
            configurable: true,
            value: {
                getPreferredCanvasFormat: () => 'bgra8unorm',
                requestAdapter: async () => ({
                    limits: { maxColorAttachmentBytesPerSample: 64 },
                    requestDevice: async () => gpuDevice.device,
                }),
            },
        });
        canvas = document.createElement('canvas');
        canvas.getContext = (() => ({
            configure() {},
            getCurrentTexture: () => ({ createView: () => ({}) }),
        })) as never;
    });

    afterEach(() => {
        vi.unstubAllGlobals();
    });

    async function makeRenderer() {
        const renderer = await VideoRenderer.create(
            canvas,
            new AbortController().signal,
        );
        renderer.setPullSource(async () => ({ ...engine }));
        return renderer;
    }

    /** Draws one frame and lets its request to the engine come back. */
    async function frame() {
        nextFrame?.();
        await settle();
    }

    /** Builds `program` for `updateId` and lets the engine apply it. */
    async function goLive(
        renderer: Awaited<ReturnType<typeof makeRenderer>>,
        program: ReturnType<typeof fakeProgram>,
        updateId: number,
    ) {
        const built = renderer.setShader({ shader, updateId });
        gpu.builds.at(-1)?.resolve(program);
        await built;
        engine.applied = updateId;
        await frame();
    }

    it('resolves a build that a newer update superseded quietly, even when it fails', async () => {
        const renderer = await makeRenderer();
        const stale = renderer.setShader({ shader, updateId: 1 });
        const current = renderer.setShader({ shader, updateId: 2 });
        gpu.builds[0].reject(new Error('stale error'));
        gpu.builds[1].resolve(fakeProgram());
        await expect(stale).resolves.toBe(false);
        await expect(current).resolves.toBe(true);
    });

    it('keeps the live shader until a failed update applies, then clears the picture', async () => {
        const renderer = await makeRenderer();
        const live = vi.fn();
        renderer.setLiveSink(live);
        const first = fakeProgram();
        await goLive(renderer, first, 1);
        expect(live).toHaveBeenLastCalledWith(true);

        const failed = renderer.setShader({ shader, updateId: 2 });
        gpu.builds[1].reject(new Error('bad wgsl'));
        await expect(failed).rejects.toThrow('bad wgsl');
        await frame();
        expect(first.destroy).not.toHaveBeenCalled();

        engine.applied = 2;
        await frame();
        expect(first.destroy).toHaveBeenCalled();
        expect(live).toHaveBeenLastCalledWith(false);
    });

    it('sets a control in the live shader and in the one waiting to replace it', async () => {
        const renderer = await makeRenderer();
        const live = fakeProgram();
        await goLive(renderer, live, 1);
        const waiting = fakeProgram();
        const built = renderer.setShader({ shader, updateId: 2 });
        gpu.builds[1].resolve(waiting);
        await built;

        renderer.setControl('knob', 0.25);
        expect(live.setControl).toHaveBeenCalledWith('knob', 0.25);
        expect(waiting.setControl).toHaveBeenCalledWith('knob', 0.25);
    });

    it('gives a shader still being built the controls set meanwhile when it goes live', async () => {
        const renderer = await makeRenderer();
        await goLive(renderer, fakeProgram(), 1);
        const next = fakeProgram();
        const built = renderer.setShader({ shader, updateId: 2 });
        renderer.setControl('knob', 0.5);
        gpu.builds[1].resolve(next);
        await built;
        engine.applied = 2;
        await frame();
        expect(next.setControl).toHaveBeenCalledWith('knob', 0.5);
    });

    it('drops a preview frame that a program no longer live drew', async () => {
        const renderer = await makeRenderer();
        const sink = vi.fn();
        renderer.setPreviewSink(sink);
        await goLive(renderer, fakeProgram(1), 1);
        await frame();
        await frame();
        const drawnByFirst = gpu.delivered.at(-1);
        expect(drawnByFirst).toBeDefined();

        await goLive(renderer, fakeProgram(1), 2);
        const preview = {
            data: new Uint8Array(4),
            height: 1,
            index: 0,
            width: 1,
        };
        gpu.previewFrame?.(preview, drawnByFirst!);
        expect(sink).not.toHaveBeenCalled();

        await frame();
        await frame();
        gpu.previewFrame?.(preview, gpu.delivered.at(-1)!);
        expect(sink).toHaveBeenCalledWith(preview);
    });

    it('writes no region average while the engine runs a patch whose shader is not live', async () => {
        const renderer = await makeRenderer();
        const cv = vi.fn();
        renderer.setCvSink(cv);
        const live = fakeProgram(1);
        live.cvSamples.set(0, { id: 'cv', size: 0.5, x: 0.5, y: 0.5 });
        await goLive(renderer, live, 1);
        await frame();
        await frame();
        const drawnBy = gpu.delivered.at(-1)!;
        const region = {
            data: new Uint8Array([255, 255, 255, 255]),
            height: 1,
            index: 0,
            width: 1,
        };
        gpu.previewFrame?.(region, drawnBy);
        expect(cv).toHaveBeenCalledTimes(1);

        void renderer.setShader({ shader, updateId: 2 });
        engine.applied = 2;
        await frame();
        gpu.previewFrame?.(region, drawnBy);
        expect(cv).toHaveBeenCalledTimes(1);
    });

    describe('audio taps across a patch update', () => {
        const samples = (...taps: number[]) =>
            taps.map((tap) => ({
                sampleRate: 48_000,
                samples: new Float32Array(1),
                tap,
            }));

        it('reads every tap afresh at first, then only as asked', async () => {
            const renderer = await makeRenderer();
            const pull = vi.fn(async (_restart: VideoRestart) => ({
                ...engine,
            }));
            renderer.setPullSource(pull);
            await frame();
            await frame();
            expect(pull.mock.calls.map(([restart]) => restart)).toEqual([
                'all',
                [],
            ]);
        });

        it('reads afresh only the taps that now carry another signal', async () => {
            const renderer = await makeRenderer();
            const pull = vi.fn(async (_restart: VideoRestart) => ({
                ...engine,
            }));
            renderer.setPullSource(pull);
            await frame();
            gpu.forgotten = [3];
            await goLive(
                renderer,
                { ...fakeProgram(), tapStamps: [1, 0, 0, 5] },
                1,
            );
            await frame();
            expect(pull.mock.calls.at(-1)?.[0]).toEqual([3]);
            await frame();
            expect(pull.mock.calls.at(-1)?.[0]).toEqual([]);
        });

        it('keeps the samples that span the switch for carried taps only', async () => {
            const renderer = await makeRenderer();
            await frame();
            gpu.forgotten = [3];
            engine.taps = samples(0, 3);
            await goLive(
                renderer,
                { ...fakeProgram(), tapStamps: [1, 0, 0, 5] },
                1,
            );
            expect(gpu.pushed.at(-1)).toEqual([0]);
            await frame();
            await frame();
            expect(gpu.pushed.at(-1)).toEqual([0, 3]);
        });

        it('reads every tap afresh after a shader without stamps', async () => {
            const renderer = await makeRenderer();
            const pull = vi.fn(async (_restart: VideoRestart) => ({
                ...engine,
            }));
            renderer.setPullSource(pull);
            await frame();
            await goLive(renderer, fakeProgram(), 1);
            await frame();
            expect(pull.mock.calls.at(-1)?.[0]).toBe('all');
        });
    });

    it('starts the taps and the scope trails over when the engine plays again', async () => {
        const renderer = await makeRenderer();
        await goLive(renderer, fakeProgram(), 1);
        engine.running = false;
        await frame();
        expect(gpu.clears).toBe(0);
        engine.running = true;
        await frame();
        expect(gpu.clears).toBe(1);
        expect(gpu.trailClears).toBe(1);
    });

    it('keeps asking the engine for its state while it has nothing to draw', async () => {
        const renderer = await makeRenderer();
        const pull = vi.fn(async (_restart: VideoRestart) => ({ ...engine }));
        renderer.setPullSource(pull);
        await frame();
        await frame();
        const asked = pull.mock.calls.length;
        await frame();
        await frame();
        expect(pull.mock.calls.length).toBeGreaterThan(asked);
    });

    it("keeps the canvas within the device's largest texture, at the same aspect ratio", async () => {
        const renderer = await makeRenderer();
        renderer.setFixedSize({ height: 10_000, width: 20_000 });
        await goLive(renderer, fakeProgram(), 1);
        await frame();
        expect(canvas.width).toBe(8192);
        expect(canvas.height).toBe(4096);
    });

    it('reports a lost device and stops drawing', async () => {
        const renderer = await makeRenderer();
        const lost = vi.fn();
        renderer.setLostSink(lost);
        gpuDevice.lose('gpu process crashed');
        await settle();
        expect(lost).toHaveBeenCalledWith('gpu process crashed');
        expect(nextFrame).toBeNull();
    });

    it('reports uncaptured GPU errors through the error sink', async () => {
        const renderer = await makeRenderer();
        const errors = vi.fn();
        renderer.setErrorSink(errors);
        gpuDevice.emit('uncapturederror', {
            error: { message: 'validation failed' },
        });
        expect(errors).toHaveBeenCalledWith('validation failed');
    });
});
