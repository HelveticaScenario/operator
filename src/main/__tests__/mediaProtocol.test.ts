import { beforeAll, describe, expect, it, vi } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const protocolMock = vi.hoisted(() => ({
    fetch: vi.fn(),
    handler: null as ((request: Request) => Promise<Response>) | null,
}));
vi.mock('electron', () => ({
    net: { fetch: protocolMock.fetch },
    protocol: {
        handle: (
            _scheme: string,
            handler: (request: Request) => Promise<Response>,
        ) => {
            protocolMock.handler = handler;
        },
    },
}));

const {
    handleMediaProtocol,
    mediaFileExists,
    parseByteRange,
    resolveMediaFile,
} = await import('../mediaProtocol');
const { mediaUrl } = await import('../../shared/video/mediaUrl');

describe('media files', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'media-root-'));
    fs.mkdirSync(path.join(root, 'pictures'));
    fs.writeFileSync(path.join(root, 'pictures', 'photo.png'), 'x');
    const outside = path.join(path.dirname(root), 'secret.png');
    fs.writeFileSync(outside, 'x');

    it('resolves a path inside the workspace', () => {
        expect(resolveMediaFile(root, mediaUrl('pictures/photo.png'))).toBe(
            path.join(root, 'pictures', 'photo.png'),
        );
    });

    const stays = (resolved: string | null) =>
        resolved === null || resolved.startsWith(root + path.sep);

    it('never resolves outside the workspace folder, however the path is written', () => {
        for (const url of [
            mediaUrl('../secret.png'),
            'operator-media://workspace/%2e%2e/secret.png',
            'operator-media://workspace/%2E%2E%2Fsecret.png',
            mediaUrl('pictures/../../secret.png'),
            'operator-media://workspace/..%5Csecret.png',
        ]) {
            const resolved = resolveMediaFile(root, url);
            expect(stays(resolved)).toBe(true);
            expect(resolved).not.toBe(outside);
        }
    });

    it('refuses the workspace folder itself, a bad URL, or no workspace', () => {
        expect(
            resolveMediaFile(root, 'operator-media://workspace/'),
        ).toBeNull();
        expect(resolveMediaFile(root, 'https://example.com/a.png')).toBeNull();
        expect(
            resolveMediaFile(null, mediaUrl('pictures/photo.png')),
        ).toBeNull();
    });

    it('does not mistake a sibling folder with the same prefix for the workspace', () => {
        const sibling = `${root}-other`;
        fs.mkdirSync(sibling, { recursive: true });
        fs.writeFileSync(path.join(sibling, 'a.png'), 'x');
        const resolved = resolveMediaFile(
            root,
            mediaUrl(`../${path.basename(sibling)}/a.png`),
        );
        expect(stays(resolved)).toBe(true);
        expect(resolved).not.toBe(path.join(sibling, 'a.png'));
    });

    it('reports whether a workspace file exists', () => {
        expect(mediaFileExists(root, 'pictures/photo.png')).toBe(true);
        expect(mediaFileExists(root, 'pictures/missing.png')).toBe(false);
        expect(mediaFileExists(root, '../secret.png')).toBe(false);
        expect(mediaFileExists(null, 'pictures/photo.png')).toBe(false);
    });
});

describe('byte ranges', () => {
    it('reads a closed, an open and a suffix range', () => {
        expect(parseByteRange('bytes=10-19', 100)).toEqual({
            start: 10,
            end: 19,
        });
        expect(parseByteRange('bytes=90-', 100)).toEqual({
            start: 90,
            end: 99,
        });
        expect(parseByteRange('bytes=-10', 100)).toEqual({
            start: 90,
            end: 99,
        });
    });

    it('clips an end past the file and a suffix longer than it', () => {
        expect(parseByteRange('bytes=90-500', 100)).toEqual({
            start: 90,
            end: 99,
        });
        expect(parseByteRange('bytes=-500', 100)).toEqual({
            start: 0,
            end: 99,
        });
    });

    it('answers no header, or one it does not understand, with the whole file', () => {
        expect(parseByteRange(null, 100)).toBeNull();
        expect(parseByteRange('bytes=0-1,5-9', 100)).toBeNull();
        expect(parseByteRange('items=0-1', 100)).toBeNull();
        expect(parseByteRange('bytes=-', 100)).toBeNull();
    });

    it('rejects a range outside the file', () => {
        expect(parseByteRange('bytes=100-', 100)).toBe('unsatisfiable');
        expect(parseByteRange('bytes=50-10', 100)).toBe('unsatisfiable');
        expect(parseByteRange('bytes=-0', 100)).toBe('unsatisfiable');
    });
});

