/// <reference types="@webgpu/types" />
import { FeedbackBuffers } from './FeedbackBuffers';
import { HistoryTexture } from './HistoryTexture';
import { PreviewCapture } from './PreviewCapture';
import { regionAverage } from './cvSample';
import { ShaderProgram } from './ShaderProgram';
import { SourceTextures, type OpenStream } from './SourceTextures';
import { gateVerdict } from './shaderGate';
import { TapStream } from './TapStream';
import { alignWindow } from './alignWindow';
import type {
    VideoCvValue,
    VideoPreviewFrame,
    VideoPull,
    VideoShaderUpdate,
    VideoTapSamples,
} from '../../shared/video/videoGraph';
import { MAX_FEEDBACK_BUFFERS } from '../../shared/video/videoGraph';
import {
    UNIFORM_RESOLUTION_OFFSET,
    UNIFORM_TIME_OFFSET,
} from '../../shared/video/uniformLayout';

/**
 * Bytes per pixel the shader writes when every frame buffer is in use: each
 * is a half-float color of 8 bytes, and the canvas takes 8 more once its
 * format is padded. The default limit is 32, which holds only three buffers.
 */
const COLOR_BYTES_PER_SAMPLE = 8 + MAX_FEEDBACK_BUFFERS * 8;

/** Previews are drawn this many pixels tall, at the output's aspect ratio. */
const PREVIEW_HEIGHT = 144;
const PREVIEW_MIN_WIDTH = 64;
const PREVIEW_MAX_WIDTH = 512;
/** Previews are drawn and read back on every Nth frame: 30 fps at 60 Hz. */
const PREVIEW_FRAME_INTERVAL = 2;

/**
 * Draws a compiled video shader to a canvas every animation frame, and when a
 * preview sink is set, draws the shader's previews and reads them back. With
 * no shader the canvas is cleared to black.
 */
export class VideoRenderer {
    private program: ShaderProgram | null = null;
    private shaderToken = 0;
    /** A built shader waiting for the engine to apply its patch update. */
    private pending: {
        program: ShaderProgram | null;
        updateId: number;
    } | null = null;
    /** The update id of the latest shader requested and not yet live. */
    private awaiting: number | null = null;
    /** The latest update ids the engine reported applying and discarding. */
    private applied = 0;
    private cancelled = 0;
    /** Control values received while a shader is still being built. */
    private pendingSlots = new Map<number, number>();
    private frameHandle = 0;
    private frameIndex = 0;
    private pullSource: ((fresh: boolean) => Promise<VideoPull>) | null = null;
    /** Counts shader changes, so a pull begun before one is not applied after it. */
    private epoch = 0;
    /** True until the first pull after a shader change, which restarts every tap. */
    private freshPull = true;
    /** False while the engine is stopped, when the picture holds still. */
    private running = true;
    /** When the shader's time was 0: the engine's latest start. */
    private timeOriginMs = performance.now();
    /** True while a request for audio samples is waiting on the engine. */
    private pulling = false;
    /** A canvas size to hold instead of following the element's size. */
    private fixedSize: { height: number; width: number } | null = null;
    private frameSink: ((canvas: HTMLCanvasElement) => void) | null = null;
    /** True once an empty frame has been drawn, after which nothing is drawn until a shader arrives. */
    private cleared = false;
    private previewSink: ((frame: VideoPreviewFrame) => void) | null = null;
    private cvSink: ((values: VideoCvValue[]) => void) | null = null;
    private readonly buffers: FeedbackBuffers;
    private readonly previews: PreviewCapture;
    private readonly history: HistoryTexture;
    private readonly sources: SourceTextures;
    private errorSink: ((message: string) => void) | null = null;
    /** Each audio signal the shader reads, played back against this clock. */
    private readonly streams = new Map<number, TapStream>();
    /** Scratch the audio history windows are copied into, per history row. */
    private historyScratch: Float32Array[] = [];
    private readonly sampler: GPUSampler;

    private constructor(
        private readonly canvas: HTMLCanvasElement,
        private readonly device: GPUDevice,
        private readonly context: GPUCanvasContext,
        private readonly format: GPUTextureFormat,
        openStream: OpenStream | undefined,
    ) {
        this.buffers = new FeedbackBuffers(device);
        this.history = new HistoryTexture(device);
        this.sources = new SourceTextures(
            device,
            (message) => this.errorSink?.(message),
            openStream,
        );
        this.previews = new PreviewCapture(device, (frame) =>
            this.routeFrame(frame),
        );
        this.sampler = device.createSampler({
            addressModeU: 'clamp-to-edge',
            addressModeV: 'clamp-to-edge',
            magFilter: 'linear',
            minFilter: 'linear',
        });
        this.frameHandle = requestAnimationFrame(this.frame);
    }

