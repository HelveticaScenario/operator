/// <reference types="@webgpu/types" />
import type { CompiledVideoShader } from '../../shared/video/videoGraph';
import {
    UNIFORM_RESOLUTION_OFFSET,
    UNIFORM_SLOTS_OFFSET,
    UNIFORM_TIME_OFFSET,
} from '../../shared/video/uniformLayout';

interface ShaderState {
    pipeline: GPURenderPipeline;
    bindGroup: GPUBindGroup;
    uniformBuffer: GPUBuffer;
    uniforms: Float32Array<ArrayBuffer>;
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

    private constructor(
        private readonly canvas: HTMLCanvasElement,
        private readonly device: GPUDevice,
        private readonly context: GPUCanvasContext,
        private readonly format: GPUTextureFormat,
    ) {
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

        const pipeline = await this.device.createRenderPipelineAsync({
            fragment: {
                entryPoint: 'fs',
                module,
                targets: [{ format: this.format }],
            },
            layout: 'auto',
            vertex: { entryPoint: 'vs', module },
        });
        const uniformBuffer = this.device.createBuffer({
            size: compiled.uniformFloatCount * 4,
            usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
        });
        const bindGroup = this.device.createBindGroup({
            entries: [{ binding: 0, resource: { buffer: uniformBuffer } }],
            layout: pipeline.getBindGroupLayout(0),
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
        this.shader = { bindGroup, pipeline, uniformBuffer, uniforms };
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
        this.device.destroy();
    }

    private release(): void {
        this.shader?.uniformBuffer.destroy();
        this.shader = null;
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

        const encoder = this.device.createCommandEncoder();
        const pass = encoder.beginRenderPass({
            colorAttachments: [
                {
                    clearValue: { a: 1, b: 0, g: 0, r: 0 },
                    loadOp: 'clear',
                    storeOp: 'store',
                    view: this.context.getCurrentTexture().createView(),
                },
            ],
        });
        if (this.shader !== null) {
            const { bindGroup, pipeline, uniformBuffer, uniforms } =
                this.shader;
            uniforms[UNIFORM_TIME_OFFSET] =
                (performance.now() - this.startMs) / 1000;
            uniforms[UNIFORM_RESOLUTION_OFFSET] = this.canvas.width;
            uniforms[UNIFORM_RESOLUTION_OFFSET + 1] = this.canvas.height;
            this.device.queue.writeBuffer(uniformBuffer, 0, uniforms);
            pass.setPipeline(pipeline);
            pass.setBindGroup(0, bindGroup);
            pass.draw(3);
        }
        pass.end();
        this.device.queue.submit([encoder.finish()]);
    };
}
