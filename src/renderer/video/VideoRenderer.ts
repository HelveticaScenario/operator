/// <reference types="@webgpu/types" />
import { FEEDBACK_FORMAT, FeedbackBuffers } from './FeedbackBuffers';
import type { CompiledVideoShader } from '../../shared/video/videoGraph';
import {
    UNIFORM_RESOLUTION_OFFSET,
    UNIFORM_SLOTS_OFFSET,
    UNIFORM_TIME_OFFSET,
} from '../../shared/video/uniformLayout';

interface ShaderState {
    pipeline: GPURenderPipeline;
    layout: GPUBindGroupLayout;
    uniformBuffer: GPUBuffer;
    uniforms: Float32Array<ArrayBuffer>;
    bufferCount: number;
    /** Bind groups for each ping-pong parity, built for `bindGroupGeneration`. */
    bindGroups: [GPUBindGroup, GPUBindGroup] | null;
    bindGroupGeneration: number;
}

/**
 * Draws a compiled video shader to a canvas every animation frame. With no
 * shader the canvas is cleared to black.
 */
export class VideoRenderer {
    private shader: ShaderState | null = null;
    private shaderToken = 0;
    /** Control values received while a shader is still being built. */
    private pendingSlots = new Map<number, number>();
    private frameHandle = 0;
    private readonly startMs = performance.now();
    private readonly buffers: FeedbackBuffers;
    private readonly sampler: GPUSampler;

    private constructor(
        private readonly canvas: HTMLCanvasElement,
        private readonly device: GPUDevice,
        private readonly context: GPUCanvasContext,
        private readonly format: GPUTextureFormat,
    ) {
        this.buffers = new FeedbackBuffers(device);
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

        const module = this.device.createShaderModule({ code: compiled.wgsl });
        const info = await module.getCompilationInfo();
        const errors = info.messages.filter((m) => m.type === 'error');
        if (errors.length > 0) {
            throw new Error(
                errors
                    .map((m) => `${m.lineNum}:${m.linePos} ${m.message}`)
                    .join('\n'),
            );
        }

        const bufferCount = compiled.feedbackBufferCount;
        const layout = this.device.createBindGroupLayout({
            entries: [
                {
                    binding: 0,
                    buffer: { type: 'uniform' },
                    visibility: GPUShaderStage.FRAGMENT,
                },
                ...(bufferCount === 0
                    ? []
                    : [
                          {
                              binding: 1,
                              sampler: { type: 'filtering' as const },
                              visibility: GPUShaderStage.FRAGMENT,
                          },
                      ]),
                ...Array.from({ length: bufferCount }, (_, k) => ({
                    binding: 2 + k,
                    texture: { sampleType: 'float' as const },
                    visibility: GPUShaderStage.FRAGMENT,
                })),
            ],
        });
        const pipeline = await this.device.createRenderPipelineAsync({
            fragment: {
                entryPoint: 'fs',
                module,
                targets: [
                    { format: this.format },
                    ...Array.from({ length: bufferCount }, () => ({
                        format: FEEDBACK_FORMAT,
                    })),
                ],
            },
            layout: this.device.createPipelineLayout({
                bindGroupLayouts: [layout],
            }),
            vertex: { entryPoint: 'vs', module },
        });
        const uniformBuffer = this.device.createBuffer({
            size: compiled.uniformFloatCount * 4,
            usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
        });

        if (token !== this.shaderToken) {
            uniformBuffer.destroy();
            return;
        }
        this.release();
        const uniforms = new Float32Array(compiled.uniformFloatCount);
        for (const { slot, value } of compiled.uniforms) {
            uniforms[UNIFORM_SLOTS_OFFSET + slot] = value;
        }
        this.shader = {
            bindGroupGeneration: -1,
            bindGroups: null,
            bufferCount,
            layout,
            pipeline,
            uniformBuffer,
            uniforms,
        };
        this.buffers.resize(bufferCount, this.canvas.width, this.canvas.height);
        for (const [slot, value] of this.pendingSlots) {
            this.setUniform(slot, value);
        }
        this.pendingSlots.clear();
    }

    /**
     * Sets a control-bound input of the shader being displayed, or of the one
     * being built when a replacement is in flight.
     */
    setUniform(slot: number, value: number): void {
        this.pendingSlots.set(slot, value);
        if (this.shader === null) return;
        const index = UNIFORM_SLOTS_OFFSET + slot;
        if (index < this.shader.uniforms.length) {
            this.shader.uniforms[index] = value;
        }
    }

    dispose(): void {
        cancelAnimationFrame(this.frameHandle);
        this.shaderToken++;
        this.release();
        this.buffers.destroy();
        this.device.destroy();
    }

    private release(): void {
        this.shader?.uniformBuffer.destroy();
        this.shader = null;
    }

    /** Bind groups reading each parity's textures, rebuilt when textures change. */
    private bindGroupsFor(shader: ShaderState): [GPUBindGroup, GPUBindGroup] {
        if (
            shader.bindGroups === null ||
            shader.bindGroupGeneration !== this.buffers.generation
        ) {
            const build = (parity: number) =>
                this.device.createBindGroup({
                    entries: [
                        {
                            binding: 0,
                            resource: { buffer: shader.uniformBuffer },
                        },
                        ...(shader.bufferCount === 0
                            ? []
                            : [{ binding: 1, resource: this.sampler }]),
                        ...Array.from(
                            { length: shader.bufferCount },
                            (_, k) => ({
                                binding: 2 + k,
                                resource: this.buffers.readView(k, parity),
                            }),
                        ),
                    ],
                    layout: shader.layout,
                });
            shader.bindGroups = [build(0), build(1)];
            shader.bindGroupGeneration = this.buffers.generation;
        }
        return shader.bindGroups;
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

    private frame = (): void => {
        this.frameHandle = requestAnimationFrame(this.frame);
        this.fitCanvas();

        const shader = this.shader;
        if (shader !== null) {
            this.buffers.resize(
                shader.bufferCount,
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
                ...Array.from({ length: shader?.bufferCount ?? 0 }, (_, k) => ({
                    clearValue: clear,
                    loadOp: 'clear' as const,
                    storeOp: 'store' as const,
                    view: this.buffers.writeView(k),
                })),
            ],
        });
        if (shader !== null) {
            const { pipeline, uniformBuffer, uniforms } = shader;
            uniforms[UNIFORM_TIME_OFFSET] =
                (performance.now() - this.startMs) / 1000;
            uniforms[UNIFORM_RESOLUTION_OFFSET] = this.canvas.width;
            uniforms[UNIFORM_RESOLUTION_OFFSET + 1] = this.canvas.height;
            this.device.queue.writeBuffer(uniformBuffer, 0, uniforms);
            pass.setPipeline(pipeline);
            pass.setBindGroup(
                0,
                this.bindGroupsFor(shader)[this.buffers.currentParity],
            );
            pass.draw(3);
        }
        pass.end();
        this.device.queue.submit([encoder.finish()]);
        this.buffers.flip();
    };
}
