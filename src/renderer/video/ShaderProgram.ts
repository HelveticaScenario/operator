/// <reference types="@webgpu/types" />
import type {
    CompiledVideoShader,
    VideoCvSample,
    VideoHistory,
    VideoSourceDef,
} from '../../shared/video/videoGraph';
import {
    UNIFORM_SLOTS_OFFSET,
    bindingSlots,
} from '../../shared/video/uniformLayout';
import { FEEDBACK_FORMAT, type FeedbackBuffers } from './FeedbackBuffers';
import type { HistoryTexture } from './HistoryTexture';
import type { SourceTextures } from './SourceTextures';
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
    /** Regions to average into audio control signals, by preview index. */
    readonly cvSamples: ReadonlyMap<number, VideoCvSample>;
    /** Uniform slots fed by an audio signal, with the engine tap that carries it. */
    readonly tapSlots: { slot: number; tap: number }[];
    /** Rows of audio history the shader reads. */
    readonly histories: VideoHistory[];
    /** Media the shader samples, one texture each. */
    readonly sources: VideoSourceDef[];
    readonly uniforms: Float32Array<ArrayBuffer>;
    readonly previewUniforms: Float32Array<ArrayBuffer>;
    private groups: { main: ParityGroups; preview: ParityGroups } | null = null;
    private groupsGeneration = '';

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
        this.histories = compiled.histories;
        this.sources = compiled.sources;
        this.tapSlots = compiled.uniforms.flatMap((u) =>
            u.kind === 'tap' ? [{ slot: u.slot, tap: u.tap }] : [],
        );
        this.cvSamples = new Map(
            compiled.cvSamples.map(({ index, ...sample }) => [index, sample]),
        );
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
        const slots = bindingSlots(bufferCount, compiled.histories.length);
        const layout = device.createBindGroupLayout({
            entries: [
                {
                    binding: 0,
                    buffer: { type: 'uniform' },
                    visibility: GPUShaderStage.FRAGMENT,
                },
                ...(bufferCount === 0 && compiled.sources.length === 0
                    ? []
                    : [
                          {
                              binding: slots.sampler,
                              sampler: { type: 'filtering' as const },
                              visibility: GPUShaderStage.FRAGMENT,
                          },
                      ]),
                ...Array.from({ length: bufferCount }, (_, k) => ({
                    binding: slots.buffer + k,
                    texture: { sampleType: 'float' as const },
                    visibility: GPUShaderStage.FRAGMENT,
                })),
                ...(compiled.histories.length === 0
                    ? []
                    : [
                          {
                              binding: slots.history,
                              texture: {
                                  sampleType: 'unfilterable-float' as const,
                              },
                              visibility: GPUShaderStage.FRAGMENT,
                          },
                      ]),
                ...compiled.sources.map((_, k) => ({
                    binding: slots.source + k,
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
        history: HistoryTexture,
        sources: SourceTextures,
        sampler: GPUSampler,
    ): { main: ParityGroups; preview: ParityGroups } {
        const generation = `${buffers.generation}:${history.generation}:${sources.generation}`;
        if (this.groups === null || this.groupsGeneration !== generation) {
            const slots = bindingSlots(this.bufferCount, this.histories.length);
            const build = (uniform: GPUBuffer, parity: number) =>
                device.createBindGroup({
                    entries: [
                        { binding: 0, resource: { buffer: uniform } },
                        ...(this.bufferCount === 0 && this.sources.length === 0
                            ? []
                            : [{ binding: slots.sampler, resource: sampler }]),
                        ...Array.from({ length: this.bufferCount }, (_, k) => ({
                            binding: slots.buffer + k,
                            resource: buffers.readView(k, parity),
                        })),
                        ...(this.histories.length === 0
                            ? []
                            : [
                                  {
                                      binding: slots.history,
                                      resource: history.view,
                                  },
                              ]),
                        ...this.sources.map((_, k) => ({
                            binding: slots.source + k,
                            resource: sources.view(k),
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
            this.groupsGeneration = generation;
        }
        return this.groups;
    }

    destroy(): void {
        this.uniformBuffer.destroy();
        this.previewUniformBuffer.destroy();
    }
}
