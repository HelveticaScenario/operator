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
});
