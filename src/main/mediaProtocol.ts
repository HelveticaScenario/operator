import { net, protocol } from 'electron';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { MEDIA_SCHEME, parseMediaUrl } from '../shared/video/mediaUrl';

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
    if (workspaceRoot === null) return null;
    const relative = parseMediaUrl(url);
    if (relative === null || relative === '') return null;
    const root = path.resolve(workspaceRoot);
    const file = path.resolve(root, relative);
    return file.startsWith(root + path.sep) ? file : null;
}

/** Whether `relative` names an existing file inside the workspace folder. */
export function mediaFileExists(
    workspaceRoot: string | null,
    relative: string,
): boolean {
    if (workspaceRoot === null) return false;
    const root = path.resolve(workspaceRoot);
    const file = path.resolve(root, relative);
    return file.startsWith(root + path.sep) && fs.existsSync(file);
}

/** Serves workspace files over the media scheme, with range requests for video. */
export function handleMediaProtocol(
    getWorkspaceRoot: () => string | null,
): void {
    protocol.handle(MEDIA_SCHEME, async (request) => {
        const file = resolveMediaFile(getWorkspaceRoot(), request.url);
        if (file === null || !fs.existsSync(file)) {
            return new Response('Not found', { status: 404 });
        }
        const response = await net.fetch(pathToFileURL(file).toString(), {
            headers: request.headers,
        });
        const headers = new Headers(response.headers);
        headers.set('Access-Control-Allow-Origin', '*');
        return new Response(response.body, {
            headers,
            status: response.status,
            statusText: response.statusText,
        });
    });
}
