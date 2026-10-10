import {
    describe,
    type VideoCore,
    type VideoMediaConfig,
} from './videoBuilderTypes';
import type { VideoOutput } from './VideoOutput';

const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'avif'];
const VIDEO_EXTENSIONS = ['mp4', 'webm', 'mov', 'm4v', 'ogv'];
const FITS = ['cover', 'contain', 'stretch'];

/** A path inside the workspace folder, with `/` separators, or an error naming `fn`. */
function workspacePath(
    core: VideoCore,
    fn: string,
    path: unknown,
    extensions: string[],
): string {
    if (typeof path !== 'string' || path === '') {
        throw new Error(
            `${fn}: path must be a string naming a file in the workspace folder, got ${describe(path)}`,
        );
    }
    const normalized = path.replaceAll('\\', '/');
    if (
        normalized.startsWith('/') ||
        /^[a-zA-Z]:/.test(normalized) ||
        normalized.split('/').includes('..')
    ) {
        throw new Error(
            `${fn}: path must stay inside the workspace folder, got "${path}"`,
        );
    }
    const extension = normalized.split('.').pop()?.toLowerCase() ?? '';
    if (!extensions.includes(extension)) {
        throw new Error(
            `${fn}: "${path}" must be one of ${extensions.map((e) => `.${e}`).join(', ')}`,
        );
    }
    if (!core.mediaExists(normalized)) {
        throw new Error(`${fn}: no file "${path}" in the workspace folder`);
    }
    return normalized;
}

/** The `$v` functions that read pictures and recordings from the workspace folder. */
export function sourceMethods(core: VideoCore) {
    const make =
        (fn: string, kind: 'image' | 'video', extensions: string[]) =>
        (path: string, config?: VideoMediaConfig): VideoOutput => {
            const fit = config?.fit ?? 'cover';
            if (!FITS.includes(fit)) {
                throw new Error(
                    `${fn}: fit must be one of ${FITS.join(', ')}, got "${fit}"`,
                );
            }
            const normalized = workspacePath(core, fn, path, extensions);
            return core.addNode(
                'source',
                'color',
                {},
                { fit },
                undefined,
                undefined,
                core.sourceIndex({ kind, path: normalized }),
            );
        };
    return {
        /**
         * A picture from the workspace folder, drawn to fill the frame as `fit`
         * says.
         */
        image: make('$v.image', 'image', IMAGE_EXTENSIONS),

        /** A recording from the workspace folder, played in a loop, muted. */
        video: make('$v.video', 'video', VIDEO_EXTENSIONS),
    };
}
