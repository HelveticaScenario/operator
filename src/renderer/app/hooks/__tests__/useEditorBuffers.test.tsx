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
 */

import { act, createElement, useEffect } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

const api = vi.hoisted(() => ({
    filesystem: {
        deleteFile: vi.fn(),
        readFile: vi.fn(),
        renameFile: vi.fn(),
        showSaveDialog: vi.fn(),
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
});

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

        let savePromise: Promise<void> | undefined;
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

        let savePromise: Promise<void> | undefined;
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