describe('serving media', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'media-serve-'));
    fs.mkdirSync(path.join(root, 'clips'));
    fs.writeFileSync(path.join(root, 'clips', 'a.mp4'), '0123456789');
    handleMediaProtocol(() => root);
    const get = (relative: string, range?: string) =>
        protocolMock.handler!(
            new Request(mediaUrl(relative), {
                headers: range === undefined ? {} : { Range: range },
            }),
        );

    it('serves the whole file with the range support a video needs to seek', async () => {
        const response = await get('clips/a.mp4');
        expect(response.status).toBe(200);
        expect(response.headers.get('Accept-Ranges')).toBe('bytes');
        expect(response.headers.get('Content-Type')).toBe('video/mp4');
        expect(response.headers.get('Content-Length')).toBe('10');
        expect(await response.text()).toBe('0123456789');
    });

    it('serves the bytes of a range as partial content', async () => {
        const response = await get('clips/a.mp4', 'bytes=3-5');
        expect(response.status).toBe(206);
        expect(response.headers.get('Content-Range')).toBe('bytes 3-5/10');
        expect(response.headers.get('Content-Length')).toBe('3');
        expect(await response.text()).toBe('345');
    });

    it('serves from an offset to the end', async () => {
        const response = await get('clips/a.mp4', 'bytes=7-');
        expect(response.status).toBe(206);
        expect(await response.text()).toBe('789');
    });

    it('refuses a range outside the file', async () => {
        const response = await get('clips/a.mp4', 'bytes=20-');
        expect(response.status).toBe(416);
        expect(response.headers.get('Content-Range')).toBe('bytes */10');
    });

    it('does not find a file that is missing', async () => {
        expect((await get('clips/none.mp4')).status).toBe(404);
    });
});

describe('serving network media', () => {
    let handler: (request: Request) => Promise<Response>;
    beforeAll(() => {
        handleMediaProtocol(() => null);
        handler = protocolMock.handler!;
    });
    const remote = 'https://cdn.example.com/live/clip.mp4?token=a b';
    const get = (url: string, range?: string) =>
        handler(
            new Request(mediaUrl(url), {
                headers: range === undefined ? {} : { Range: range },
            }),
        );

    it('fetches the address and answers with the CORS header a shader needs', async () => {
        protocolMock.fetch.mockResolvedValueOnce(
            new Response('video-bytes', {
                headers: { 'Content-Type': 'video/mp4' },
            }),
        );
        const response = await get(remote);
        expect(protocolMock.fetch.mock.calls.at(-1)![0]).toBe(remote);
        expect(response.status).toBe(200);
        expect(response.headers.get('Access-Control-Allow-Origin')).toBe('*');
        expect(response.headers.get('Content-Type')).toBe('video/mp4');
        expect(await response.text()).toBe('video-bytes');
    });

    it('passes a byte range on and answers with the partial content', async () => {
        protocolMock.fetch.mockResolvedValueOnce(
            new Response('ytes', {
                headers: { 'Content-Range': 'bytes 1-4/11' },
                status: 206,
            }),
        );
        const response = await get(remote, 'bytes=1-4');
        const sent = protocolMock.fetch.mock.calls.at(-1)![1] as {
            headers: Headers;
        };
        expect(sent.headers.get('Range')).toBe('bytes=1-4');
        expect(response.status).toBe(206);
        expect(response.headers.get('Content-Range')).toBe('bytes 1-4/11');
    });

    it('drops the length and encoding of a body that has already been decoded', async () => {
        protocolMock.fetch.mockResolvedValueOnce(
            new Response('decoded', {
                headers: { 'Content-Encoding': 'gzip', 'Content-Length': '3' },
            }),
        );
        const response = await get(remote);
        expect(response.headers.get('Content-Encoding')).toBeNull();
        expect(response.headers.get('Content-Length')).toBeNull();
    });

    it('passes on the status of a missing file', async () => {
        protocolMock.fetch.mockResolvedValueOnce(
            new Response('no', { status: 404 }),
        );
        expect((await get(remote)).status).toBe(404);
    });

    it('answers 502 when the address cannot be reached', async () => {
        protocolMock.fetch.mockRejectedValueOnce(new Error('ECONNREFUSED'));
        const response = await get(remote);
        expect(response.status).toBe(502);
        expect(await response.text()).toBe('ECONNREFUSED');
    });

    it('does not fetch anything that is not an http or https address', async () => {
        protocolMock.fetch.mockClear();
        const response = await handler(
            new Request(
                `operator-media://remote/${encodeURIComponent('file:///etc/passwd')}`,
            ),
        );
        expect(response.status).toBe(404);
        expect(protocolMock.fetch).not.toHaveBeenCalled();
    });
});
