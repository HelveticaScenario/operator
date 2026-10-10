/** Scheme the app serves workspace media over; the host part is a fixed label. */
export const MEDIA_SCHEME = 'operator-media';
const MEDIA_HOST = 'workspace';
const REMOTE_HOST = 'remote';

/** Whether a media path is an http or https URL on the network, not a workspace file. */
export function isRemoteMedia(path: string): boolean {
    return /^https?:\/\//i.test(path);
}

/** Whether a network URL is an HLS playlist, which the browser engine cannot play itself. */
export function isHlsUrl(url: string): boolean {
    try {
        return new URL(url).pathname.toLowerCase().endsWith('.m3u8');
    } catch {
        return false;
    }
}

/**
 * The URL the renderer loads the workspace file or network URL `path` from.
 * Network media goes through the app, which adds the CORS headers a shader
 * needs to read a video's pixels.
 */
export function mediaUrl(path: string): string {
    if (isRemoteMedia(path)) {
        return `${MEDIA_SCHEME}://${REMOTE_HOST}/${encodeURIComponent(path)}`;
    }
    const encoded = path.split('/').map(encodeURIComponent).join('/');
    return `${MEDIA_SCHEME}://${MEDIA_HOST}/${encoded}`;
}

/**
 * The workspace-relative path a media URL names, or null when the URL is not a
 * well-formed media URL. The result may still contain `..`; resolving it
 * against the workspace folder and checking containment is the caller's job.
 */
export function parseMediaUrl(url: string): string | null {
    let parsed: URL;
    try {
        parsed = new URL(url);
    } catch {
        return null;
    }
    if (parsed.protocol !== `${MEDIA_SCHEME}:` || parsed.host !== MEDIA_HOST) {
        return null;
    }
    try {
        return parsed.pathname
            .slice(1)
            .split('/')
            .map(decodeURIComponent)
            .join('/');
    } catch {
        return null;
    }
}

/** The network URL a media URL stands for, or null when it is not one. */
export function parseRemoteMediaUrl(url: string): string | null {
    let parsed: URL;
    try {
        parsed = new URL(url);
    } catch {
        return null;
    }
    if (parsed.protocol !== `${MEDIA_SCHEME}:` || parsed.host !== REMOTE_HOST) {
        return null;
    }
    try {
        const remote = decodeURIComponent(parsed.pathname.slice(1));
        return isRemoteMedia(remote) ? remote : null;
    } catch {
        return null;
    }
}
