import type { VideoSourceDef } from '../../../shared/video/videoGraph';
import {
    describe,
    type VideoCore,
    type VideoVideoConfig,
} from './videoBuilderTypes';
import type { VideoOutput } from './VideoOutput';

const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'avif'];
const VIDEO_EXTENSIONS = ['mp4', 'webm', 'mov', 'm4v', 'ogv'];
const FITS = ['cover', 'contain', 'stretch'];
/** The fastest playback rate the browser engine's media element supports. */
const MAX_SPEED = 16;

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

/** The `speed` and `loop` options of `$v.video` as the fields of a source definition. */
function playback(
    config: VideoVideoConfig | undefined,
): Pick<VideoSourceDef, 'speed' | 'loopStart' | 'loopEnd'> {
    const out: Pick<VideoSourceDef, 'speed' | 'loopStart' | 'loopEnd'> = {};
    const speed = config?.speed ?? 1;
    if (typeof speed !== 'number' || !(speed >= 0 && speed <= MAX_SPEED)) {
        throw new Error(
            `$v.video: speed must be a number from 0 to ${MAX_SPEED} (video cannot play backwards), got ${describe(speed)}`,
        );
    }
    if (speed !== 1) out.speed = speed;
    const loop = config?.loop;
    if (loop !== undefined) {
        const [start, end] = Array.isArray(loop) ? loop : [];
        const valid =
            Array.isArray(loop) &&
            loop.length >= 1 &&
            loop.length <= 2 &&
            typeof start === 'number' &&
            Number.isFinite(start) &&
            start >= 0 &&
            (end === undefined ||
                (typeof end === 'number' &&
                    Number.isFinite(end) &&
                    end > start));
        if (!valid) {
            throw new Error(
                `$v.video: loop must be [start] or [start, end] in seconds with 0 <= start < end, got ${describe(loop)}`,
            );
        }
        if (start > 0) out.loopStart = start;
        if (end !== undefined) out.loopEnd = end;
    }
    return out;
}

/** The `$v` functions that read pictures and recordings from the workspace folder. */
export function sourceMethods(core: VideoCore) {
    const make =
        (fn: string, kind: 'image' | 'video', extensions: string[]) =>
        (path: string, config?: VideoVideoConfig): VideoOutput => {
            const fit = config?.fit ?? 'cover';
            if (!FITS.includes(fit)) {
                throw new Error(
                    `${fn}: fit must be one of ${FITS.join(', ')}, got "${fit}"`,
                );
            }
            if (
                kind === 'image' &&
                (config?.speed !== undefined || config?.loop !== undefined)
            ) {
                throw new Error(`${fn}: speed and loop apply only to video`);
            }
            const normalized = workspacePath(core, fn, path, extensions);
            return core.addNode(
                'source',
                'color',
                {},
                { fit },
                undefined,
                undefined,
                core.sourceIndex({
                    kind,
                    path: normalized,
                    ...(kind === 'video' ? playback(config) : {}),
                }),
            );
        };
    return {
        /**
         * A picture from the workspace folder, drawn to fill the frame as `fit`
         * says.
         */
        image: make('$v.image', 'image', IMAGE_EXTENSIONS),

        /**
         * A recording from the workspace folder, played in a loop, muted, at
         * `speed` and between the `loop` points.
         */
        video: make('$v.video', 'video', VIDEO_EXTENSIONS),
    };
}