    /**
     * Rejects with `signal.reason` if aborted before the canvas context is
     * claimed, so a superseded creation never reconfigures a canvas that a
     * newer renderer owns.
     */
    static async create(
        canvas: HTMLCanvasElement,
        signal: AbortSignal,
        openStream?: OpenStream,
    ): Promise<VideoRenderer> {
        if (!navigator.gpu) throw new Error('WebGPU is not available');
        const adapter = await navigator.gpu.requestAdapter();
        if (adapter === null) throw new Error('No WebGPU adapter found');
        const device = await adapter.requestDevice({
            requiredLimits: {
                maxColorAttachmentBytesPerSample: Math.min(
                    adapter.limits.maxColorAttachmentBytesPerSample,
                    COLOR_BYTES_PER_SAMPLE,
                ),
            },
        });
        if (signal.aborted) {
            device.destroy();
            throw signal.reason;
        }
        const context = canvas.getContext('webgpu');
        if (context === null) {
            throw new Error('Canvas does not support a WebGPU context');
        }
        const format = navigator.gpu.getPreferredCanvasFormat();
        context.configure({ alphaMode: 'opaque', device, format });
        return new VideoRenderer(canvas, device, context, format, openStream);
    }

    /**
     * Builds the shader of a patch update. It goes live once the audio engine
     * has applied that update, because the audio taps it reads are numbered by
     * the patch; until then the previous shader keeps drawing, and from the
     * moment the engine applies the update that shader holds its last frame
     * rather than read the new patch's signals. Rejects with the WGSL
     * compiler's messages when the shader is invalid, leaving the previous
     * shader on screen.
     */
    async setShader({ shader, updateId }: VideoShaderUpdate): Promise<void> {
        const token = ++this.shaderToken;
        this.pendingSlots = new Map();
        this.pending?.program?.destroy();
        this.pending = null;
        this.awaiting = updateId;

        let program: ShaderProgram | null = null;
        try {
            if (shader !== null) {
                program = await ShaderProgram.build(
                    this.device,
                    this.format,
                    shader,
                );
            }
        } catch (error) {
            if (token === this.shaderToken) this.awaiting = null;
            throw error;
        }
        if (token !== this.shaderToken) {
            program?.destroy();
            return;
        }
        this.pending = { program, updateId };
        this.settle(false);
    }

    /** True from the moment the engine applies the awaited update until its shader is live. */
    private get holding(): boolean {
        return this.awaiting !== null && this.applied >= this.awaiting;
    }

    /**
     * Makes the pending shader live if the engine has applied its update, or
     * drops it if the engine discarded the update. `force` makes it live
     * regardless, for an engine whose update numbering has started over.
     * Returns whether a shader went live.
     */
    private settle(force: boolean): boolean {
        const { pending } = this;
        if (pending === null) return false;
        const verdict = force
            ? 'activate'
            : gateVerdict(pending.updateId, this.applied, this.cancelled);
        if (verdict === 'wait') return false;
        this.pending = null;
        this.awaiting = null;
        if (verdict === 'drop') {
            pending.program?.destroy();
            return false;
        }
        this.activate(pending.program);
        return true;
    }

    private activate(program: ShaderProgram | null): void {
        // Taps are numbered afresh by every compile, so what the streams hold
        // belongs to the previous patch's signals.
        this.streams.clear();
        this.freshPull = true;
        this.epoch++;
        if (program === null) {
            this.clear();
            this.buffers.resize(0, this.canvas.width, this.canvas.height);
            return;
        }
        // Only the old program goes: media, buffers and history are reconciled
        // below, so a video that stays in the patch keeps playing.
        this.releaseProgram();
        this.program = program;
        this.buffers.resize(
            program.bufferCount,
            this.canvas.width,
            this.canvas.height,
        );
        this.history.resize(program.histories.length);
        this.sources.sync(program.sources);
        this.historyScratch = program.histories.map(
            ({ samples, trigger }) =>
                new Float32Array(
                    trigger ? Math.min(2 * samples, 4096) : samples,
                ),
        );
        for (const [slot, value] of this.pendingSlots) {
            program.setSlot(slot, value);
        }
    }

    /**
     * Sets a control-bound input of the shader being displayed, or of the one
     * being built when a replacement is in flight.
     */
    setUniform(slot: number, value: number): void {
        this.pendingSlots.set(slot, value);
        if (this.awaiting === null) this.program?.setSlot(slot, value);
        else this.pending?.program?.setSlot(slot, value);
    }

