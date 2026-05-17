import type { DSLExecutionOptions, WavsFolderNode } from '../executor/types';

/**
 * `$wavs()` — return a proxy tree matching the wavs/ folder structure.
 * Leaf nodes trigger `loadWav()` and return `{ type: 'wav_ref', ... }` objects.
 *
 * Numeric-index access (`$wavs()[i]` or `$wavs().folder[i]`) wraps modulo the
 * lexicographically-sorted direct file list at that node. Subfolders are not
 * included in the index — they have their own.
 */
export function create$wavs(options: DSLExecutionOptions) {
    return (): unknown => {
        const tree = options.wavsFolderTree;
        if (!tree) {
            return new Proxy(
                {},
                {
                    get(_target, prop) {
                        throw new Error(
                            `$wavs().${String(prop)}: no wavs/ folder found in workspace`,
                        );
                    },
                },
            );
        }

        // Memoize the lexicographically-sorted list of direct file basenames
        // per folder node, so numeric-index resolution sorts at most once
        // per node regardless of how many `$wavs()[i]` calls happen.
        const sortedFilesCache = new WeakMap<WavsFolderNode, string[]>();
        function sortedFileList(node: WavsFolderNode): string[] {
            const cached = sortedFilesCache.get(node);
            if (cached) return cached;
            const files = Object.entries(node)
                .filter(([, v]) => v === 'file')
                .map(([k]) => k)
                .sort((a, b) => a.localeCompare(b));
            sortedFilesCache.set(node, files);
            return files;
        }

        function makeProxy(node: WavsFolderNode, pathParts: string[]): unknown {
            // Resolve a known file leaf (basename `fileName` exists in `node`
            // as a `'file'`) into a `WavHandle`. Single source of truth shared
            // by named-key access and numeric-index access.
            function loadFile(fileName: string): unknown {
                const relPath = [...pathParts, fileName].join('/');
                if (!options.loadWav) {
                    throw new Error('$wavs(): loadWav function not provided');
                }
                const info = options.loadWav(relPath);
                return {
                    type: 'wav_ref' as const,
                    path: relPath,
                    channels: info.channels,
                    sampleRate: info.sampleRate,
                    frameCount: info.frameCount,
                    duration: info.duration,
                    bitDepth: info.bitDepth,
                    mtime: info.mtime,
                    ...(info.pitch != null && { pitch: info.pitch }),
                    ...(info.playback != null && { playback: info.playback }),
                    ...(info.bpm != null && { bpm: info.bpm }),
                    ...(info.beats != null && { beats: info.beats }),
                    ...(info.timeSignature != null && {
                        timeSignature: {
                            num: info.timeSignature.num,
                            den: info.timeSignature.den,
                        },
                    }),
                    ...(info.barCount != null && { barCount: info.barCount }),
                    loops: info.loops.map(
                        (l: {
                            loopType: string;
                            start: number;
                            end: number;
                        }) => ({
                            type: l.loopType as
                                | 'forward'
                                | 'pingpong'
                                | 'backward',
                            start: l.start,
                            end: l.end,
                        }),
                    ),
                    cuePoints: info.cuePoints.map(
                        (c: { position: number; label: string }) => ({
                            position: c.position,
                            label: c.label,
                        }),
                    ),
                };
            }

            return new Proxy(
                {},
                {
                    get(_target, prop) {
                        if (typeof prop !== 'string') return undefined;

                        // Numeric index access wraps modulo the file count of
                        // this folder. Only direct files participate
                        // (subfolders excluded — they get their own index).
                        if (/^-?(0|[1-9][0-9]*)$/.test(prop)) {
                            const files = sortedFileList(node);
                            if (files.length === 0) {
                                const fullPath = [
                                    ...pathParts,
                                    `[${prop}]`,
                                ].join('/');
                                throw new Error(
                                    `$wavs(): "${fullPath}" — no wav files in this folder to index into`,
                                );
                            }
                            const i = parseInt(prop, 10);
                            const wrapped =
                                ((i % files.length) + files.length) %
                                files.length;
                            return loadFile(files[wrapped]);
                        }

                        const child = node[prop];
                        if (child === undefined) {
                            const fullPath = [...pathParts, prop].join('/');
                            throw new Error(
                                `$wavs(): "${fullPath}" not found. Available: ${Object.keys(node).join(', ') || '(empty)'}`,
                            );
                        }

                        if (child === 'file') {
                            return loadFile(prop);
                        }

                        // Directory node — return nested proxy
                        return makeProxy(child, [...pathParts, prop]);
                    },
                    ownKeys() {
                        return Object.keys(node);
                    },
                    getOwnPropertyDescriptor(_target, prop) {
                        if (typeof prop === 'string' && prop in node) {
                            return {
                                configurable: true,
                                enumerable: true,
                                writable: false,
                            };
                        }
                        return undefined;
                    },
                },
            );
        }

        return makeProxy(tree, []);
    };
}
