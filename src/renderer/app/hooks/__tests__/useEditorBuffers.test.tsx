// @vitest-environment jsdom

/**
 * Invariants around buffer identity and dirty tracking in useEditorBuffers:
 *
 * - File buffers are identified by absolute path everywhere, including a
 *   just-saved untitled buffer (the save dialog returns workspace-relative
 *   paths for in-workspace saves), so one disk file can never be open as two
 *   divergent buffers.
 * - Saving only marks a buffer clean if its content still equals the snapshot
 *   that reached disk; keystrokes landing during the async write stay dirty
 *   and are never reverted.
 * - A file buffer is dirty exactly when its content differs from what is on
 *   disk, including buffers restored from storage.
 * - Saving never overwrites a file that changed on disk since the buffer last
 *   read or wrote it: the save is refused as a conflict, resolved by
 *   overwriting or by reverting the buffer to the disk content.
 * - Open file buffers are watched; a clean buffer takes external changes to
 *   its file, a dirty one keeps its edits.
 * - Closing a dirty untitled buffer via the Save choice closes the tab under
 *   the buffer's post-save id; cancelling the save dialog keeps the tab open.
 */

import { act, createElement, useEffect } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

const api = vi.hoisted(() => ({
    filesystem: {
        deleteFile: vi.fn(),
        readFile: vi.fn(),
        renameFile: vi.fn(),
        onFileChanged: vi.fn(),
        showSaveDialog: vi.fn(),
        watchOpenFiles: vi.fn(),
        writeFile: vi.fn(),
    },
    showUnsavedChangesDialog: vi.fn(),
}));

vi.mock('../../../electronAPI', () => ({ default: api }));

import { useEditorBuffers } from '../useEditorBuffers';

type Hook = ReturnType<typeof useEditorBuffers>;

const WORKSPACE = '/workspace';

let root: Root | null = null;
let container: HTMLElement | null = null;

function renderBuffersHook() {
    const hookRef = { current: null as unknown as Hook };
    function Probe() {
        const hook = useEditorBuffers({
            refreshFileTree: async () => {},
            workspaceRoot: WORKSPACE,
        });
        // act() flushes effects, so hookRef is fresh after every act block.
        useEffect(() => {
            hookRef.current = hook;
        });
        return null;
    }
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    act(() => {
        root!.render(createElement(Probe));
    });
    return hookRef;
}

/** Let pending timers (performCloseBuffer defers by 50 ms) and IPC settle. */
async function flush(ms = 80) {
    await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, ms));
    });
}

