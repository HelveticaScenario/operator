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

const sameSource = (a: VideoSourceDef, b: VideoSourceDef) =>
    a.kind === b.kind &&
    a.path === b.path &&
    a.speed === b.speed &&
    a.loopStart === b.loopStart &&
    a.loopEnd === b.loopEnd;

const sameFile = (a: VideoSourceDef, b: VideoSourceDef) =>
    a.kind === b.kind && a.path === b.path;

/**
 * The pictures and recordings a shader samples, as GPU textures. Each is a
 * black texel until it has loaded; a video's texture is refreshed whenever the
 * video has a new frame.
 */
export class SourceTextures {
    /** Changes whenever a texture is created, replaced or destroyed. */
    generation = 0;
    /** The live entries, indexed as the shader's `sources`. */
    private order: Entry[] = [];
    private playing = true;
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

    /**
     * Makes the set match `defs`. A file that is still wanted keeps its
     * element, and so its playback position, even when its speed or loop
     * points change; the rest are dropped.
     */
    sync(defs: VideoSourceDef[]): void {
        const unclaimed = new Set(this.order);
        const claimed: (Entry | undefined)[] = defs.map(() => undefined);
        for (const matches of [sameSource, sameFile]) {
            defs.forEach((def, i) => {
                if (claimed[i] !== undefined) return;
                for (const entry of unclaimed) {
                    if (matches(entry.def, def)) {
                        unclaimed.delete(entry);
                        claimed[i] = entry;
                        return;
                    }
                }
            });
        }
        for (const entry of unclaimed) this.dispose(entry);
        this.order = defs.map((def, i) => {
            const entry = claimed[i];
            if (entry === undefined) return this.create(def);
            if (!sameSource(entry.def, def)) {
                entry.def = def;
                this.applyPlayback(entry);
            }
            return entry;
        });
        this.generation++;
    }

    /** Plays or pauses every video; paused videos hold their position. */
    setPlaying(playing: boolean): void {
        this.playing = playing;
        for (const entry of this.order) this.syncPlayback(entry);
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
        for (const entry of this.order) this.dispose(entry);
        this.order = [];
        this.placeholderTexture.destroy();
    }

    private create(def: VideoSourceDef): Entry {
        const entry: Entry = {
            def,
            disposed: false,
            fresh: false,
            texture: null,
            video: null,
            view: this.placeholder,
        };
        if (def.kind === 'image') void this.loadImage(entry);
        else this.loadVideo(entry);
        return entry;
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
        video.playsInline = true;
        entry.video = video;
        const onFrame = () => {
            if (entry.disposed) return;
            entry.fresh = true;
            const { loopStart = 0, loopEnd } = entry.def;
            if (loopEnd !== undefined && video.currentTime >= loopEnd) {
                video.currentTime = loopStart;
            }
            video.requestVideoFrameCallback(onFrame);
        };
        // Without a loop end the file plays to its end, then returns to the loop start.
        video.addEventListener('ended', () => {
            if (entry.disposed) return;
            video.currentTime = entry.def.loopStart ?? 0;
            this.syncPlayback(entry);
        });
        video.addEventListener('error', () =>
            this.fail(
                entry,
                video.error?.message || 'the file could not be played',
            ),
        );
        // A file whose audio decodes but whose picture does not loads without
        // an error and reports no frame size.
        video.addEventListener('loadedmetadata', () => {
            if (video.videoWidth === 0) {
                this.fail(entry, 'the file has no picture the app can decode');
            }
            this.applyPlayback(entry);
        });
        video.src = mediaUrl(entry.def.path);
        video.requestVideoFrameCallback(onFrame);
        this.applyPlayback(entry);
    }

    /** Gives a video its speed and loop points, moving it into the loop if it is outside. */
    private applyPlayback(entry: Entry): void {
        const { video } = entry;
        if (video === null) return;
        const { speed = 1, loopStart = 0, loopEnd } = entry.def;
        video.playbackRate = speed;
        video.loop = loopStart === 0 && loopEnd === undefined;
        const outside =
            video.currentTime < loopStart ||
            (loopEnd !== undefined && video.currentTime >= loopEnd);
        if (outside) video.currentTime = loopStart;
        this.syncPlayback(entry);
    }

    /** Starts or pauses a video: it plays while the engine runs and its speed is above 0. */
    private syncPlayback(entry: Entry): void {
        const { video } = entry;
        if (video === null) return;
        if (this.playing && (entry.def.speed ?? 1) > 0) {
            video.play().catch((error: unknown) => {
                // A pause that lands while play() is pending cancels it.
                if (error instanceof Error && error.name === 'AbortError') {
                    return;
                }
                this.fail(entry, error);
            });
        } else {
            video.pause();
        }
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
                ? ' (the app plays H.264, HEVC, VP8/VP9 and AV1; ProRes, Motion JPEG and other codecs need converting)'
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
