/// <reference types="@webgpu/types" />
import { FeedbackBuffers } from './FeedbackBuffers';
import { PreviewCapture } from './PreviewCapture';
import { regionAverage } from './cvSample';
import { ShaderProgram } from './ShaderProgram';
import type {
    CompiledVideoShader,
    VideoCvValue,
    VideoPreviewFrame,
} from '../../shared/video/videoGraph';
import {
    UNIFORM_RESOLUTION_OFFSET,
    UNIFORM_TIME_OFFSET,
} from '../../shared/video/uniformLayout';

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
    /** Control values received while a shader is still being built. */
    private pendingSlots = new Map<number, number>();
    private frameHandle = 0;
    private frameIndex = 0;
    private previewSink: ((frame: VideoPreviewFrame) => void) | null = null;
    private cvSink: ((values: VideoCvValue[]) => void) | null = null;
    private readonly startMs = performance.now();
    private readonly buffers: FeedbackBuffers;
    private readonly previews: PreviewCapture;
    private readonly sampler: GPUSampler;

    private constructor(
        private readonly canvas: HTMLCanvasElement,
        private readonly device: GPUDevice,
        private readonly context: GPUCanvasContext,
        private readonly format: GPUTextureFormat,
    ) {
        this.buffers = new FeedbackBuffers(device);
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
    ): Promise<VideoRenderer> {
        if (!navigator.gpu) throw new Error('WebGPU is not available');
        const adapter = await navigator.gpu.requestAdapter();
        if (adapter === null) throw new Error('No WebGPU adapter found');
        const device = await adapter.requestDevice();
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
        return new VideoRenderer(canvas, device, context, format);
    }

    /**
     * Replaces the displayed shader. Rejects with the WGSL compiler's messages
     * when the shader is invalid, leaving the previous shader on screen.
     */
    async setShader(compiled: CompiledVideoShader | null): Promise<void> {
        const token = ++this.shaderToken;
        this.pendingSlots = new Map();
        if (compiled === null) {
            this.release();
            this.buffers.resize(0, this.canvas.width, this.canvas.height);
            return;
        }

        const program = await ShaderProgram.build(
            this.device,
            this.format,
            compiled,
        );
        if (token !== this.shaderToken) {
            program.destroy();
            return;
        }
        this.release();
        this.program = program;
        this.buffers.resize(
            program.bufferCount,
            this.canvas.width,
            this.canvas.height,
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
        this.program?.setSlot(slot, value);
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
        this.release();
        this.previews.destroy();
        this.buffers.destroy();
        this.device.destroy();
    }

    private release(): void {
        this.program?.destroy();
        this.program = null;
        this.previews.resize(0, 0, 0);
    }

    private fitCanvas(): void {
        const scale = window.devicePixelRatio;
        const width = Math.max(1, Math.round(this.canvas.clientWidth * scale));
        const height = Math.max(
            1,
            Math.round(this.canvas.clientHeight * scale),
        );
        if (this.canvas.width !== width) this.canvas.width = width;
        if (this.canvas.height !== height) this.canvas.height = height;
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
            uniforms[UNIFORM_TIME_OFFSET] =
                (performance.now() - this.startMs) / 1000;
            uniforms[UNIFORM_RESOLUTION_OFFSET] = this.canvas.width;
            uniforms[UNIFORM_RESOLUTION_OFFSET + 1] = this.canvas.height;
            this.device.queue.writeBuffer(uniformBuffer, 0, uniforms);
            groups = program.bindGroups(
                this.device,
                this.buffers,
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
        this.previews.deliver(copied);
        this.buffers.flip();
        this.frameIndex++;
    };
}