beforeEach(() => {
    (
        globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    window.localStorage.clear();
    vi.clearAllMocks();
    api.filesystem.watchOpenFiles.mockResolvedValue(undefined);
    api.filesystem.onFileChanged.mockReturnValue(() => {});
});

/** Deliver a change event as the main process's file watcher would. */
function emitFileChange(filePath: string, content: string) {
    const listener = api.filesystem.onFileChanged.mock.lastCall?.[0] as
        | ((change: { filePath: string; content: string }) => void)
        | undefined;
    act(() => listener?.({ content, filePath }));
}

afterEach(() => {
    act(() => root?.unmount());
    root = null;
    container?.remove();
    container = null;
});

describe('buffer identity is the absolute file path', () => {
    test('saving an untitled buffer resolves the dialog result against the workspace root', async () => {
        api.filesystem.showSaveDialog.mockResolvedValue('newfile.mjs');
        api.filesystem.writeFile.mockResolvedValue({ success: true });

        const hookRef = renderBuffersHook();
        act(() => hookRef.current.createUntitledFile());

        await act(async () => {
            await hookRef.current.saveFile();
        });

        const buffer = hookRef.current.buffers[0];
        expect(buffer.kind).toBe('file');
        expect(buffer.kind === 'file' ? buffer.filePath : undefined).toBe(
            `${WORKSPACE}/newfile.mjs`,
        );
        expect(hookRef.current.activeBufferId).toBe(`${WORKSPACE}/newfile.mjs`);
        expect(api.filesystem.writeFile).toHaveBeenCalledWith(
            `${WORKSPACE}/newfile.mjs`,
            expect.any(String),
        );
    });

    test('opening a just-saved untitled buffer from the file tree reuses the buffer', async () => {
        api.filesystem.showSaveDialog.mockResolvedValue('newfile.mjs');
        api.filesystem.writeFile.mockResolvedValue({ success: true });

        const hookRef = renderBuffersHook();
        act(() => hookRef.current.createUntitledFile());
        await act(async () => {
            await hookRef.current.saveFile();
        });

        // The file tree hands openFile a workspace-relative path.
        api.filesystem.readFile.mockResolvedValue('disk content');
        await act(async () => {
            await hookRef.current.openFile('newfile.mjs');
        });

        expect(hookRef.current.buffers).toHaveLength(1);
        expect(hookRef.current.activeBufferId).toBe(`${WORKSPACE}/newfile.mjs`);
        expect(api.filesystem.readFile).not.toHaveBeenCalled();
    });
});

describe('untitled ids stay unique after a save', () => {
    test('a new untitled does not reuse the id of a file saved from an untitled buffer', async () => {
        api.filesystem.showSaveDialog.mockResolvedValue('kept.mjs');
        api.filesystem.writeFile.mockResolvedValue({ success: true });

        const hookRef = renderBuffersHook();
        act(() => hookRef.current.createUntitledFile());
        await act(async () => {
            await hookRef.current.saveFile();
        });

        // The saved buffer keeps its `untitled-1` id (so its source id stays
        // stable across the save) while its path becomes the buffer key.
        const saved = hookRef.current.buffers[0];
        expect(saved.id).toBe('untitled-1');
        expect(saved.kind).toBe('file');

        act(() => hookRef.current.createUntitledFile());

        expect(hookRef.current.buffers).toHaveLength(2);
        const ids = hookRef.current.buffers.map((b) => b.id);
        expect(new Set(ids).size).toBe(2);
        // The new untitled skips the number the saved file still holds.
        expect(ids).toContain('untitled-2');
    });
});

describe('dirty tracking across an in-flight save', () => {
    test('keystrokes typed during the write stay dirty and keep their content', async () => {
        const hookRef = renderBuffersHook();
        api.filesystem.readFile.mockResolvedValue('original');
        await act(async () => {
            await hookRef.current.openFile('a.mjs');
        });
        act(() => hookRef.current.handlePatchChange('saved snapshot'));

        let resolveWrite: (v: { success: boolean }) => void = () => {};
        api.filesystem.writeFile.mockImplementation(
            () =>
                new Promise((resolve) => {
                    resolveWrite = resolve;
                }),
        );

        let savePromise: Promise<string | undefined> | undefined;
        act(() => {
            savePromise = hookRef.current.saveFile();
        });
        act(() => hookRef.current.handlePatchChange('newer keystrokes'));
        await act(async () => {
            resolveWrite({ success: true });
            await savePromise;
        });

        expect(api.filesystem.writeFile).toHaveBeenCalledWith(
            `${WORKSPACE}/a.mjs`,
            'saved snapshot',
            'original',
        );
        const buffer = hookRef.current.buffers[0];
        expect(buffer.content).toBe('newer keystrokes');
        expect(buffer.dirty).toBe(true);
    });

    test('a save with no concurrent edits marks the buffer clean', async () => {
        const hookRef = renderBuffersHook();
        api.filesystem.readFile.mockResolvedValue('original');
        await act(async () => {
            await hookRef.current.openFile('a.mjs');
        });
        act(() => hookRef.current.handlePatchChange('edited'));
        api.filesystem.writeFile.mockResolvedValue({ success: true });

        await act(async () => {
            await hookRef.current.saveFile();
        });

        const buffer = hookRef.current.buffers[0];
        expect(buffer.content).toBe('edited');
        expect(buffer.dirty).toBe(false);
    });

    test('saving an untitled buffer preserves keystrokes typed while the dialog and write were pending', async () => {
        const hookRef = renderBuffersHook();
        act(() => hookRef.current.createUntitledFile());
        act(() => hookRef.current.handlePatchChange('saved snapshot'));

        api.filesystem.showSaveDialog.mockResolvedValue('u.mjs');
        let resolveWrite: (v: { success: boolean }) => void = () => {};
        api.filesystem.writeFile.mockImplementation(
            () =>
                new Promise((resolve) => {
                    resolveWrite = resolve;
                }),
        );

        let savePromise: Promise<string | undefined> | undefined;
        act(() => {
            savePromise = hookRef.current.saveFile();
        });
        await flush(0);
        act(() => hookRef.current.handlePatchChange('newer keystrokes'));
        await act(async () => {
            resolveWrite({ success: true });
            await savePromise;
        });

        expect(api.filesystem.writeFile).toHaveBeenCalledWith(
            `${WORKSPACE}/u.mjs`,
            'saved snapshot',
        );
        const buffer = hookRef.current.buffers[0];
        expect(buffer.kind).toBe('file');
        expect(buffer.content).toBe('newer keystrokes');
        expect(buffer.dirty).toBe(true);
    });
});

describe('file buffers are dirty only when they differ from disk', () => {
    test('editing back to the disk content clears the dirty flag', async () => {
        const hookRef = renderBuffersHook();
        api.filesystem.readFile.mockResolvedValue('original');
        await act(async () => {
            await hookRef.current.openFile('a.mjs');
        });

        act(() => hookRef.current.handlePatchChange('original!'));
        expect(hookRef.current.buffers[0].dirty).toBe(true);

        act(() => hookRef.current.handlePatchChange('original'));
        expect(hookRef.current.buffers[0].dirty).toBe(false);
    });

    test('editing back to the content of the last save clears the dirty flag', async () => {
        const hookRef = renderBuffersHook();
        api.filesystem.readFile.mockResolvedValue('original');
        api.filesystem.writeFile.mockResolvedValue({ success: true });
        await act(async () => {
            await hookRef.current.openFile('a.mjs');
        });
        act(() => hookRef.current.handlePatchChange('saved'));
        await act(async () => {
            await hookRef.current.saveFile();
        });

        act(() => hookRef.current.handlePatchChange('original'));
        expect(hookRef.current.buffers[0].dirty).toBe(true);
        act(() => hookRef.current.handlePatchChange('saved'));
        expect(hookRef.current.buffers[0].dirty).toBe(false);
    });

    test('a restored buffer is compared against the file on disk', async () => {
        window.localStorage.setItem(
            'modular_unsaved_buffers',
            JSON.stringify([
                {
                    content: 'same',
                    filePath: `${WORKSPACE}/same.mjs`,
                    id: `${WORKSPACE}/same.mjs`,
                    kind: 'file',
                },
                {
                    content: 'edited',
                    filePath: `${WORKSPACE}/edited.mjs`,
                    id: `${WORKSPACE}/edited.mjs`,
                    kind: 'file',
                },
            ]),
        );
        api.filesystem.readFile.mockImplementation(async (path: string) =>
            path.endsWith('same.mjs') ? 'same' : 'on disk',
        );

        const hookRef = renderBuffersHook();
        await flush(0);

        const dirtyByPath = Object.fromEntries(
            hookRef.current.buffers.map((b) => [
                b.kind === 'file' ? b.filePath : b.id,
                b.dirty,
            ]),
        );
        expect(dirtyByPath).toEqual({
            [`${WORKSPACE}/edited.mjs`]: true,
            [`${WORKSPACE}/same.mjs`]: false,
        });
    });
});

describe('restored buffers keep their baseline', () => {
    test('the baseline is persisted with an unsaved file buffer', async () => {
        const hookRef = renderBuffersHook();
        api.filesystem.readFile.mockResolvedValue('original');
        await act(async () => {
            await hookRef.current.openFile('a.mjs');
        });
        act(() => hookRef.current.handlePatchChange('mine'));

        const [snapshot] = JSON.parse(
            window.localStorage.getItem('modular_unsaved_buffers')!,
        );
        expect(snapshot).toMatchObject({
            content: 'mine',
            savedContent: 'original',
        });
    });

    test('edits restored over a file changed while closed still conflict', async () => {
        window.localStorage.setItem(
            'modular_unsaved_buffers',
            JSON.stringify([
                {
                    content: 'mine',
                    filePath: `${WORKSPACE}/a.mjs`,
                    id: `${WORKSPACE}/a.mjs`,
                    kind: 'file',
                    savedContent: 'original',
                },
            ]),
        );
        api.filesystem.readFile.mockResolvedValue('changed while closed');

        const hookRef = renderBuffersHook();
        await flush(0);

        const buffer = hookRef.current.buffers[0];
        expect(buffer.content).toBe('mine');
        expect(buffer.dirty).toBe(true);
        api.filesystem.writeFile.mockResolvedValue({ success: true });
        await act(async () => {
            await hookRef.current.saveFile();
        });
        expect(api.filesystem.writeFile).toHaveBeenLastCalledWith(
            `${WORKSPACE}/a.mjs`,
            'mine',
            'original',
        );
    });

    test('a restored buffer matching its file is clean', async () => {
        window.localStorage.setItem(
            'modular_unsaved_buffers',
            JSON.stringify([
                {
                    content: 'mine',
                    filePath: `${WORKSPACE}/a.mjs`,
                    id: `${WORKSPACE}/a.mjs`,
                    kind: 'file',
                    savedContent: 'original',
                },
            ]),
        );
        api.filesystem.readFile.mockResolvedValue('mine');

        const hookRef = renderBuffersHook();
        await flush(0);

        expect(hookRef.current.buffers[0].dirty).toBe(false);
    });
});

describe('save conflicts with changes on disk', () => {
    async function openEdited(hookRef: { current: Hook }) {
        api.filesystem.readFile.mockResolvedValue('original');
        await act(async () => {
            await hookRef.current.openFile('a.mjs');
        });
        act(() => hookRef.current.handlePatchChange('mine'));
    }

    test('a save sends the content last read from disk as the expected base', async () => {
        const hookRef = renderBuffersHook();
        await openEdited(hookRef);
        api.filesystem.writeFile.mockResolvedValue({ success: true });

        await act(async () => {
            await hookRef.current.saveFile();
        });

        expect(api.filesystem.writeFile).toHaveBeenCalledWith(
            `${WORKSPACE}/a.mjs`,
            'mine',
            'original',
        );
        expect(hookRef.current.saveConflict).toBeNull();
    });

    test('a refused save keeps the buffer dirty and reports the conflict', async () => {
        const hookRef = renderBuffersHook();
        await openEdited(hookRef);
        api.filesystem.writeFile.mockResolvedValue({
            conflict: true,
            success: false,
        });

        let saved: string | undefined = 'unset';
        await act(async () => {
            saved = await hookRef.current.saveFile();
        });

        expect(saved).toBeUndefined();
        expect(hookRef.current.buffers[0].dirty).toBe(true);
        expect(hookRef.current.saveConflict).toEqual({
            bufferId: `${WORKSPACE}/a.mjs`,
            filePath: `${WORKSPACE}/a.mjs`,
        });
    });

    test('overwriting writes without an expected base and marks the buffer clean', async () => {
        const hookRef = renderBuffersHook();
        await openEdited(hookRef);
        api.filesystem.writeFile.mockResolvedValueOnce({
            conflict: true,
            success: false,
        });
        await act(async () => {
            await hookRef.current.saveFile();
        });

        api.filesystem.writeFile.mockResolvedValue({ success: true });
        await act(async () => {
            await hookRef.current.overwriteOnConflict(`${WORKSPACE}/a.mjs`);
        });

        expect(api.filesystem.writeFile).toHaveBeenLastCalledWith(
            `${WORKSPACE}/a.mjs`,
            'mine',
            undefined,
        );
        expect(hookRef.current.buffers[0].dirty).toBe(false);
        expect(hookRef.current.saveConflict).toBeNull();
    });

    test('reverting replaces the buffer with the disk content and uses it as the new base', async () => {
        const hookRef = renderBuffersHook();
        await openEdited(hookRef);

        act(() =>
            hookRef.current.revertToDiskContent(`${WORKSPACE}/a.mjs`, 'theirs'),
        );
        expect(hookRef.current.buffers[0].content).toBe('theirs');
        expect(hookRef.current.buffers[0].dirty).toBe(false);

        act(() => hookRef.current.handlePatchChange('mine again'));
        api.filesystem.writeFile.mockResolvedValue({ success: true });
        await act(async () => {
            await hookRef.current.saveFile();
        });
        expect(api.filesystem.writeFile).toHaveBeenLastCalledWith(
            `${WORKSPACE}/a.mjs`,
            'mine again',
            'theirs',
        );
    });
});

describe('external changes to open files', () => {
    test('the watched set follows the open file buffers', async () => {
        const hookRef = renderBuffersHook();
        api.filesystem.readFile.mockResolvedValue('content');
        await act(async () => {
            await hookRef.current.openFile('a.mjs');
        });
        expect(api.filesystem.watchOpenFiles).toHaveBeenLastCalledWith([
            `${WORKSPACE}/a.mjs`,
        ]);

        await act(async () => {
            await hookRef.current.closeBuffer(`${WORKSPACE}/a.mjs`);
        });
        await flush();
        expect(api.filesystem.watchOpenFiles).toHaveBeenLastCalledWith([]);
    });

    test('a clean buffer reloads the new content', async () => {
        const hookRef = renderBuffersHook();
        api.filesystem.readFile.mockResolvedValue('original');
        await act(async () => {
            await hookRef.current.openFile('a.mjs');
        });

        emitFileChange(`${WORKSPACE}/a.mjs`, 'theirs');

        const buffer = hookRef.current.buffers[0];
        expect(buffer.content).toBe('theirs');
        expect(buffer.dirty).toBe(false);
        expect(buffer.kind === 'file' && buffer.savedContent).toBe('theirs');
    });

    test('a dirty buffer keeps its edits and its next save conflicts', async () => {
        const hookRef = renderBuffersHook();
        api.filesystem.readFile.mockResolvedValue('original');
        await act(async () => {
            await hookRef.current.openFile('a.mjs');
        });
        act(() => hookRef.current.handlePatchChange('mine'));

        emitFileChange(`${WORKSPACE}/a.mjs`, 'theirs');

        const buffer = hookRef.current.buffers[0];
        expect(buffer.content).toBe('mine');
        expect(buffer.dirty).toBe(true);
        api.filesystem.writeFile.mockResolvedValue({ success: true });
        await act(async () => {
            await hookRef.current.saveFile();
        });
        expect(api.filesystem.writeFile).toHaveBeenLastCalledWith(
            `${WORKSPACE}/a.mjs`,
            'mine',
            'original',
        );
    });

    test('a change matching the dirty buffer marks it clean', async () => {
        const hookRef = renderBuffersHook();
        api.filesystem.readFile.mockResolvedValue('original');
        await act(async () => {
            await hookRef.current.openFile('a.mjs');
        });
        act(() => hookRef.current.handlePatchChange('same'));

        emitFileChange(`${WORKSPACE}/a.mjs`, 'same');

        expect(hookRef.current.buffers[0].dirty).toBe(false);
    });
});

describe('closing a dirty untitled buffer with Save', () => {
    test('closes the tab under the buffer id assigned by the save', async () => {
        api.showUnsavedChangesDialog.mockResolvedValue(0);
        api.filesystem.showSaveDialog.mockResolvedValue('kept.mjs');
        api.filesystem.writeFile.mockResolvedValue({ success: true });

        const hookRef = renderBuffersHook();
        act(() => hookRef.current.createUntitledFile());
        act(() => hookRef.current.handlePatchChange('some content'));

        await act(async () => {
            await hookRef.current.closeBuffer('untitled-1');
        });
        await flush();

        expect(api.filesystem.writeFile).toHaveBeenCalledWith(
            `${WORKSPACE}/kept.mjs`,
            'some content',
        );
        expect(hookRef.current.buffers).toHaveLength(0);
        expect(hookRef.current.activeBufferId).toBeUndefined();
    });

    test('keeps the tab open when the save dialog is cancelled', async () => {
        api.showUnsavedChangesDialog.mockResolvedValue(0);
        api.filesystem.showSaveDialog.mockResolvedValue(null);

        const hookRef = renderBuffersHook();
        act(() => hookRef.current.createUntitledFile());
        act(() => hookRef.current.handlePatchChange('some content'));

        await act(async () => {
            await hookRef.current.closeBuffer('untitled-1');
        });
        await flush();

        expect(hookRef.current.buffers).toHaveLength(1);
        expect(hookRef.current.buffers[0].content).toBe('some content');
        expect(api.filesystem.writeFile).not.toHaveBeenCalled();
    });
});
