/// <reference types="@webgpu/types" />
import {
    MAX_SCOPE_TRACES,
    SCOPE_SETTING_RANGES,
    type VideoScope,
    type VideoScopeSetting,
} from '../../shared/video/videoGraph';
import { UNIFORM_SLOTS_OFFSET } from '../../shared/video/uniformLayout';
import type { HistoryTexture } from './HistoryTexture';
import {
    SCOPE_FORMAT,
    createScopePipelines,
    type ScopePipelines,
} from './scopePipelines';
import {
    BEAM_UNIFORM_FLOATS,
    KERNEL_OFFSET,
    MAX_SEGMENTS,
    ROWS_OFFSET,
    STARTS_OFFSET,
    UPSAMPLE_STEPS,
    lanczosKernel,
} from './scopeShaders';

/** A full-frame pass's single target. */
const target = (
    view: GPUTextureView,
    load: GPULoadOp = 'clear',
): GPURenderPassDescriptor => ({
    colorAttachments: [
        {
            clearValue: { a: 0, b: 0, g: 0, r: 0 },
            loadOp: load,
            storeOp: 'store',
            view,
        },
    ],
});

interface Layer {
    texture: GPUTexture;
    view: GPUTextureView;
}

/** One scope's textures: the persistent trace, its two glows, and the result. */
interface Targets {
    line: Layer;
    tightA: Layer;
    tightB: Layer;
    wideA: Layer;
    wideB: Layer;
    out: Layer;
    /** Bind groups of the glow and composite passes, in drawing order. */
    groups: GPUBindGroup[];
}

interface Beams {
    scope: VideoScope;
    uniforms: GPUBuffer;
    beamGroup: GPUBindGroup | null;
    targets: Targets;
    /** True until the first frame drawn into a new trace, which clears it. */
    fresh: boolean;
    /** The beam's half-width, gain and fade as last written to `uniforms`. */
    live: Float32Array<ArrayBuffer>;
    /** Each trace's first real sample, as last written to `uniforms`. */
    starts: Float32Array<ArrayBuffer>;
}

/**
 * Draws the XY scopes of a shader into frame-sized textures before the
 * picture, which `scopeXY` nodes sample. Each frame fades a scope's trace,
 * adds every trace's latest samples from the audio history, blurs the trace
 * at half and at an eighth of the frame's size for its glow, and tonemaps
 * the sum into the beam's brightness.
 */
export class ScopeBeams {
    /** Changes whenever a texture is created or destroyed. */
    generation = 0;
    private beams: Beams[] = [];
    private width = 1;
    private height = 1;
    private historyGeneration = -1;
    private readonly gpu: ScopePipelines;
    private readonly indices: GPUBuffer;
    /** Blur steps of one texel: across and up the half-size glow, then the eighth-size one. */
    private readonly offsets: GPUBuffer[];
    private readonly scratch = new ArrayBuffer(BEAM_UNIFORM_FLOATS * 4);
    private readonly placeholder: GPUTexture;

    constructor(private readonly device: GPUDevice) {
        this.gpu = createScopePipelines(device);
        this.offsets = Array.from({ length: 4 }, () =>
            device.createBuffer({
                size: 16,
                usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
            }),
        );
        this.writeOffsets();

        // Two triangles per segment over its four corners: 0 and 1 at the
        // start, 2 and 3 at the end.
        const quads = new Uint32Array(MAX_SEGMENTS * 6);
        for (let k = 0; k < MAX_SEGMENTS; k++) {
            const v = k * 4;
            quads.set([v, v + 2, v + 1, v + 1, v + 2, v + 3], k * 6);
        }
        this.indices = device.createBuffer({
            size: quads.byteLength,
            usage: GPUBufferUsage.INDEX | GPUBufferUsage.COPY_DST,
        });
        device.queue.writeBuffer(this.indices, 0, quads);

        new Float32Array(this.scratch, KERNEL_OFFSET * 4).set(lanczosKernel());
        this.placeholder = device.createTexture({
            format: SCOPE_FORMAT,
            size: [1, 1],
            usage: GPUTextureUsage.TEXTURE_BINDING,
        });
    }

    /**
     * Makes the set match `scopes`. A scope that keeps its position keeps its
     * textures, so its trail survives an edit to the patch.
     */
    sync(scopes: VideoScope[]): void {
        while (this.beams.length > scopes.length) this.drop(this.beams.pop()!);
        scopes.forEach((scope, k) => {
            const beams = this.beams[k] ?? this.create(scope);
            beams.scope = scope;
            this.beams[k] = beams;
            this.writeUniforms(beams);
        });
    }

    /** Sizes every texture to the frame; a size change clears the trails. */
    resize(width: number, height: number): void {
        if (width === this.width && height === this.height) return;
        this.width = width;
        this.height = height;
        this.writeOffsets();
        for (const beams of this.beams) {
            destroyTargets(beams.targets);
            beams.targets = this.createTargets();
            beams.fresh = true;
            this.writeUniforms(beams);
        }
        this.generation++;
    }

