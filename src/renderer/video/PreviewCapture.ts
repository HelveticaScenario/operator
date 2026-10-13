/// <reference types="@webgpu/types" />
import type { VideoPreviewFrame } from '../../shared/video/videoGraph';
import { UNIFORM_RESOLUTION_OFFSET } from '../../shared/video/uniformLayout';
import type { ShaderProgram } from './ShaderProgram';

export const PREVIEW_FORMAT: GPUTextureFormat = 'rgba8unorm';

/** WebGPU requires each texture row in a buffer copy to start on this boundary. */
const ROW_ALIGNMENT = 256;

/** Previews are drawn this many pixels tall, at the output's aspect ratio. */
const PREVIEW_HEIGHT = 144;
const PREVIEW_MIN_WIDTH = 64;
const PREVIEW_MAX_WIDTH = 512;

interface Target {
    texture: GPUTexture;
    view: GPUTextureView;
    staging: GPUBuffer;
    /** True from the copy until its pixels have been delivered. */
    busy: boolean;
}

/**
 * Small render targets, one per preview, and the staging buffers that bring
 * their pixels back to the CPU. A target still being read back is skipped, so
 * a slow readback drops frames instead of queueing them.
 */
export class PreviewCapture {
    private targets: Target[] = [];
    private width = 0;
    private height = 0;
    private bytesPerRow = 0;

    constructor(
        private readonly device: GPUDevice,
        private readonly onFrame: (frame: VideoPreviewFrame) => void,
    ) {}

    /** Sizes the set to `count` targets of `width` x `height` pixels. */
    resize(count: number, width: number, height: number): void {
        if (
            count === this.targets.length &&
            width === this.width &&
            height === this.height
        ) {
            return;
        }
        this.destroy();
        this.width = width;
        this.height = height;
        this.bytesPerRow =
            Math.ceil((width * 4) / ROW_ALIGNMENT) * ROW_ALIGNMENT;
        for (let k = 0; k < count; k++) {
            const texture = this.device.createTexture({
                format: PREVIEW_FORMAT,
                size: [width, height],
                usage:
                    GPUTextureUsage.RENDER_ATTACHMENT |
                    GPUTextureUsage.COPY_SRC,
            });
            this.targets.push({
                busy: false,
                staging: this.device.createBuffer({
                    size: this.bytesPerRow * height,
                    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
                }),
                texture,
                view: texture.createView(),
            });
        }
    }

    /**
     * Draws every preview of `program` at the output's `aspect` ratio and
     * records copies of the targets not still being read back. Returns the
     * targets copied, for {@link deliver}.
     */
    draw(
        encoder: GPUCommandEncoder,
        program: ShaderProgram,
        group: GPUBindGroup,
        aspect: number,
    ): number[] {
        const width = Math.min(
            PREVIEW_MAX_WIDTH,
            Math.max(PREVIEW_MIN_WIDTH, Math.round(PREVIEW_HEIGHT * aspect)),
        );
        this.resize(program.previewCount, width, PREVIEW_HEIGHT);

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
                        view: this.targets[k].view,
                    },
                ],
            });
            pass.setPipeline(program.previewPipelines[k]);
            pass.setBindGroup(0, group);
            pass.draw(3);
            pass.end();
        }
        return this.encodeReadback(encoder);
    }

    /** Records a copy of every target that is not still being read back. */
    private encodeReadback(encoder: GPUCommandEncoder): number[] {
        const copied: number[] = [];
        this.targets.forEach((target, k) => {
            if (target.busy) return;
            target.busy = true;
            encoder.copyTextureToBuffer(
                { texture: target.texture },
                { buffer: target.staging, bytesPerRow: this.bytesPerRow },
                [this.width, this.height],
            );
            copied.push(k);
        });
        return copied;
    }

    /** Reads back the targets `draw` copied. Call after submitting. */
    deliver(indices: number[]): void {
        const { width, height, bytesPerRow } = this;
        for (const index of indices) {
            const target = this.targets[index];
            target.staging.mapAsync(GPUMapMode.READ).then(
                () => {
                    const mapped = new Uint8Array(
                        target.staging.getMappedRange(),
                    );
                    const data = new Uint8Array(width * height * 4);
                    for (let row = 0; row < height; row++) {
                        data.set(
                            mapped.subarray(
                                row * bytesPerRow,
                                row * bytesPerRow + width * 4,
                            ),
                            row * width * 4,
                        );
                    }
                    target.staging.unmap();
                    target.busy = false;
                    this.onFrame({ data, height, index, width });
                },
                () => {
                    target.busy = false;
                },
            );
        }
    }

    destroy(): void {
        for (const target of this.targets) {
            target.texture.destroy();
            target.staging.destroy();
        }
        this.targets = [];
    }
}
