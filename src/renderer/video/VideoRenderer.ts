/// <reference types="@webgpu/types" />
import { FeedbackBuffers } from './FeedbackBuffers';
import { AudioInputs } from './AudioInputs';
import { withinTextureLimit } from './canvasSize';
import { PreviewCapture } from './PreviewCapture';
import { regionAverage } from './cvSample';
import { ScopeBeams } from './ScopeBeams';
import { ShaderProgram } from './ShaderProgram';
import { SourceTextures, type OpenStream } from './SourceTextures';
import { ShaderGate } from './shaderGate';
import type {
    VideoCvValue,
    VideoPreviewFrame,
    VideoPull,
    VideoRestart,
    VideoShaderUpdate,
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

/** Previews are drawn and read back on every Nth frame: 30 fps at 60 Hz. */
const PREVIEW_FRAME_INTERVAL = 2;

const CLEAR_COLOR: GPUColor = { a: 1, b: 0, g: 0, r: 0 };

/**
 * Draws a compiled video shader to a canvas every animation frame, and when a
 * preview sink is set, draws the shader's previews and reads them back. With
 * no shader the canvas is cleared to black.
 */
export class VideoRenderer {
    private program: ShaderProgram | null = null;
    private readonly gate = new ShaderGate<ShaderProgram>();
    /** Control values received since the latest shader was requested, by module id. */
    private pendingControls = new Map<string, number>();
    private frameHandle = 0;
    private frameIndex = 0;
    private pullSource: ((restart: VideoRestart) => Promise<VideoPull>) | null =
        null;
    /**
     * Counts shader changes, so a pull begun before one is not applied after
     * it, and a preview read back from one program is not taken for another's.
     */
    private epoch = 0;
    /**
     * The taps the next pull reads afresh: every tap at first, then those a
     * shader change moved onto another signal.
     */
    private restart: VideoRestart = 'all';
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
    private readonly audio: AudioInputs;
    private readonly sources: SourceTextures;
    private readonly scopes: ScopeBeams;
    private errorSink: ((message: string) => void) | null = null;
    private liveSink: ((hasOutput: boolean) => void) | null = null;
    private lostSink: ((message: string) => void) | null = null;
    private readonly sampler: GPUSampler;
    /** The output pass's color attachments, whose views are set each frame. */
    private readonly attachments: GPURenderPassColorAttachment[] = [];

    private constructor(
        private readonly canvas: HTMLCanvasElement,
        private readonly device: GPUDevice,
        private readonly context: GPUCanvasContext,
        private readonly format: GPUTextureFormat,
        openStream: OpenStream | undefined,
    ) {
        this.buffers = new FeedbackBuffers(device);
        this.audio = new AudioInputs(device);
        this.sources = new SourceTextures(
            device,
            (message) => this.errorSink?.(message),
            openStream,
        );
        this.scopes = new ScopeBeams(device);
        this.previews = new PreviewCapture(device, (frame, drawnBy) =>
            this.routeFrame(frame, drawnBy),
        );
        this.sampler = device.createSampler({
            addressModeU: 'clamp-to-edge',
            addressModeV: 'clamp-to-edge',
            magFilter: 'linear',
            minFilter: 'linear',
        });
        device.addEventListener('uncapturederror', (event) => {
            this.errorSink?.(event.error.message);
        });
        void device.lost.then((info) => {
            if (info.reason === 'destroyed') return;
            cancelAnimationFrame(this.frameHandle);
            this.lostSink?.(info.message);
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
     * Builds the shader of a patch update, which goes live once the engine
     * applies that update, since its taps and control slots are numbered by
     * the patch. Until then the previous shader draws, holding its last frame
     * while the build finishes after the update applies.
     *
     * Rejects with the WGSL compiler's messages when the shader is invalid; the
     * picture then clears once the update applies. Resolves to false, without
     * effect, when a newer update supersedes this one, and to true otherwise.
     */
    async setShader({ shader, updateId }: VideoShaderUpdate): Promise<boolean> {
        const token = this.gate.request(updateId);
        this.pendingControls = new Map();

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
            if (!this.gate.offer(token, null)) return false;
            this.settle(false);
            throw error;
        }
        if (!this.gate.offer(token, program)) return false;
        this.settle(false);
        return true;
    }

    /**
     * Sets the inputs bound to the control `moduleId` in the shader being
     * displayed and in the one waiting to replace it. Each finds the control
     * in its own uniform slots; a shader still being built takes the value
     * when it is ready.
     */
    setControl(moduleId: string, value: number): void {
        this.pendingControls.set(moduleId, value);
        this.program?.setControl(moduleId, value);
        this.gate.waiting?.setControl(moduleId, value);
    }

    /**
     * Where the engine's state comes from: asked once per frame for whether it
     * is running and for the audio samples it has produced since. While it is
     * stopped nothing is drawn and videos pause; when it runs again, the
     * shader's time starts over from 0 and videos from their loop start.
     */
    setPullSource(
        source: ((restart: VideoRestart) => Promise<VideoPull>) | null,
    ): void {
        this.pullSource = source;
    }

    /**
     * Receives problems loading media, such as a file that will not decode,
     * and GPU errors no other path reports.
     */
    setErrorSink(sink: ((message: string) => void) | null): void {
        this.errorSink = sink;
    }

    /** Receives, each time a shader goes live, whether it draws a picture. */
    setLiveSink(sink: ((hasOutput: boolean) => void) | null): void {
        this.liveSink = sink;
    }

    /**
     * Receives the reason when the GPU device is lost. The renderer stops
     * drawing for good; a new one must be created to draw again.
     */
    setLostSink(sink: ((message: string) => void) | null): void {
        this.lostSink = sink;
    }

    /** Receives every editor preview frame; with no sinks, previews are not drawn. */
    setPreviewSink(sink: ((frame: VideoPreviewFrame) => void) | null): void {
        this.previewSink = sink;
    }

    /** Receives the region averages that feed audio control signals. */
    setCvSink(sink: ((values: VideoCvValue[]) => void) | null): void {
        this.cvSink = sink;
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

    dispose(): void {
        cancelAnimationFrame(this.frameHandle);
        this.gate.dispose();
        this.clear();
        this.previews.destroy();
        this.buffers.destroy();
        this.audio.destroy();
        this.sources.destroy();
        this.scopes.destroy();
        this.device.destroy();
    }

    /** Makes the gate's shader live if it releases one; returns whether it did. */
    private settle(force: boolean): boolean {
        const released = this.gate.settle(force);
        if (released !== undefined) this.activate(released.program);
        return released !== undefined;
    }

    private activate(program: ShaderProgram | null): void {
        const forgotten = this.audio.reset(
            program?.histories ?? [],
            program?.tapStamps ?? [],
        );
        this.restart = program?.tapStamps === undefined ? 'all' : forgotten;
        this.epoch++;
        this.liveSink?.(program?.hasOutput ?? false);
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
        this.sources.sync(program.sources);
        this.scopes.sync(program.scopes);
        for (const [moduleId, value] of this.pendingControls) {
            program.setControl(moduleId, value);
        }
    }

    private releaseProgram(): void {
        this.program?.destroy();
        this.program = null;
    }

    /** Drops the shader and everything that only it used. */
    private clear(): void {
        this.releaseProgram();
        this.audio.reset([]);
        this.sources.sync([]);
        this.scopes.sync([]);
        this.previews.resize(0, 0, 0);
    }

    /**
     * Sends a frame the live program drew to the CV sink or, for an editor
     * preview, the preview sink. A frame an earlier program drew is dropped:
     * its index means another preview now. While the gate holds, the engine
     * runs the next patch, so a region average is dropped rather than written
     * to that patch's signal of the same id.
     */
    private routeFrame(frame: VideoPreviewFrame, drawnBy: number): void {
        if (drawnBy !== this.epoch) return;
        const sample = this.program?.cvSamples.get(frame.index);
        if (sample === undefined) {
            this.previewSink?.(frame);
        } else if (!this.gate.holding) {
            this.cvSink?.([
                { id: sample.id, value: regionAverage(frame, sample) },
            ]);
        }
    }

    /** The canvas, then `bufferCount` feedback buffers, each cleared before drawing. */
    private colorAttachments(
        bufferCount: number,
    ): GPURenderPassColorAttachment[] {
        const { attachments } = this;
        attachments.length = Math.min(attachments.length, 1 + bufferCount);
        for (let k = 0; k <= bufferCount; k++) {
            const view =
                k === 0
                    ? this.context.getCurrentTexture().createView()
                    : this.buffers.writeView(k - 1);
            if (k < attachments.length) {
                attachments[k].view = view;
            } else {
                attachments.push({
                    clearValue: CLEAR_COLOR,
                    loadOp: 'clear',
                    storeOp: 'store',
                    view,
                });
            }
        }
        return attachments;
    }

    private setRunning(running: boolean): void {
        if (running === this.running) return;
        this.running = running;
        if (running) {
            this.timeOriginMs = performance.now();
            this.sources.restart();
            // A new start draws only what is played from now on.
            this.audio.clear();
            this.scopes.clearTrails();
        } else {
            this.sources.pause();
        }
    }

    private fitCanvas(): void {
        const scale = window.devicePixelRatio;
        const { width, height } = withinTextureLimit(
            this.fixedSize?.width ??
                Math.max(1, Math.round(this.canvas.clientWidth * scale)),
            this.fixedSize?.height ??
                Math.max(1, Math.round(this.canvas.clientHeight * scale)),
            this.device.limits.maxTextureDimension2D,
        );
        if (this.canvas.width !== width) this.canvas.width = width;
        if (this.canvas.height !== height) this.canvas.height = height;
    }

    /**
     * Asks the engine for its state and new audio samples, unless a request
     * is already waiting.
     */
    private pull(): void {
        if (this.pullSource === null || this.pulling) return;
        this.pulling = true;
        const { epoch, restart } = this;
        this.pullSource(restart)
            .then(({ applied, cancelled, running, taps }) => {
                // These samples may span the moment the engine changed
                // patch, so only taps carrying the same signal through the
                // change keep them; the rest are read afresh.
                const carried = () =>
                    taps.filter(({ tap }) => this.carries(tap));
                if (epoch !== this.epoch) {
                    this.audio.push(carried());
                    return;
                }
                if (this.restart === restart) this.restart = [];
                const restarted = this.gate.report(applied, cancelled);
                this.setRunning(running);
                if (this.settle(restarted)) {
                    this.audio.push(carried());
                    return;
                }
                this.audio.push(taps);
            })
            .catch(() => undefined)
            .finally(() => {
                this.pulling = false;
            });
    }

    /** Whether tap `tap` carries on unbroken, needing no fresh read. */
    private carries(tap: number): boolean {
        return this.restart !== 'all' && !this.restart.includes(tap);
    }

    private frame = (): void => {
        this.frameHandle = requestAnimationFrame(this.frame);
        // Pulled even with nothing to draw, so the engine's starts and stops
        // keep the shader's clock in step with it.
        this.pull();
        if (this.program === null && this.cleared && !this.gate.swapping) {
            return;
        }
        if (this.gate.holding) return;
        if (!this.running && this.program !== null) return;
        // A frame drawn before an audio input has any samples would read it as
        // zero; the previous frame stays up until every input has data.
        if (this.program !== null && !this.audio.ready(this.program)) return;
        this.fitCanvas();

        const program = this.program;
        if (program !== null) {
            this.buffers.resize(
                program.bufferCount,
                this.canvas.width,
                this.canvas.height,
            );
            this.scopes.resize(this.canvas.width, this.canvas.height);
        }

        const encoder = this.device.createCommandEncoder();
        const now = performance.now();
        if (program !== null) {
            this.audio.update(program, now);
            this.scopes.draw(
                encoder,
                this.audio.history,
                program.uniforms,
                (row) => this.audio.filled(row),
            );
        }
        const pass = encoder.beginRenderPass({
            colorAttachments: this.colorAttachments(program?.bufferCount ?? 0),
        });

        let groups: ReturnType<ShaderProgram['bindGroups']> | null = null;
        if (program !== null) {
            const { uniformBuffer, uniforms } = program;
            this.sources.update();
            uniforms[UNIFORM_TIME_OFFSET] = (now - this.timeOriginMs) / 1000;
            uniforms[UNIFORM_RESOLUTION_OFFSET] = this.canvas.width;
            uniforms[UNIFORM_RESOLUTION_OFFSET + 1] = this.canvas.height;
            this.device.queue.writeBuffer(uniformBuffer, 0, uniforms);
            groups = program.bindGroups(
                this.device,
                this.buffers,
                this.audio.history,
                this.sources,
                this.scopes,
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
            copied = this.previews.draw(
                encoder,
                program,
                groups.preview[this.buffers.currentParity],
                this.canvas.width / this.canvas.height,
            );
        }

        this.device.queue.submit([encoder.finish()]);
        this.cleared = program === null;
        this.frameSink?.(this.canvas);
        this.previews.deliver(copied, this.epoch);
        this.buffers.flip();
        this.frameIndex++;
    };
}
