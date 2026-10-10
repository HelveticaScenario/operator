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

type ResourceKind = 'uniform' | 'sampler' | 'buffer' | 'history' | 'source';

/** What each kind of resource looks like in the bind group layout. */
const LAYOUT_ENTRIES: Record<
    ResourceKind,
    Omit<GPUBindGroupLayoutEntry, 'binding' | 'visibility'>
> = {
    buffer: { texture: { sampleType: 'float' } },
    history: { texture: { sampleType: 'unfilterable-float' } },
    sampler: { sampler: { type: 'filtering' } },
    source: { texture: { sampleType: 'float' } },
    uniform: { buffer: { type: 'uniform' } },
};

/**
 * Every resource a shader binds, in binding order: the uniforms, the sampler
 * when anything is sampled, each feedback buffer, the audio history when
 * there is any, and each media source. `index` counts within its kind.
 */
function resourceSlots(
    bufferCount: number,
    historyCount: number,
    sourceCount: number,
): { binding: number; kind: ResourceKind; index: number }[] {
    const at = bindingSlots(bufferCount, historyCount);
    const many = (kind: ResourceKind, start: number, count: number) =>
        Array.from({ length: count }, (_, index) => ({
            binding: start + index,
            index,
            kind,
        }));
    return [
        ...many('uniform', 0, 1),
        ...many(
            'sampler',
            at.sampler,
            bufferCount > 0 || sourceCount > 0 ? 1 : 0,
        ),
        ...many('buffer', at.buffer, bufferCount),
        ...many('history', at.history, historyCount > 0 ? 1 : 0),
        ...many('source', at.source, sourceCount),
    ];
}

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
        const layout = device.createBindGroupLayout({
            entries: resourceSlots(
                bufferCount,
                compiled.histories.length,
                compiled.sources.length,
            ).map(({ binding, kind }) => ({
                binding,
                visibility: GPUShaderStage.FRAGMENT,
                ...LAYOUT_ENTRIES[kind],
            })),
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
            const slots = resourceSlots(
                this.bufferCount,
                this.histories.length,
                this.sources.length,
            );
            const build = (uniform: GPUBuffer, parity: number) => {
                const resources: Record<
                    ResourceKind,
                    (index: number) => GPUBindingResource
                > = {
                    buffer: (index) => buffers.readView(index, parity),
                    history: () => history.view,
                    sampler: () => sampler,
                    source: (index) => sources.view(index),
                    uniform: () => ({ buffer: uniform }),
                };
                return device.createBindGroup({
                    entries: slots.map(({ binding, kind, index }) => ({
                        binding,
                        resource: resources[kind](index),
                    })),
                    layout: this.layout,
                });
            };
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
