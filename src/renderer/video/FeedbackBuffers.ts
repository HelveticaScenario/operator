/// <reference types="@webgpu/types" />

/** Half-float keeps feedback smooth where 8-bit would band and stall. */
export const FEEDBACK_FORMAT: GPUTextureFormat = 'rgba16float';

interface Pair {
    textures: [GPUTexture, GPUTexture];
    views: [GPUTextureView, GPUTextureView];
}

/**
 * Ping-pong textures holding what each feedback buffer stored on the previous
 * frame. Buffer `k` is read from `textures[parity]` and written to
 * `textures[1 - parity]`; `flip` swaps the roles after a frame is drawn.
 * Textures start black, and a resize recreates them (and so clears them).
 */
export class FeedbackBuffers {
    private pairs: Pair[] = [];
    private width = 0;
    private height = 0;
    private parity = 0;
    /** Changes whenever a texture is created or destroyed. */
    generation = 0;

    constructor(private readonly device: GPUDevice) {}

    /**
     * Sizes the set to `count` buffers of `width` x `height`. Existing
     * buffers keep their contents when the size is unchanged.
     */
    resize(count: number, width: number, height: number): void {
        const sizeChanged = width !== this.width || height !== this.height;
        if (!sizeChanged && count === this.pairs.length) return;

        const keep = sizeChanged ? 0 : Math.min(count, this.pairs.length);
        for (const pair of this.pairs.splice(keep)) {
            pair.textures.forEach((t) => t.destroy());
        }
        this.width = width;
        this.height = height;
        while (this.pairs.length < count) this.pairs.push(this.createPair());
        this.generation++;
    }

    /** View of the texture holding last frame's value of buffer `k`. */
    readView(k: number, parity = this.parity): GPUTextureView {
        return this.pairs[k].views[parity];
    }

    /** View of the texture this frame writes buffer `k` into. */
    writeView(k: number): GPUTextureView {
        return this.pairs[k].views[1 - this.parity];
    }

    get currentParity(): number {
        return this.parity;
    }

    flip(): void {
        this.parity = 1 - this.parity;
    }

    destroy(): void {
        this.resize(0, 0, 0);
    }

    private createPair(): Pair {
        const make = () =>
            this.device.createTexture({
                format: FEEDBACK_FORMAT,
                size: [this.width, this.height],
                usage:
                    GPUTextureUsage.RENDER_ATTACHMENT |
                    GPUTextureUsage.TEXTURE_BINDING,
            });
        const textures: [GPUTexture, GPUTexture] = [make(), make()];
        return {
            textures,
            views: [textures[0].createView(), textures[1].createView()],
        };
    }
}