    /** Clears every scope's trail, from the next frame drawn. */
    clearTrails(): void {
        for (const beams of this.beams) beams.fresh = true;
    }

    /** A view of scope `index`'s brightness, or of an empty stand-in. */
    view(index: number): GPUTextureView {
        return (
            this.beams[index]?.targets.out.view ?? this.placeholder.createView()
        );
    }

    /** Draws every scope, ahead of the passes that sample them. */
    /**
     * Draws every scope, ahead of the passes that sample them. `slots` are
     * the shader's uniforms, where settings bound to a signal read its value
     * this frame, and `filled` counts the real samples at the end of each
     * history row.
     */
    draw(
        encoder: GPUCommandEncoder,
        history: HistoryTexture,
        slots: Float32Array,
        filled: (row: number) => number,
    ): void {
        if (this.beams.length === 0) return;
        if (history.generation !== this.historyGeneration) {
            this.historyGeneration = history.generation;
            for (const beams of this.beams) beams.beamGroup = null;
        }
        for (const beams of this.beams) {
            this.updateSettings(beams, slots);
            this.updateStarts(beams, filled);
            this.drawTrace(encoder, beams, history);
            const { targets } = beams;
            const [
                copyTight,
                blurTightX,
                blurTightY,
                copyWide,
                blurWideX,
                blurWideY,
                composite,
            ] = targets.groups;
            const passes: [Layer, GPURenderPipeline, GPUBindGroup][] = [
                [targets.tightA, this.gpu.copy, copyTight],
                [targets.tightB, this.gpu.blur, blurTightX],
                [targets.tightA, this.gpu.blur, blurTightY],
                [targets.wideA, this.gpu.copy, copyWide],
                [targets.wideB, this.gpu.blur, blurWideX],
                [targets.wideA, this.gpu.blur, blurWideY],
                [targets.out, this.gpu.composite, composite],
            ];
            for (const [layer, pipeline, group] of passes) {
                const pass = encoder.beginRenderPass(target(layer.view));
                pass.setPipeline(pipeline);
                pass.setBindGroup(0, group);
                pass.draw(3);
                pass.end();
            }
        }
    }

    destroy(): void {
        for (const beams of this.beams) this.drop(beams);
        this.beams = [];
        this.indices.destroy();
        this.placeholder.destroy();
        for (const buffer of this.offsets) buffer.destroy();
    }

    /** Fades a scope's trace and adds its traces' newest samples. */
    private drawTrace(
        encoder: GPUCommandEncoder,
        beams: Beams,
        history: HistoryTexture,
    ): void {
        const pass = encoder.beginRenderPass(
            target(beams.targets.line.view, beams.fresh ? 'clear' : 'load'),
        );
        beams.fresh = false;
        pass.setPipeline(this.gpu.fadeFloor);
        pass.draw(3);
        const fade = beams.live[2];
        pass.setPipeline(this.gpu.fade);
        pass.setBlendConstant({ a: fade, b: fade, g: fade, r: fade });
        pass.draw(3);
        beams.beamGroup ??= this.device.createBindGroup({
            entries: [
                { binding: 0, resource: { buffer: beams.uniforms } },
                { binding: 1, resource: history.view },
            ],
            layout: this.gpu.beamLayout,
        });
        pass.setPipeline(this.gpu.line);
        pass.setBindGroup(0, beams.beamGroup);
        pass.setIndexBuffer(this.indices, 'uint32');
        pass.drawIndexed(
            segmentsOf(beams.scope) * 6,
            beams.scope.traces.length,
        );
        pass.end();
    }

    private create(scope: VideoScope): Beams {
        this.generation++;
        return {
            beamGroup: null,
            fresh: true,
            scope,
            live: new Float32Array(3),
            starts: new Float32Array(MAX_SCOPE_TRACES),
            targets: this.createTargets(),
            uniforms: this.device.createBuffer({
                size: BEAM_UNIFORM_FLOATS * 4,
                usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
            }),
        };
    }

    private createTargets(): Targets {
        const layer = (divisor: number): Layer => {
            const texture = this.device.createTexture({
                format: SCOPE_FORMAT,
                size: [
                    Math.max(1, Math.floor(this.width / divisor)),
                    Math.max(1, Math.floor(this.height / divisor)),
                ],
                usage:
                    GPUTextureUsage.RENDER_ATTACHMENT |
                    GPUTextureUsage.TEXTURE_BINDING,
            });
            return { texture, view: texture.createView() };
        };
        const line = layer(1);
        const tightA = layer(2);
        const tightB = layer(2);
        const wideA = layer(8);
        const wideB = layer(8);
        const out = layer(1);
        const filter = (source: Layer, offset: GPUBuffer) =>
            this.device.createBindGroup({
                entries: [
                    { binding: 0, resource: this.gpu.sampler },
                    { binding: 1, resource: source.view },
                    { binding: 2, resource: { buffer: offset } },
                ],
                layout: this.gpu.filterLayout,
            });
        const [tightX, tightY, wideX, wideY] = this.offsets;
        return {
            groups: [
                filter(line, tightX),
                filter(tightA, tightX),
                filter(tightB, tightY),
                filter(tightA, wideX),
                filter(wideA, wideX),
                filter(wideB, wideY),
                this.device.createBindGroup({
                    entries: [
                        { binding: 0, resource: this.gpu.sampler },
                        { binding: 1, resource: line.view },
                        { binding: 3, resource: tightA.view },
                        { binding: 4, resource: wideA.view },
                    ],
                    layout: this.gpu.compositeLayout,
                }),
            ],
            line,
            out,
            tightA,
            tightB,
            wideA,
            wideB,
        };
    }

