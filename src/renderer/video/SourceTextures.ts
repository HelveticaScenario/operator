/// <reference types="@webgpu/types" />
import type { VideoSourceDef } from '../../shared/video/videoGraph';
import { mediaUrl } from '../../shared/video/mediaUrl';

const FORMAT: GPUTextureFormat = 'rgba8unorm';

interface Entry {
    def: VideoSourceDef;
    texture: GPUTexture | null;
    view: GPUTextureView;
    video: HTMLVideoElement | null;
    /** True when the video has a frame the texture has not been given yet. */
    fresh: boolean;
    disposed: boolean;
}

const keyOf = (def: VideoSourceDef) => `${def.kind}:${def.path}`;

/**
 * The pictures and recordings a shader samples, as GPU textures. Each is a
 * black texel until it has loaded; a video's texture is refreshed whenever the
 * video has a new frame.
 */
export class SourceTextures {
    /** Changes whenever a texture is created, replaced or destroyed. */
    generation = 0;
    private entries = new Map<string, Entry>();
    private order: Entry[] = [];
    private readonly placeholder: GPUTextureView;
    private readonly placeholderTexture: GPUTexture;

    constructor(
        private readonly device: GPUDevice,
        private readonly onError: (message: string) => void,
    ) {
        this.placeholderTexture = device.createTexture({
            format: FORMAT,
            size: [1, 1],
            usage: GPUTextureUsage.TEXTURE_BINDING,
        });
        this.placeholder = this.placeholderTexture.createView();
    }

    /** Makes the set match `defs`, keeping what is already loaded and dropping the rest. */
    sync(defs: VideoSourceDef[]): void {
        const wanted = new Set(defs.map(keyOf));
        for (const [key, entry] of this.entries) {
            if (!wanted.has(key)) {
                this.dispose(entry);
                this.entries.delete(key);
            }
        }
        this.order = defs.map((def) => {
            let entry = this.entries.get(keyOf(def));
            if (entry === undefined) {
                entry = {
                    def,
                    disposed: false,
                    fresh: false,
                    texture: null,
                    video: null,
                    view: this.placeholder,
                };
                this.entries.set(keyOf(def), entry);
                if (def.kind === 'image') void this.loadImage(entry);
                else this.loadVideo(entry);
            }
            return entry;
        });
        this.generation++;
    }

    view(index: number): GPUTextureView {
        return this.order[index]?.view ?? this.placeholder;
    }

    /** Copies the newest frame of each playing video into its texture. */
    update(): void {
        for (const entry of this.order) {
            const { video } = entry;
            if (video === null || !entry.fresh || video.videoWidth === 0) {
                continue;
            }
            entry.fresh = false;
            if (
                entry.texture === null ||
                entry.texture.width !== video.videoWidth ||
                entry.texture.height !== video.videoHeight
            ) {
                this.replaceTexture(entry, video.videoWidth, video.videoHeight);
            }
            try {
                this.device.queue.copyExternalImageToTexture(
                    { source: video },
                    { texture: entry.texture! },
                    [video.videoWidth, video.videoHeight],
                );
            } catch (error) {
                this.fail(entry, error);
            }
        }
    }

    destroy(): void {
        for (const entry of this.entries.values()) this.dispose(entry);
        this.entries.clear();
        this.order = [];
        this.placeholderTexture.destroy();
    }

    private async loadImage(entry: Entry): Promise<void> {
        try {
            const response = await fetch(mediaUrl(entry.def.path));
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            const bitmap = await createImageBitmap(await response.blob());
            if (entry.disposed) {
                bitmap.close();
                return;
            }
            this.replaceTexture(entry, bitmap.width, bitmap.height);
            this.device.queue.copyExternalImageToTexture(
                { source: bitmap },
                { texture: entry.texture! },
                [bitmap.width, bitmap.height],
            );
            bitmap.close();
        } catch (error) {
            this.fail(entry, error);
        }
    }

    private loadVideo(entry: Entry): void {
        const video = document.createElement('video');
        video.crossOrigin = 'anonymous';
        video.muted = true;
        video.loop = true;
        video.playsInline = true;
        entry.video = video;
        const onFrame = () => {
            if (entry.disposed) return;
            entry.fresh = true;
            video.requestVideoFrameCallback(onFrame);
        };
        video.addEventListener('error', () =>
            this.fail(
                entry,
                video.error?.message || 'the file could not be played',
            ),
        );
        video.src = mediaUrl(entry.def.path);
        video.requestVideoFrameCallback(onFrame);
        video.play().catch((error: unknown) => this.fail(entry, error));
    }

    private replaceTexture(entry: Entry, width: number, height: number): void {
        entry.texture?.destroy();
        entry.texture = this.device.createTexture({
            format: FORMAT,
            size: [width, height],
            usage:
                GPUTextureUsage.TEXTURE_BINDING |
                GPUTextureUsage.COPY_DST |
                GPUTextureUsage.RENDER_ATTACHMENT,
        });
        entry.view = entry.texture.createView();
        this.generation++;
    }

    private fail(entry: Entry, error: unknown): void {
        if (entry.disposed) return;
        const detail = error instanceof Error ? error.message : String(error);
        const hint =
            entry.def.kind === 'video'
                ? ' (the app plays H.264, HEVC, VP8/VP9 and AV1; ProRes and other codecs need converting)'
                : '';
        this.onError(`${entry.def.kind} "${entry.def.path}": ${detail}${hint}`);
    }

    private dispose(entry: Entry): void {
        entry.disposed = true;
        if (entry.video !== null) {
            entry.video.pause();
            entry.video.removeAttribute('src');
            entry.video.load();
        }
        entry.texture?.destroy();
    }
}
