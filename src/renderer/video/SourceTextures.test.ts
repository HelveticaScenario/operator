// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SourceTextures } from './SourceTextures';

/** A GPU device that records nothing but lets textures be made and destroyed. */
function fakeDevice() {
    const texture = () => ({
        createView: () => ({}),
        destroy: vi.fn(),
        height: 1,
        width: 1,
    });
    return { createTexture: vi.fn(texture), queue: {} } as unknown as GPUDevice;
}

describe('SourceTextures', () => {
    let videos: HTMLVideoElement[];
    let play: ReturnType<typeof vi.fn>;
    let pause: ReturnType<typeof vi.fn>;

    beforeEach(() => {
        videos = [];
        play = vi.fn(() => Promise.resolve());
        pause = vi.fn();
        const create = document.createElement.bind(document);
        vi.spyOn(document, 'createElement').mockImplementation(((
            tag: string,
        ) => {
            const element = create(tag);
            if (tag === 'video') {
                Object.assign(element, {
                    load: vi.fn(),
                    pause,
                    play,
                    requestVideoFrameCallback: vi.fn(),
                });
                videos.push(element as HTMLVideoElement);
            }
            return element;
        }) as typeof document.createElement);
        (globalThis as { GPUTextureUsage?: unknown }).GPUTextureUsage = {
            COPY_DST: 2,
            RENDER_ATTACHMENT: 16,
            TEXTURE_BINDING: 4,
        };
    });

    afterEach(() => {
        vi.restoreAllMocks();
    });

    const video = (path: string) => ({ kind: 'video' as const, path });

    it('keeps a video that is still wanted playing when the set is synced again', () => {
        const sources = new SourceTextures(fakeDevice(), vi.fn());
        sources.sync([video('clips/a.mp4')]);
        expect(videos).toHaveLength(1);
        expect(play).toHaveBeenCalledTimes(1);

        sources.sync([video('clips/a.mp4')]);
        sources.sync([video('clips/a.mp4'), video('clips/b.mp4')]);

        // The same element carries on: no new element for a.mp4, no pause.
        expect(videos).toHaveLength(2);
        expect(videos[0].getAttribute('src')).toContain('a.mp4');
        expect(pause).not.toHaveBeenCalled();
    });

    it('stops and unloads a video that is no longer wanted', () => {
        const sources = new SourceTextures(fakeDevice(), vi.fn());
        sources.sync([video('clips/a.mp4'), video('clips/b.mp4')]);
        sources.sync([video('clips/b.mp4')]);
        expect(pause).toHaveBeenCalledTimes(1);
        expect(videos[0].getAttribute('src')).toBeNull();
        expect(videos[1].getAttribute('src')).toContain('b.mp4');
    });

    it('tells a different file apart even at the same path as another kind', () => {
        const sources = new SourceTextures(fakeDevice(), vi.fn());
        sources.sync([video('x.mp4')]);
        sources.sync([video('x.mp4'), { kind: 'image', path: 'x.mp4' }]);
        expect(videos).toHaveLength(1);
    });

    it('reports a video that will not play', async () => {
        const onError = vi.fn();
        play.mockRejectedValueOnce(new Error('no supported source was found'));
        const sources = new SourceTextures(fakeDevice(), onError);
        sources.sync([video('clips/a.mov')]);
        await Promise.resolve();
        await Promise.resolve();
        expect(onError).toHaveBeenCalledWith(
            expect.stringContaining('clips/a.mov'),
        );
    });

    it('bumps its generation when the set changes so bind groups are rebuilt', () => {
        const sources = new SourceTextures(fakeDevice(), vi.fn());
        const before = sources.generation;
        sources.sync([video('a.mp4')]);
        expect(sources.generation).toBeGreaterThan(before);
    });

    it('reports a video whose picture cannot be decoded', () => {
        const onError = vi.fn();
        const sources = new SourceTextures(fakeDevice(), onError);
        sources.sync([video('clips/a.mov')]);
        videos[0].dispatchEvent(new Event('loadedmetadata'));
        expect(onError).toHaveBeenCalledWith(
            expect.stringContaining('no picture'),
        );
    });

    it('keeps a video playing when only its speed or loop points change', () => {
        const sources = new SourceTextures(fakeDevice(), vi.fn());
        sources.sync([video('clips/a.mp4')]);
        videos[0].currentTime = 1.5;
        sources.sync([
            {
                kind: 'video',
                path: 'clips/a.mp4',
                speed: 2,
                loopStart: 1,
                loopEnd: 3,
            },
        ]);
        expect(videos).toHaveLength(1);
        expect(videos[0].playbackRate).toBe(2);
        expect(videos[0].loop).toBe(false);
        expect(videos[0].currentTime).toBe(1.5);
    });

    it('moves a video that is outside its new loop into it', () => {
        const sources = new SourceTextures(fakeDevice(), vi.fn());
        sources.sync([video('clips/a.mp4')]);
        videos[0].currentTime = 5;
        sources.sync([
            { kind: 'video', path: 'clips/a.mp4', loopStart: 1, loopEnd: 3 },
        ]);
        expect(videos[0].currentTime).toBe(1);
    });

    it('lets each of two uses of one file keep its own element', () => {
        const sources = new SourceTextures(fakeDevice(), vi.fn());
        const a = video('clips/a.mp4');
        const fast = { ...a, speed: 2 };
        sources.sync([a, fast]);
        sources.sync([fast, a]);
        expect(videos).toHaveLength(2);
        expect(videos[0].playbackRate).toBe(1);
        expect(videos[1].playbackRate).toBe(2);
    });

    it('pauses every video while stopped and restarts them from the loop start', () => {
        const sources = new SourceTextures(fakeDevice(), vi.fn());
        sources.sync([
            video('clips/a.mp4'),
            { kind: 'video', path: 'clips/b.mp4', loopStart: 2, loopEnd: 4 },
        ]);
        videos[0].currentTime = 3;
        videos[1].currentTime = 3;
        play.mockClear();
        sources.pause();
        expect(pause).toHaveBeenCalledTimes(2);
        expect(videos[0].currentTime).toBe(3);

        sources.restart();
        expect(play).toHaveBeenCalledTimes(2);
        expect(videos[0].currentTime).toBe(0);
        expect(videos[1].currentTime).toBe(2);
    });

    it('starts a video that arrives while stopped paused', () => {
        const sources = new SourceTextures(fakeDevice(), vi.fn());
        sources.pause();
        play.mockClear();
        sources.sync([video('clips/a.mp4')]);
        expect(play).not.toHaveBeenCalled();
    });

    it('holds a video at speed 0', () => {
        const sources = new SourceTextures(fakeDevice(), vi.fn());
        play.mockClear();
        sources.sync([{ kind: 'video', path: 'clips/a.mp4', speed: 0 }]);
        expect(play).not.toHaveBeenCalled();
    });

    it('ignores the abort of a play that a pause cancelled', async () => {
        const onError = vi.fn();
        const abort = new Error('interrupted');
        abort.name = 'AbortError';
        play.mockRejectedValueOnce(abort);
        const sources = new SourceTextures(fakeDevice(), onError);
        sources.sync([video('clips/a.mp4')]);
        await Promise.resolve();
        await Promise.resolve();
        expect(onError).not.toHaveBeenCalled();
    });

    describe('cameras and screens', () => {
        const camera = (device?: string) => ({
            kind: 'camera' as const,
            path: '',
            ...(device === undefined ? {} : { device }),
        });
        const screen = { kind: 'screen' as const, path: '' };

        function fakeStream() {
            const track = { addEventListener: vi.fn(), stop: vi.fn() };
            const stream = {
                getTracks: () => [track],
                getVideoTracks: () => [track],
            } as unknown as MediaStream;
            return { stream, track };
        }

        const settle = async () => {
            await Promise.resolve();
            await Promise.resolve();
        };

        it('shows the stream the opener gives in a video element and plays it', async () => {
            const { stream } = fakeStream();
            const open = vi.fn(() => Promise.resolve(stream));
            const sources = new SourceTextures(fakeDevice(), vi.fn(), open);
            sources.sync([camera('FaceTime')]);
            await settle();
            expect(open).toHaveBeenCalledWith(camera('FaceTime'));
            expect((videos[0] as { srcObject?: unknown }).srcObject).toBe(
                stream,
            );
            expect(play).toHaveBeenCalled();
        });

        it('stops the stream when the source is dropped', async () => {
            const { stream, track } = fakeStream();
            const sources = new SourceTextures(fakeDevice(), vi.fn(), () =>
                Promise.resolve(stream),
            );
            sources.sync([screen]);
            await settle();
            sources.sync([]);
            expect(track.stop).toHaveBeenCalled();
        });

        it('stops a stream that arrives after its source was dropped', async () => {
            const { stream, track } = fakeStream();
            let deliver: (s: MediaStream) => void = () => undefined;
            const sources = new SourceTextures(
                fakeDevice(),
                vi.fn(),
                () => new Promise<MediaStream>((r) => (deliver = r)),
            );
            sources.sync([camera()]);
            sources.sync([]);
            deliver(stream);
            await settle();
            expect(track.stop).toHaveBeenCalled();
            expect((videos[0] as { srcObject?: unknown }).srcObject).toBeNull();
        });

        it('names a camera in the error when it cannot be opened', async () => {
            const onError = vi.fn();
            const sources = new SourceTextures(fakeDevice(), onError, () =>
                Promise.reject(new Error('camera access is not allowed')),
            );
            sources.sync([camera()]);
            await settle();
            expect(onError).toHaveBeenCalledWith(
                'camera: camera access is not allowed',
            );
        });

        it('reports a screen when live sources are not available', () => {
            const onError = vi.fn();
            const sources = new SourceTextures(fakeDevice(), onError);
            sources.sync([screen]);
            expect(onError).toHaveBeenCalledWith(
                'screen: live sources are not available here',
            );
        });

        it('opens a new stream when the camera changes, rather than reuse the element', async () => {
            const open = vi.fn(() => Promise.resolve(fakeStream().stream));
            const sources = new SourceTextures(fakeDevice(), vi.fn(), open);
            sources.sync([camera('a')]);
            sources.sync([camera('b')]);
            await settle();
            expect(open).toHaveBeenCalledTimes(2);
            expect(videos).toHaveLength(2);
        });

        it('keeps a camera that is still wanted when the set is synced again', async () => {
            const open = vi.fn(() => Promise.resolve(fakeStream().stream));
            const sources = new SourceTextures(fakeDevice(), vi.fn(), open);
            sources.sync([camera('a')]);
            sources.sync([camera('a')]);
            await settle();
            expect(open).toHaveBeenCalledTimes(1);
        });

        it('pauses and resumes a camera with the engine without seeking it', async () => {
            const sources = new SourceTextures(fakeDevice(), vi.fn(), () =>
                Promise.resolve(fakeStream().stream),
            );
            sources.sync([camera()]);
            await settle();
            const seek = vi.spyOn(videos[0], 'currentTime', 'set');
            sources.pause();
            expect(pause).toHaveBeenCalled();
            play.mockClear();
            sources.restart();
            expect(play).toHaveBeenCalled();
            expect(seek).not.toHaveBeenCalled();
        });
    });
});
