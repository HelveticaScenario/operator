import { net, protocol } from 'electron';
import fs from 'node:fs';
import path from 'node:path';
import { Readable } from 'node:stream';
import {
    MEDIA_SCHEME,
    parseMediaUrl,
    parseRemoteMediaUrl,
} from '../shared/video/mediaUrl';

/**
 * Lets the renderer load the workspace's pictures and recordings. Must run
 * before the app is ready.
 */
export function registerMediaScheme(): void {
    protocol.registerSchemesAsPrivileged([
        {
            privileges: {
                bypassCSP: true,
                corsEnabled: true,
                secure: true,
                standard: true,
                stream: true,
                supportFetchAPI: true,
            },
            scheme: MEDIA_SCHEME,
        },
    ]);
}

/**
 * The file a media request names, or null when there is no workspace, the URL
 * is malformed, or the path would leave the workspace folder.
 */
export function resolveMediaFile(
    workspaceRoot: string | null,
    url: string,
): string | null {
    const relative = parseMediaUrl(url);
    return relative === null || relative === ''
        ? null
        : workspaceFile(workspaceRoot, relative);
}

/** The absolute path of `relative`, or null when there is no workspace or it would leave the workspace folder. */
function workspaceFile(
    workspaceRoot: string | null,
    relative: string,
): string | null {
    if (workspaceRoot === null) return null;
    const root = path.resolve(workspaceRoot);
    const file = path.resolve(root, relative);
    return file.startsWith(root + path.sep) ? file : null;
}

/** Whether `relative` names an existing file inside the workspace folder. */
export function mediaFileExists(
    workspaceRoot: string | null,
    relative: string,
): boolean {
    const file = workspaceFile(workspaceRoot, relative);
    return file !== null && fs.existsSync(file);
}

const CONTENT_TYPES: Record<string, string> = {
    avif: 'image/avif',
    bmp: 'image/bmp',
    gif: 'image/gif',
    jpeg: 'image/jpeg',
    jpg: 'image/jpeg',
    m4v: 'video/x-m4v',
    mov: 'video/quicktime',
    mp4: 'video/mp4',
    ogv: 'video/ogg',
    png: 'image/png',
    webm: 'video/webm',
    webp: 'image/webp',
};

/**
 * The bytes a `Range` header asks of a file of `size` bytes, as an inclusive
 * range; `null` when there is no usable header, which is answered with the
 * whole file; `'unsatisfiable'` when the range lies outside the file.
 */
export function parseByteRange(
    header: string | null,
    size: number,
): { start: number; end: number } | 'unsatisfiable' | null {
    const match = header === null ? null : /^bytes=(\d*)-(\d*)$/.exec(header);
    if (match === null || (match[1] === '' && match[2] === '')) return null;
    let start: number;
    let end: number;
    if (match[1] === '') {
        const suffix = Number(match[2]);
        if (suffix === 0) return 'unsatisfiable';
        start = Math.max(0, size - suffix);
        end = size - 1;
    } else {
        start = Number(match[1]);
        end = match[2] === '' ? size - 1 : Math.min(Number(match[2]), size - 1);
    }
    return start >= size || start > end ? 'unsatisfiable' : { start, end };
}

/**
 * Fetches a network URL on the renderer's behalf and adds the CORS header the
 * response lacks, so a shader may read its pixels. Byte ranges go through for
 * seeking, and the body is streamed, so a live stream plays as it arrives.
 */
async function proxyRemote(url: string, request: Request): Promise<Response> {
    const headers = new Headers();
    const range = request.headers.get('Range');
    if (range !== null) headers.set('Range', range);
    let upstream: Response;
    try {
        upstream = await net.fetch(url, { headers, signal: request.signal });
    } catch (error) {
        return new Response(
            error instanceof Error ? error.message : String(error),
            { status: 502 },
        );
    }
    const out = new Headers(upstream.headers);
    out.set('Access-Control-Allow-Origin', '*');
    // The body is already decoded, so the encoding and length no longer describe it.
    out.delete('Content-Encoding');
    out.delete('Content-Length');
    return new Response(upstream.body, {
        headers: out,
        status: upstream.status,
        statusText: upstream.statusText,
    });
}

/**
 * Serves workspace files over the media scheme, and network URLs through
 * {@link proxyRemote}. Byte ranges are answered so a video can seek, which its
 * loop points depend on.
 */
export function handleMediaProtocol(
    getWorkspaceRoot: () => string | null,
): void {
    protocol.handle(MEDIA_SCHEME, async (request) => {
        const remote = parseRemoteMediaUrl(request.url);
        if (remote !== null) return proxyRemote(remote, request);
        const file = resolveMediaFile(getWorkspaceRoot(), request.url);
        if (file === null || !fs.existsSync(file)) {
            return new Response('Not found', { status: 404 });
        }
        const { size } = await fs.promises.stat(file);
        const headers: Record<string, string> = {
            'Accept-Ranges': 'bytes',
            'Access-Control-Allow-Origin': '*',
            'Content-Type':
                CONTENT_TYPES[path.extname(file).slice(1).toLowerCase()] ??
                'application/octet-stream',
        };
        const range = parseByteRange(request.headers.get('Range'), size);
        if (range === 'unsatisfiable') {
            headers['Content-Range'] = `bytes */${size}`;
            return new Response(null, { headers, status: 416 });
        }
        const { start, end } = range ?? { start: 0, end: size - 1 };
        headers['Content-Length'] = String(size === 0 ? 0 : end - start + 1);
        if (range !== null) {
            headers['Content-Range'] = `bytes ${start}-${end}/${size}`;
        }
        const body =
            size === 0
                ? null
                : (Readable.toWeb(
                      fs.createReadStream(file, { end, start }),
                  ) as ReadableStream);
        return new Response(body, {
            headers,
            status: range === null ? 200 : 206,
        });
    });
}