    private drop(beams: Beams): void {
        destroyTargets(beams.targets);
        beams.uniforms.destroy();
        this.generation++;
    }

    /** One texel across and up each glow, at the frame's current size. */
    private writeOffsets(): void {
        const texel = (divisor: number, axis: 0 | 1) => {
            const size = axis === 0 ? this.width : this.height;
            const step = 1 / Math.max(1, Math.floor(size / divisor));
            return new Float32Array(
                axis === 0 ? [step, 0, 0, 0] : [0, step, 0, 0],
            );
        };
        [texel(2, 0), texel(2, 1), texel(8, 0), texel(8, 1)].forEach(
            (data, k) =>
                this.device.queue.writeBuffer(this.offsets[k], 0, data),
        );
    }

    /** Writes the beam's half-width, gain and fade for this frame, if they changed. */
    private updateSettings(beams: Beams, slots: Float32Array): void {
        const { scope } = beams;
        const read = (
            setting: VideoScopeSetting,
            { min, max }: { min: number; max: number },
        ) => {
            const value =
                setting.kind === 'const'
                    ? setting.value
                    : (slots[UNIFORM_SLOTS_OFFSET + setting.slot] ?? 0);
            return Math.min(max, Math.max(min, value));
        };
        const r = SCOPE_SETTING_RANGES;
        const next = [
            STANDARD_HALF_WIDTH * read(scope.lineWidth, r.lineWidth),
            beamGain(read(scope.intensity, r.intensity)),
            1 - read(scope.persistence, r.persistence),
        ];
        if (next.every((v, k) => Math.fround(v) === beams.live[k])) return;
        beams.live.set(next);
        this.device.queue.writeBuffer(
            beams.uniforms,
            BEAM_LIVE_OFFSET * 4,
            beams.live,
        );
    }

    /**
     * Writes where each trace's real samples start, so nothing is drawn from
     * before its signals began.
     */
    private updateStarts(beams: Beams, filled: (row: number) => number): void {
        const { samples, traces } = beams.scope;
        let changed = false;
        traces.forEach(({ x, y }, i) => {
            const first = samples - Math.min(filled(x), filled(y));
            if (beams.starts[i] !== first) {
                beams.starts[i] = first;
                changed = true;
            }
        });
        if (changed) {
            this.device.queue.writeBuffer(
                beams.uniforms,
                STARTS_OFFSET * 4,
                beams.starts,
            );
        }
    }

    private writeUniforms(beams: Beams): void {
        const { scope, uniforms } = beams;
        const floats = new Float32Array(this.scratch);
        const [xMin, xMax] = scope.xRange;
        const [yMin, yMax] = scope.yRange;
        floats.set(
            [
                xMin,
                xMax - xMin,
                yMin,
                yMax - yMin,
                ...beams.live,
                scope.samples,
                scope.upsample ? UPSAMPLE_STEPS : 1,
                segmentsOf(scope),
                this.width / this.height,
                0,
            ],
            0,
        );
        const rows = new Uint32Array(this.scratch, ROWS_OFFSET * 4);
        scope.traces.forEach(({ x, y }, i) => {
            rows[i * 2] = x;
            rows[i * 2 + 1] = y;
        });
        new Float32Array(this.scratch, STARTS_OFFSET * 4).set(beams.starts);
        this.device.queue.writeBuffer(uniforms, 0, this.scratch);
    }
}

function destroyTargets(targets: Targets): void {
    for (const layer of [
        targets.line,
        targets.tightA,
        targets.tightB,
        targets.wideA,
        targets.wideB,
        targets.out,
    ]) {
        layer.texture.destroy();
    }
}

/**
 * The beam's gain at an intensity from 0 to 1. Cubed so the low end reaches
 * near black even though a dense trace's light piles up, and scaled so the
 * default 0.6 is a gain of 0.6.
 */
const beamGain = (intensity: number) => intensity ** 3 / 0.36;

/** The beam's half-width at `lineWidth` 1, as a fraction of half the frame height. */
const STANDARD_HALF_WIDTH = 0.012;

/** Where the beam's half-width, gain and fade start in its uniform block, in floats. */
const BEAM_LIVE_OFFSET = 4;

/** Segments in each of a scope's traces. */
function segmentsOf(scope: VideoScope): number {
    return (scope.samples - 1) * (scope.upsample ? UPSAMPLE_STEPS : 1);
}
