/// <reference types="@webgpu/types" />
import { VIDEO_HISTORY_LEN } from '../../shared/video/videoGraph';

/**
 * One row of audio-rate samples per `fromAudio` signal, as a float texture the
 * shader reads with `textureLoad`. Rows are rewritten every frame.
 */
export class HistoryTexture {
    private texture: GPUTexture | null = null;
    private rows = 0;
    /** Changes whenever the texture is created or destroyed. */
    generation = 0;
    private readonly placeholder: GPUTexture;

    constructor(private readonly device: GPUDevice) {
        this.placeholder = device.createTexture({
            format: 'r32float',
            size: [1, 1],
            usage: GPUTextureUsage.TEXTURE_BINDING,
        });
    }

    /** Sizes the texture to `rows` rows; a size change clears it. */
    resize(rows: number): void {
        if (rows === this.rows) return;
        this.texture?.destroy();
        this.rows = rows;
        this.texture =
            rows === 0
                ? null
                : this.device.createTexture({
                      format: 'r32float',
                      size: [VIDEO_HISTORY_LEN, rows],
                      usage:
                          GPUTextureUsage.TEXTURE_BINDING |
                          GPUTextureUsage.COPY_DST,
                  });
        this.generation++;
    }

    /** A view of the samples, or of a one-texel stand-in when there are no rows. */
    get view(): GPUTextureView {
        return (this.texture ?? this.placeholder).createView();
    }

    /** Replaces the first `samples.length` samples of row `row`. */
    write(row: number, samples: Float32Array<ArrayBuffer>): void {
        if (this.texture === null || row >= this.rows) return;
        this.device.queue.writeTexture(
            { origin: [0, row], texture: this.texture },
            samples,
            { bytesPerRow: samples.byteLength },
            [samples.length, 1],
        );
    }

    destroy(): void {
        this.texture?.destroy();
        this.placeholder.destroy();
        this.texture = null;
        this.rows = 0;
    }
}
