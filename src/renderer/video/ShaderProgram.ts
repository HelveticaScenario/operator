/// <reference types="@webgpu/types" />
import type { CompiledVideoShader } from '../../shared/video/videoGraph';
import { UNIFORM_SLOTS_OFFSET } from '../../shared/video/uniformLayout';
import { FEEDBACK_FORMAT, type FeedbackBuffers } from './FeedbackBuffers';
import { PREVIEW_FORMAT } from './PreviewCapture';

type ParityGroups = [GPUBindGroup, GPUBindGroup];

/**
 * A compiled shader's GPU objects: the output pipeline, one pipeline per
 * preview, and the uniform buffers each pass reads. The previews have their
 * own uniforms so they can run at their own resolution while sharing the
 * clock, control values and feedback textures of the output pass.
 */
export class ShaderProgram {
    readonly bufferCount: number;
    readonly previewCount: number;
    readonly uniforms: Float32Array<ArrayBuffer>;
    readonly previewUniforms: Float32Array<ArrayBuffer>;
    private groups: { main: ParityGroups; preview: ParityGroups } | null = null;
    private groupsGeneration = -1;

    private constructor(
        compiled: CompiledVideoShader,
        readonly pipeline: GPURenderPipeline,
        readonly previewPipelines: GPURenderPipeline[],
        private readonly layout: GPUBindGroupLayout,
        readonly uniformBuffer: GPUBuffer,
        readonly previewUniformBuffer: GPUBuffer,
    ) {
        this.bufferCount = compiled.feedbackBufferCount;
        this.previewCount = compiled.previewCount;
        this.uniforms = new Float32Array(compiled.uniformFloatCount);
        this.previewUniforms = new Float32Array(compiled.uniformFloatCount);
        for (const { slot, value } of compiled.uniforms) {
            this.uniforms[UNIFORM_SLOTS_OFFSET + slot] = value;
        }
    }

    /**
     * Compiles `compiled`. Rejects with the WGSL compiler's messages when the
     * shader is invalid.
     */
    static async build(
        device: GPUDevice,
        canvasFormat: GPUTextureFormat,
        compiled: CompiledVideoShader,
    ): Promise<ShaderProgram> {
        const module = device.createShaderModule({ code: compiled.wgsl });
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
        const layout = device.createBindGroupLayout({
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
        const pipelineLayout = device.createPipelineLayout({
            bindGroupLayouts: [layout],
        });
        const vertex = { entryPoint: 'vs', module };

        const [pipeline, ...previewPipelines] = await Promise.all([
            device.createRenderPipelineAsync({
                fragment: {
                    entryPoint: 'fs',
                    module,
                    targets: [
                        { format: canvasFormat },
                        ...Array.from({ length: bufferCount }, () => ({
                            format: FEEDBACK_FORMAT,
                        })),
                    ],
                },
                layout: pipelineLayout,
                vertex,
            }),
            ...Array.from({ length: compiled.previewCount }, (_, k) =>
                device.createRenderPipelineAsync({
                    fragment: {
                        entryPoint: `preview_${k}`,
                        module,
                        targets: [{ format: PREVIEW_FORMAT }],
                    },
                    layout: pipelineLayout,
                    vertex,
                }),
            ),
        ]);

        const uniformBuffer = () =>
            device.createBuffer({
                size: compiled.uniformFloatCount * 4,
                usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
            });
        return new ShaderProgram(
            compiled,
            pipeline,
            previewPipelines,
            layout,
            uniformBuffer(),
            uniformBuffer(),
        );
    }

    /** Sets a control-bound input of this shader. */
    setSlot(slot: number, value: number): void {
        const index = UNIFORM_SLOTS_OFFSET + slot;
        if (index < this.uniforms.length) this.uniforms[index] = value;
    }

    /**
     * Bind groups reading each ping-pong parity's feedback textures, rebuilt
     * whenever the textures change.
     */
    bindGroups(
        device: GPUDevice,
        buffers: FeedbackBuffers,
        sampler: GPUSampler,
    ): { main: ParityGroups; preview: ParityGroups } {
        if (
            this.groups === null ||
            this.groupsGeneration !== buffers.generation
        ) {
            const build = (uniform: GPUBuffer, parity: number) =>
                device.createBindGroup({
                    entries: [
                        { binding: 0, resource: { buffer: uniform } },
                        ...(this.bufferCount === 0
                            ? []
                            : [{ binding: 1, resource: sampler }]),
                        ...Array.from({ length: this.bufferCount }, (_, k) => ({
                            binding: 2 + k,
                            resource: buffers.readView(k, parity),
                        })),
                    ],
                    layout: this.layout,
                });
            this.groups = {
                main: [
                    build(this.uniformBuffer, 0),
                    build(this.uniformBuffer, 1),
                ],
                preview: [
                    build(this.previewUniformBuffer, 0),
                    build(this.previewUniformBuffer, 1),
                ],
            };
            this.groupsGeneration = buffers.generation;
        }
        return this.groups;
    }

    destroy(): void {
        this.uniformBuffer.destroy();
        this.previewUniformBuffer.destroy();
    }
}
