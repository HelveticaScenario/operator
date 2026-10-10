import type { Collection } from '../GraphBuilder';
import { isRemoteMedia } from '../../../shared/video/mediaUrl';
import type { VideoSourceDef } from '../../../shared/video/videoGraph';
import {
    describe,
    type VideoCameraConfig,
    type VideoCore,
    type VideoMediaConfig,
    type VideoScreenConfig,
    type VideoVideoConfig,
} from './videoBuilderTypes';
import type { VideoOutput } from './VideoOutput';

const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'avif'];
const VIDEO_EXTENSIONS = ['mp4', 'webm', 'mov', 'm4v', 'ogv'];
const FITS = ['cover', 'contain', 'stretch'];
/** The fastest playback rate the browser engine's media element supports. */
const MAX_SPEED = 16;

/** A network URL, normalized, or an error naming `fn` if it cannot be one. */
function networkUrl(fn: string, url: string): string {
    try {
        return new URL(url).href;
    } catch {
        throw new Error(`${fn}: "${url}" is not a valid URL`);
    }
}

/** The scheme of a `scheme://` address that is not a workspace path, or an error naming `fn`. */
function rejectScheme(fn: string, path: string): void {
    const scheme = /^([a-z][a-z0-9+.-]*):\/\//i.exec(path);
    if (scheme !== null) {
        throw new Error(
            `${fn}: ${scheme[1]}:// addresses cannot be played; use an http or https URL, such as an .m3u8 HLS stream or an .mp4 file`,
        );
    }
}

/**
 * A path inside the workspace folder, with `/` separators, or, when `remote`
 * allows it, an http(s) URL as given, or an error naming `fn`.
 */
function mediaPath(
    core: VideoCore,
    fn: string,
    path: unknown,
    extensions: string[],
    remote: boolean,
): string {
    if (typeof path !== 'string' || path === '') {
        throw new Error(
            `${fn}: path must be a string naming a file in the workspace folder${remote ? ' or an http(s) URL' : ''}, got ${describe(path)}`,
        );
    }
    if (isRemoteMedia(path)) {
        if (!remote) {
            throw new Error(
                `${fn}: plays files in the workspace folder; use $v.stream for the network address "${path}"`,
            );
        }
        return networkUrl(fn, path);
    }
    rejectScheme(fn, path);
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

/** How a source that does not match the frame is fitted, or an error naming `fn`. */
function fitOf(fn: string, config: VideoMediaConfig | undefined): string {
    const fit = config?.fit ?? 'cover';
    if (!FITS.includes(fit)) {
        throw new Error(
            `${fn}: fit must be one of ${FITS.join(', ')}, got "${fit}"`,
        );
    }
    return fit;
}

/**
 * Gives a video's color an `audio` property: the audio track as an audio
 * signal, playing at the video's speed between its loop points. One signal is
 * made however often the property is read.
 */
function withAudio(
    core: VideoCore,
    output: VideoOutput,
    path: string,
    timing: Pick<VideoSourceDef, 'speed' | 'loopStart' | 'loopEnd'>,
): VideoOutput {
    let audio: Collection | undefined;
    Object.defineProperty(output, 'audio', {
        enumerable: true,
        get: () =>
            (audio ??= core.mediaAudio(path, {
                speed: timing.speed ?? 1,
                loopStart: timing.loopStart ?? 0,
                ...(timing.loopEnd !== undefined && {
                    loopEnd: timing.loopEnd,
                }),
            })),
    });
    return output;
}

/** The `$v` functions that read pictures, recordings, cameras and screens. */
export function sourceMethods(core: VideoCore) {
    const live = (def: VideoSourceDef, fit: string): VideoOutput =>
        core.addNode(
            'source',
            'color',
            {},
            { fit },
            undefined,
            undefined,
            core.sourceIndex(def),
        );
    const make =
        (fn: string, kind: 'image' | 'video', extensions: string[]) =>
        (path: string, config?: VideoVideoConfig): VideoOutput => {
            const fit = fitOf(fn, config);
            if (
                kind === 'image' &&
                (config?.speed !== undefined || config?.loop !== undefined)
            ) {
                throw new Error(`${fn}: speed and loop apply only to video`);
            }
            const normalized = mediaPath(
                core,
                fn,
                path,
                extensions,
                kind === 'image',
            );
            const timing = kind === 'video' ? playback(config) : {};
            const output = core.addNode(
                'source',
                'color',
                {},
                { fit },
                undefined,
                undefined,
                core.sourceIndex({ kind, path: normalized, ...timing }),
            );
            return kind === 'video'
                ? withAudio(core, output, normalized, timing)
                : output;
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

        /**
         * A video or live stream at an http(s) address, played as it arrives
         * and muted, as a color.
         */
        stream: (url: string, config?: VideoMediaConfig): VideoOutput => {
            const fit = fitOf('$v.stream', config);
            if (typeof url !== 'string' || !isRemoteMedia(url)) {
                if (typeof url === 'string') rejectScheme('$v.stream', url);
                throw new Error(
                    `$v.stream: url must be an http or https address, got ${describe(url)}`,
                );
            }
            return live(
                { kind: 'video', path: networkUrl('$v.stream', url) },
                fit,
            );
        },

        /** The live picture of a camera, as a color. */
        camera: (config?: VideoCameraConfig): VideoOutput => {
            const fit = fitOf('$v.camera', config);
            const { device } = config ?? {};
            if (
                device !== undefined &&
                (typeof device !== 'string' || device.trim() === '')
            ) {
                throw new Error(
                    `$v.camera: device must be part of a camera's name, got ${describe(device)}`,
                );
            }
            return live(
                { kind: 'camera', path: '', ...(device ? { device } : {}) },
                fit,
            );
        },

        /** The live picture of a display, as a color. */
        screen: (config?: VideoScreenConfig): VideoOutput => {
            const fit = fitOf('$v.screen', config);
            const { display } = config ?? {};
            if (
                display !== undefined &&
                (!Number.isInteger(display) || display < 1)
            ) {
                throw new Error(
                    `$v.screen: display must be a whole number from 1, got ${describe(display)}`,
                );
            }
            return live(
                {
                    kind: 'screen',
                    path: '',
                    ...(display !== undefined && display !== 1
                        ? { display }
                        : {}),
                },
                fit,
            );
        },
    };
}
