/// <reference types="@webgpu/types" />
import { BEAM_WGSL, FRAME_WGSL } from './scopeShaders';

/**
 * Eight bits of light per pixel. Each pass of the beam saturates at full
 * brightness, which sets how wide a bright trace looks.
 */
export const SCOPE_FORMAT: GPUTextureFormat = 'r8unorm';

/** The layouts, pipelines and sampler every XY scope draws with. */
export interface ScopePipelines {
    beamLayout: GPUBindGroupLayout;
    filterLayout: GPUBindGroupLayout;
    compositeLayout: GPUBindGroupLayout;
    /** Adds the beam into a trace. */
    line: GPURenderPipeline;
    /** Subtracts a floor from a trace, then `fade` scales it by the blend constant. */
    fadeFloor: GPURenderPipeline;
    fade: GPURenderPipeline;
    copy: GPURenderPipeline;
    blur: GPURenderPipeline;
    composite: GPURenderPipeline;
    sampler: GPUSampler;
}

export function createScopePipelines(device: GPUDevice): ScopePipelines {
    const beam = device.createShaderModule({ code: BEAM_WGSL });
    const frame = device.createShaderModule({ code: FRAME_WGSL });
    const fragment = GPUShaderStage.FRAGMENT;
    const texture = (binding: number): GPUBindGroupLayoutEntry => ({
        binding,
        texture: { sampleType: 'float' },
        visibility: fragment,
    });
    const samplerEntry: GPUBindGroupLayoutEntry = {
        binding: 0,
        sampler: { type: 'filtering' },
        visibility: fragment,
    };
    const beamLayout = device.createBindGroupLayout({
        entries: [
            {
                binding: 0,
                buffer: { type: 'uniform' },
                visibility: GPUShaderStage.VERTEX | fragment,
            },
            {
                binding: 1,
                texture: { sampleType: 'unfilterable-float' },
                visibility: GPUShaderStage.VERTEX,
            },
        ],
    });
    const filterLayout = device.createBindGroupLayout({
        entries: [
            samplerEntry,
            texture(1),
            {
                binding: 2,
                buffer: { type: 'uniform' },
                visibility: fragment,
            },
        ],
    });
    const compositeLayout = device.createBindGroupLayout({
        entries: [samplerEntry, texture(1), texture(3), texture(4)],
    });

    const add: GPUBlendComponent = { dstFactor: 'one', srcFactor: 'one' };
    const pipeline = (
        module: GPUShaderModule,
        vertex: string,
        fragmentEntry: string,
        layouts: GPUBindGroupLayout[],
        blend?: GPUBlendState,
    ) =>
        device.createRenderPipeline({
            fragment: {
                entryPoint: fragmentEntry,
                module,
                targets: [{ blend, format: SCOPE_FORMAT }],
            },
            layout: device.createPipelineLayout({
                bindGroupLayouts: layouts,
            }),
            vertex: { entryPoint: vertex, module },
        });
    const line = pipeline(beam, 'line_vs', 'line_fs', [beamLayout], {
        alpha: add,
        color: add,
    });
    const subtract: GPUBlendComponent = {
        ...add,
        operation: 'reverse-subtract',
    };
    const fadeFloor = pipeline(frame, 'quad_vs', 'fade_floor_fs', [], {
        alpha: subtract,
        color: subtract,
    });
    const scale: GPUBlendComponent = {
        dstFactor: 'one-minus-constant',
        srcFactor: 'zero',
    };
    const fade = pipeline(frame, 'quad_vs', 'fade_fs', [], {
        alpha: scale,
        color: scale,
    });
    const copy = pipeline(frame, 'quad_vs', 'copy_fs', [filterLayout]);
    const blur = pipeline(frame, 'quad_vs', 'blur_fs', [filterLayout]);
    const composite = pipeline(frame, 'quad_vs', 'composite_fs', [
        compositeLayout,
    ]);

    const sampler = device.createSampler({
        addressModeU: 'clamp-to-edge',
        addressModeV: 'clamp-to-edge',
        magFilter: 'linear',
        minFilter: 'linear',
    });
    return {
        beamLayout,
        blur,
        composite,
        compositeLayout,
        copy,
        fade,
        fadeFloor,
        filterLayout,
        line,
        sampler,
    };
}
