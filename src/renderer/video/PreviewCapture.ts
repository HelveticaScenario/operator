/// <reference types="@webgpu/types" />
import type { VideoPreviewFrame } from '../../shared/video/videoGraph';

export const PREVIEW_FORMAT: GPUTextureFormat = 'rgba8unorm';

/** WebGPU requires each texture row in a buffer copy to start on this boundary. */
const ROW_ALIGNMENT = 256;

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

    view(k: number): GPUTextureView {
        return this.targets[k].view;
    }

    /** Records a copy of every target that is not still being read back. */
    encodeReadback(encoder: GPUCommandEncoder): number[] {
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

    /** Reads back the targets `encodeReadback` copied. Call after submitting. */
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
