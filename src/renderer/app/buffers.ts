import type { EditorBuffer, UnsavedBufferSnapshot } from '../types/editor';

export const DEFAULT_PATCH = `// Simple 440 Hz sine wave
$sine('a3').out();
`;

const UNSAVED_STORAGE_KEY = 'modular_unsaved_buffers';

export const readUnsavedBuffers = (): EditorBuffer[] => {
    if (typeof window === 'undefined') {
        return [];
    }

    try {
        const raw = window.localStorage.getItem(UNSAVED_STORAGE_KEY);
        if (!raw) {
            return [];
        }

        const parsed = JSON.parse(raw) as UnsavedBufferSnapshot[];
        return parsed.map((snapshot): EditorBuffer => {
            if (snapshot.kind === 'file') {
                return {
                    content: snapshot.content,
                    dirty: true,
                    filePath: snapshot.filePath,
                    id: snapshot.id,
                    kind: 'file',
                };
            }
            return {
                content: snapshot.content,
                dirty: true,
                id: snapshot.id,
                kind: 'untitled',
            };
        });
    } catch (error) {
        console.error('Failed to read unsaved buffers:', error);
        return [];
    }
};

export const saveUnsavedBuffers = (buffers: EditorBuffer[]) => {
    if (typeof window === 'undefined') {
        return;
    }

    try {
        const dirtyBuffers = buffers.filter((b) => b.dirty);
        const snapshots: UnsavedBufferSnapshot[] = dirtyBuffers.map(
            (buffer) => {
                if (buffer.kind === 'file') {
                    return {
                        content: buffer.content,
                        filePath: buffer.filePath,
                        id: buffer.filePath,
                        kind: 'file',
                    };
                }
                return {
                    content: buffer.content,
                    id: buffer.id,
                    kind: 'untitled',
                };
            },
        );

        window.localStorage.setItem(
            UNSAVED_STORAGE_KEY,
            JSON.stringify(snapshots),
        );
    } catch (error) {
        console.error('Failed to save unsaved buffers:', error);
    }
};

export const getBufferId = (buffer: EditorBuffer): string =>
    buffer.kind === 'file' ? buffer.filePath : buffer.id;

export const formatBufferLabel = (buffer: EditorBuffer) => {
    if (buffer.kind === 'untitled') {
        return buffer.id;
    }
    return buffer.filePath;
};

export const normalizeFileName = (name: string) => {
    const trimmed = name.trim();
    if (!trimmed) {
        return trimmed;
    }
    return trimmed.endsWith('.js') || trimmed.endsWith('.mjs')
        ? trimmed
        : `${trimmed}.mjs`;
};
