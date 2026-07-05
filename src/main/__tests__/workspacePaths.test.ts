import * as path from 'path';
import { describe, expect, test } from 'vitest';
import { resolveWorkspacePath } from '../workspacePaths';

const ROOT = path.resolve('/Users/me/workspace');

describe('resolveWorkspacePath', () => {
    test('resolves relative paths inside the workspace', () => {
        expect(resolveWorkspacePath(ROOT, 'songs/a.mjs')).toBe(
            path.join(ROOT, 'songs', 'a.mjs'),
        );
        expect(resolveWorkspacePath(ROOT, '.')).toBe(ROOT);
    });

    test('rejects relative paths that escape via ..', () => {
        expect(resolveWorkspacePath(ROOT, '../../.zshrc')).toBeNull();
        expect(resolveWorkspacePath(ROOT, 'songs/../../outside')).toBeNull();
        expect(resolveWorkspacePath(ROOT, '..')).toBeNull();
    });

    test('accepts absolute paths inside the workspace', () => {
        expect(resolveWorkspacePath(ROOT, path.join(ROOT, 'a.mjs'))).toBe(
            path.join(ROOT, 'a.mjs'),
        );
    });

    test('rejects absolute paths outside the workspace', () => {
        expect(
            resolveWorkspacePath(ROOT, '/Users/me/.ssh/id_ed25519'),
        ).toBeNull();
        expect(
            resolveWorkspacePath(ROOT, path.join(ROOT, '..', 'sibling')),
        ).toBeNull();
    });

    test('rejects absolute paths that escape via embedded ..', () => {
        expect(
            resolveWorkspacePath(
                ROOT,
                path.join(ROOT, 'songs', '..', '..', 'etc'),
            ),
        ).toBeNull();
    });

    test('does not treat a ..-prefixed sibling directory as contained', () => {
        // "/Users/me/workspaceX" shares the workspace prefix as a string but
        // is a different directory.
        expect(resolveWorkspacePath(ROOT, `${ROOT}X/file.mjs`)).toBeNull();
    });

    test('rejects everything without a workspace', () => {
        expect(resolveWorkspacePath(null, 'a.mjs')).toBeNull();
        expect(resolveWorkspacePath(null, '/anywhere/a.mjs')).toBeNull();
    });

    test('allowlisted files pass regardless of workspace containment', () => {
        const keybindings = path.resolve('/Users/me/userData/keybindings.json');
        expect(resolveWorkspacePath(ROOT, keybindings, [keybindings])).toBe(
            keybindings,
        );
        expect(resolveWorkspacePath(null, keybindings, [keybindings])).toBe(
            keybindings,
        );
        // The allowlist is exact: neighbours of an allowed file stay rejected.
        expect(
            resolveWorkspacePath(
                ROOT,
                path.resolve('/Users/me/userData/other.json'),
                [keybindings],
            ),
        ).toBeNull();
    });
});
