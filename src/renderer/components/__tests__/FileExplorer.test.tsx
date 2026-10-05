// @vitest-environment jsdom

/**
 * Invariant: renamingPath is an absolute path (like buffer identity), while
 * workspace file-tree entries carry workspace-relative paths. The tree must
 * resolve entries to absolute paths before comparing, so renaming a file that
 * is not open in any buffer shows the inline rename input on its tree row and
 * commits under the absolute path.
 */

import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

vi.mock('../FileExplorer.css', () => ({}));
vi.mock('../../electronAPI', () => ({
    default: { showContextMenu: vi.fn() },
}));
vi.mock('../../keybindings/contextKeyBootstrap', () => ({
    bindFileExplorerFocus: () => () => {},
}));

import { FileExplorer } from '../FileExplorer';
import type { FileTreeEntry } from '../../../shared/ipcTypes';

const WORKSPACE = '/workspace';

const FILE_TREE: FileTreeEntry[] = [
    { fileType: 'js', name: 'song.mjs', path: 'song.mjs', type: 'file' },
];

let root: Root | null = null;
let container: HTMLElement | null = null;

function renderExplorer(props: {
    renamingPath: string | null;
    onRenameCommit?: (path: string, newName: string) => void;
}) {
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    act(() => {
        root!.render(
            createElement(FileExplorer, {
                activeBufferId: undefined,
                buffers: [],
                fileTree: FILE_TREE,
                formatLabel: () => '',
                onCloseBuffer: () => {},
                onCreateFile: () => {},
                onDeleteFile: () => {},
                onKeepBuffer: () => {},
                onOpenFile: () => {},
                onRefreshTree: () => {},
                onRenameCancel: () => {},
                onRenameCommit: props.onRenameCommit ?? (() => {}),
                onRenameFile: () => {},
                onSaveFile: () => {},
                onSelectBuffer: () => {},
                onSelectWorkspace: () => {},
                renamingPath: props.renamingPath,
                runningBufferId: null,
                workspaceRoot: WORKSPACE,
            }),
        );
    });
}

beforeEach(() => {
    (
        globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    window.localStorage.clear();
});

afterEach(() => {
    act(() => root?.unmount());
    root = null;
    container?.remove();
    container = null;
});

describe('workspace-tree rename', () => {
    test('an unopened file whose absolute path is being renamed shows the rename input', () => {
        renderExplorer({ renamingPath: `${WORKSPACE}/song.mjs` });

        const input = container!.querySelector<HTMLInputElement>(
            '.file-tree .rename-input',
        );
        expect(input).not.toBeNull();
        expect(input!.defaultValue).toBe('song.mjs');
    });

    test('no rename input appears while nothing is being renamed', () => {
        renderExplorer({ renamingPath: null });
        expect(container!.querySelector('.rename-input')).toBeNull();
    });

    test('committing the rename passes the file’s absolute path', () => {
        const onRenameCommit = vi.fn();
        renderExplorer({
            onRenameCommit,
            renamingPath: `${WORKSPACE}/song.mjs`,
        });

        const input = container!.querySelector<HTMLInputElement>(
            '.file-tree .rename-input',
        )!;
        input.value = 'renamed.mjs';
        act(() => {
            input.dispatchEvent(
                new KeyboardEvent('keydown', { bubbles: true, key: 'Enter' }),
            );
        });

        expect(onRenameCommit).toHaveBeenCalledWith(
            `${WORKSPACE}/song.mjs`,
            'renamed.mjs',
        );
    });
});