    /**
     * Where the engine's state comes from: asked once per frame for whether it
     * is running and for the audio samples it has produced since. While it is
     * stopped nothing is drawn and videos pause; when it runs again, the
     * shader's time starts over from 0 and videos from their loop start.
     */
    setPullSource(
        source: ((fresh: boolean) => Promise<VideoPull>) | null,
    ): void {
        this.pullSource = source;
    }

    private tapsReady(program: ShaderProgram): boolean {
        const taps = [
            ...program.tapSlots.map(({ tap }) => tap),
            ...program.histories.map(({ tap }) => tap),
        ];
        return taps.every((tap) => this.streams.get(tap)?.hasData === true);
    }

    private setRunning(running: boolean): void {
        if (running === this.running) return;
        this.running = running;
        if (running) {
            this.timeOriginMs = performance.now();
            this.sources.restart();
        } else {
            this.sources.pause();
        }
    }

    /** Receives problems loading media, such as a file that will not decode. */
    setErrorSink(sink: ((message: string) => void) | null): void {
        this.errorSink = sink;
    }

    /** Adds audio samples that have just arrived. */
    private pushTapSamples(chunks: VideoTapSamples[]): void {
        const now = performance.now();
        for (const { tap, samples, sampleRate } of chunks) {
            let stream = this.streams.get(tap);
            if (stream === undefined) {
                stream = new TapStream();
                this.streams.set(tap, stream);
            }
            stream.push(samples, sampleRate, now);
        }
    }

    /** Receives every editor preview frame; with no sinks, previews are not drawn. */
    setPreviewSink(sink: ((frame: VideoPreviewFrame) => void) | null): void {
        this.previewSink = sink;
    }

    /** Receives the region averages that feed audio control signals. */
    setCvSink(sink: ((values: VideoCvValue[]) => void) | null): void {
        this.cvSink = sink;
    }

    /** Sends a delivered frame to the CV sink or, for an editor preview, the preview sink. */
    private routeFrame(frame: VideoPreviewFrame): void {
        const sample = this.program?.cvSamples.get(frame.index);
        if (sample === undefined) {
            this.previewSink?.(frame);
        } else {
            this.cvSink?.([
                { id: sample.id, value: regionAverage(frame, sample) },
            ]);
        }
    }

    dispose(): void {
        cancelAnimationFrame(this.frameHandle);
        this.shaderToken++;
        this.pending?.program?.destroy();
        this.pending = null;
        this.clear();
        this.previews.destroy();
        this.buffers.destroy();
        this.history.destroy();
        this.sources.destroy();
        this.device.destroy();
    }

    private releaseProgram(): void {
        this.program?.destroy();
        this.program = null;
    }

    /** Drops the shader and everything that only it used. */
    private clear(): void {
        this.releaseProgram();
        this.history.resize(0);
        this.sources.sync([]);
        this.previews.resize(0, 0, 0);
    }

    /**
     * Holds the canvas at `size` pixels, whatever the element's size, so the
     * picture has another window's resolution; null follows the element again.
     */
    setFixedSize(size: { height: number; width: number } | null): void {
        this.fixedSize = size;
    }

    /** Receives the canvas right after each frame is drawn, to copy it elsewhere. */
    setFrameSink(sink: ((canvas: HTMLCanvasElement) => void) | null): void {
        this.frameSink = sink;
    }

    private fitCanvas(): void {
        const scale = window.devicePixelRatio;
        const width =
            this.fixedSize?.width ??
            Math.max(1, Math.round(this.canvas.clientWidth * scale));
        const height =
            this.fixedSize?.height ??
            Math.max(1, Math.round(this.canvas.clientHeight * scale));
        if (this.canvas.width !== width) this.canvas.width = width;
        if (this.canvas.height !== height) this.canvas.height = height;
    }

    /**
     * Reads each audio signal at `nowMs`: its value into the uniform slots it
     * feeds, and its recent samples into the history rows that show them.
     */
    private updateAudioInputs(program: ShaderProgram, nowMs: number): void {
        for (const stream of this.streams.values()) stream.advance(nowMs);
        for (const { slot, tap } of program.tapSlots) {
            program.setSlot(slot, this.streams.get(tap)?.value() ?? 0);
        }
        program.histories.forEach(({ tap, samples, trigger }, row) => {
            const stream = this.streams.get(tap);
            if (stream === undefined) return;
            const recent = this.historyScratch[row];
            stream.recent(recent);
            this.history.write(row, alignWindow(recent, samples, trigger));
        });
    }

    /** Draws and copies every preview; returns the targets whose pixels to read. */
    private drawPreviews(
        encoder: GPUCommandEncoder,
        program: ShaderProgram,
        group: GPUBindGroup,
    ): number[] {
        const aspect = this.canvas.width / this.canvas.height;
        const width = Math.min(
            PREVIEW_MAX_WIDTH,
            Math.max(PREVIEW_MIN_WIDTH, Math.round(PREVIEW_HEIGHT * aspect)),
        );
        this.previews.resize(program.previewCount, width, PREVIEW_HEIGHT);

        program.previewUniforms.set(program.uniforms);
        program.previewUniforms[UNIFORM_RESOLUTION_OFFSET] = width;
        program.previewUniforms[UNIFORM_RESOLUTION_OFFSET + 1] = PREVIEW_HEIGHT;
        this.device.queue.writeBuffer(
            program.previewUniformBuffer,
            0,
            program.previewUniforms,
        );

        for (let k = 0; k < program.previewCount; k++) {
            const pass = encoder.beginRenderPass({
                colorAttachments: [
                    {
                        clearValue: { a: 1, b: 0, g: 0, r: 0 },
                        loadOp: 'clear',
                        storeOp: 'store',
                        view: this.previews.view(k),
                    },
                ],
            });
            pass.setPipeline(program.previewPipelines[k]);
            pass.setBindGroup(0, group);
            pass.draw(3);
            pass.end();
        }
        return this.previews.encodeReadback(encoder);
    }

    private frame = (): void => {
        this.frameHandle = requestAnimationFrame(this.frame);
        if (this.program === null && this.cleared && this.awaiting === null) {
            return;
        }

        if (this.pullSource !== null && !this.pulling) {
            this.pulling = true;
            const { epoch } = this;
            const fresh = this.freshPull;
            this.freshPull = false;
            this.pullSource(fresh)
                .then(({ applied, cancelled, running, taps }) => {
                    if (epoch !== this.epoch) {
                        this.freshPull = true;
                        return;
                    }
                    const restarted = applied < this.applied;
                    this.applied = applied;
                    this.cancelled = cancelled;
                    this.setRunning(running);
                    // These samples may span the moment the engine changed
                    // patch, so they go to neither shader.
                    if (this.settle(restarted) || this.holding) return;
                    this.pushTapSamples(taps);
                })
                .catch(() => undefined)
                .finally(() => {
                    this.pulling = false;
                });
        }
        if (this.holding) return;
        if (!this.running && this.program !== null) return;
        // A frame drawn before an audio input has any samples would read it as
        // zero; the previous frame stays up until every input has data.
        if (this.program !== null && !this.tapsReady(this.program)) return;
        this.fitCanvas();

        const program = this.program;
        if (program !== null) {
            this.buffers.resize(
                program.bufferCount,
                this.canvas.width,
                this.canvas.height,
            );
        }

        const clear = { a: 1, b: 0, g: 0, r: 0 };
        const encoder = this.device.createCommandEncoder();
        const pass = encoder.beginRenderPass({
            colorAttachments: [
                {
                    clearValue: clear,
                    loadOp: 'clear',
                    storeOp: 'store',
                    view: this.context.getCurrentTexture().createView(),
                },
                ...Array.from(
                    { length: program?.bufferCount ?? 0 },
                    (_, k) => ({
                        clearValue: clear,
                        loadOp: 'clear' as const,
                        storeOp: 'store' as const,
                        view: this.buffers.writeView(k),
                    }),
                ),
            ],
        });

        let groups: ReturnType<ShaderProgram['bindGroups']> | null = null;
        if (program !== null) {
            const { uniformBuffer, uniforms } = program;
            const now = performance.now();
            this.updateAudioInputs(program, now);
            this.sources.update();
            uniforms[UNIFORM_TIME_OFFSET] = (now - this.timeOriginMs) / 1000;
            uniforms[UNIFORM_RESOLUTION_OFFSET] = this.canvas.width;
            uniforms[UNIFORM_RESOLUTION_OFFSET + 1] = this.canvas.height;
            this.device.queue.writeBuffer(uniformBuffer, 0, uniforms);
            groups = program.bindGroups(
                this.device,
                this.buffers,
                this.history,
                this.sources,
                this.sampler,
            );
            pass.setPipeline(program.pipeline);
            pass.setBindGroup(0, groups.main[this.buffers.currentParity]);
            pass.draw(3);
        }
        pass.end();

        let copied: number[] = [];
        if (
            program !== null &&
            groups !== null &&
            program.previewCount > 0 &&
            (this.previewSink !== null || this.cvSink !== null) &&
            this.frameIndex % PREVIEW_FRAME_INTERVAL === 0
        ) {
            copied = this.drawPreviews(
                encoder,
                program,
                groups.preview[this.buffers.currentParity],
            );
        }

        this.device.queue.submit([encoder.finish()]);
        this.cleared = program === null;
        this.frameSink?.(this.canvas);
        this.previews.deliver(copied);
        this.buffers.flip();
        this.frameIndex++;
    };
}
