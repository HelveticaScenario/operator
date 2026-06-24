/**
 * Tests for VS Code -> tinykeys key translation and override normalization.
 * Cases are drawn from the shapes in a real VS Code keybindings.json.
 */
import { describe, expect, test } from 'vitest';
import {
    aliasCommand,
    authoringId,
    normalizeOverride,
    toTinykeys,
} from './vscodeKeys';

describe('toTinykeys', () => {
    test('maps VS Code modifiers to tinykeys tokens (darwin)', () => {
        expect(toTinykeys('cmd+enter', 'darwin')).toBe('Meta+Enter');
        expect(toTinykeys('ctrl+e', 'darwin')).toBe('Control+e');
        expect(toTinykeys('shift+cmd+enter', 'darwin')).toBe(
            'Shift+Meta+Enter',
        );
    });

    test('emits code-regex tokens for Alt presses and Shift+digit/symbol', () => {
        // Alt composes printable keys, so match by event.code.
        expect(toTinykeys('alt+shift+i', 'darwin')).toBe('Alt+Shift+(KeyI)');
        expect(toTinykeys('alt+[', 'darwin')).toBe('Alt+(BracketLeft)');
        // Shift composes digits/symbols (but not letters).
        expect(toTinykeys('shift+cmd+0', 'darwin')).toBe('Shift+Meta+(Digit0)');
        expect(toTinykeys('shift+p', 'darwin')).toBe('Shift+p');
        // No Alt/Shift: plain char form.
        expect(toTinykeys('cmd+k', 'darwin')).toBe('Meta+k');
    });

    test('keeps cmd and ctrl distinct (no $mod collapsing for explicit mods)', () => {
        expect(toTinykeys('cmd+e', 'darwin')).toBe('Meta+e');
        expect(toTinykeys('ctrl+e', 'darwin')).toBe('Control+e');
    });

    test('resolves $mod to the platform primary modifier', () => {
        expect(toTinykeys('$mod+Enter', 'darwin')).toBe('Meta+Enter');
        expect(toTinykeys('$mod+Enter', 'other')).toBe('Control+Enter');
    });

    test('orders modifiers canonically regardless of source order', () => {
        // Alt present -> code-regex key form.
        expect(toTinykeys('ctrl+shift+cmd+alt+i', 'darwin')).toBe(
            'Control+Alt+Shift+Meta+(KeyI)',
        );
        expect(toTinykeys('cmd+ctrl+i', 'darwin')).toBe('Control+Meta+i');
    });

    test('translates arrow keys, leaves case-insensitive names readable', () => {
        expect(toTinykeys('cmd+right', 'darwin')).toBe('Meta+ArrowRight');
        expect(toTinykeys('shift+down', 'darwin')).toBe('Shift+ArrowDown');
        expect(toTinykeys('pageup', 'darwin')).toBe('PageUp');
        expect(toTinykeys('shift+tab', 'darwin')).toBe('Shift+Tab');
        expect(toTinykeys('alt+f5', 'darwin')).toBe('Alt+F5');
    });

    test('translates each press of a chord', () => {
        expect(toTinykeys('cmd+k cmd+i', 'darwin')).toBe('Meta+k Meta+i');
        // First press has Alt -> code form; arrow stays a named key.
        expect(toTinykeys('alt+cmd+u up', 'darwin')).toBe(
            'Alt+Meta+(KeyU) ArrowUp',
        );
        expect(toTinykeys('cmd+g shift+cmd+left', 'darwin')).toBe(
            'Meta+g Shift+Meta+ArrowLeft',
        );
    });

    test('handles a bare key with no modifiers', () => {
        expect(toTinykeys('c', 'darwin')).toBe('c');
        expect(toTinykeys('F2', 'darwin')).toBe('F2');
    });

    test('lower-cases letter keys but preserves punctuation/digits', () => {
        expect(toTinykeys('cmd+1', 'darwin')).toBe('Meta+1');
        expect(toTinykeys('cmd+K', 'darwin')).toBe('Meta+k');
    });

    test('returns null for an empty or modifier-only binding', () => {
        expect(toTinykeys('', 'darwin')).toBeNull();
        expect(toTinykeys('cmd+', 'darwin')).toBeNull();
    });
});

describe('aliasCommand', () => {
    test('rewrites workbench ids Monaco re-registers as editor actions', () => {
        expect(aliasCommand('workbench.action.gotoLine')).toBe(
            'editor.action.gotoLine',
        );
        expect(aliasCommand('workbench.action.gotoSymbol')).toBe(
            'editor.action.quickOutline',
        );
    });

    test('rewrites workbench ids Operator owns as registry commands', () => {
        expect(aliasCommand('workbench.action.files.save')).toBe(
            'operator.save',
        );
        expect(aliasCommand('workbench.action.showCommands')).toBe(
            'operator.showCommandPalette',
        );
        expect(aliasCommand('workbench.action.files.openFolder')).toBe(
            'operator.openWorkspace',
        );
    });

    test('is identity for shared editor ids, operator-native ids, and unknown ids', () => {
        // Shared with VS Code verbatim — dispatches to Monaco as-is.
        expect(aliasCommand('editor.action.rename')).toBe(
            'editor.action.rename',
        );
        // Operator-native — already in dispatch form.
        expect(aliasCommand('operator.updatePatch')).toBe(
            'operator.updatePatch',
        );
        // Feature-absent in Operator — passes through untouched.
        expect(aliasCommand('workbench.action.showAllSymbols')).toBe(
            'workbench.action.showAllSymbols',
        );
    });
});

describe('authoringId', () => {
    test('offers the VS Code id for an aliased dispatch id', () => {
        expect(authoringId('editor.action.gotoLine')).toBe(
            'workbench.action.gotoLine',
        );
        expect(authoringId('editor.action.quickOutline')).toBe(
            'workbench.action.gotoSymbol',
        );
        expect(authoringId('operator.save')).toBe(
            'workbench.action.files.save',
        );
        expect(authoringId('operator.showCommandPalette')).toBe(
            'workbench.action.showCommands',
        );
    });

    test('picks the first-listed VS Code id when several alias to one command', () => {
        // openKeybindings: the File (JSON) variant is listed first.
        expect(authoringId('operator.openKeybindings')).toBe(
            'workbench.action.openGlobalKeybindingsFile',
        );
        // openWorkspace: the Open Folder id is listed first.
        expect(authoringId('operator.openWorkspace')).toBe(
            'workbench.action.files.openFolder',
        );
    });

    test('is identity for shared editor ids and operator-native ids', () => {
        expect(authoringId('editor.action.rename')).toBe(
            'editor.action.rename',
        );
        expect(authoringId('operator.updatePatch')).toBe(
            'operator.updatePatch',
        );
    });

    test('round-trips back to the dispatch id through aliasCommand', () => {
        for (const dispatchId of [
            'editor.action.gotoLine',
            'editor.action.quickOutline',
            'operator.save',
            'operator.openWorkspace',
            'operator.openKeybindings',
        ]) {
            expect(aliasCommand(authoringId(dispatchId))).toBe(dispatchId);
        }
    });
});

describe('normalizeOverride', () => {
    test('a plain entry becomes a bind with translated key', () => {
        // showAllSymbols has no Operator equivalent, so the id passes through.
        expect(
            normalizeOverride(
                { key: 'cmd+e', command: 'workbench.action.showAllSymbols' },
                'darwin',
            ),
        ).toEqual({
            type: 'bind',
            key: 'Meta+e',
            command: 'workbench.action.showAllSymbols',
        });
    });

    test('aliases an imported VS Code command to its dispatch id', () => {
        expect(
            normalizeOverride(
                { key: 'cmd+shift+s', command: 'workbench.action.files.save' },
                'darwin',
            ),
        ).toEqual({
            type: 'bind',
            key: 'Shift+Meta+s',
            command: 'operator.save',
        });
    });

    test('aliases a -prefixed VS Code removal to the dispatch id', () => {
        // So `-workbench.action.gotoLine` cancels a default authored as the
        // same VS Code id (which also resolves to editor.action.gotoLine).
        expect(
            normalizeOverride(
                { key: 'ctrl+g', command: '-workbench.action.gotoLine' },
                'darwin',
            ),
        ).toEqual({
            type: 'remove',
            key: 'Control+g',
            command: 'editor.action.gotoLine',
        });
    });

    test('a -prefixed command becomes a removal for that command', () => {
        expect(
            normalizeOverride(
                { key: 'ctrl+e', command: '-cursorLineEnd' },
                'darwin',
            ),
        ).toEqual({ type: 'remove', key: 'Control+e', command: 'cursorLineEnd' });
    });

    test('a null command becomes a key-wide removal', () => {
        expect(
            normalizeOverride({ key: '$mod+Enter', command: null }, 'darwin'),
        ).toEqual({ type: 'remove', key: 'Meta+Enter', command: null });
    });

    test('preserves when and object args on a bind', () => {
        expect(
            normalizeOverride(
                {
                    key: 'alt+shift+i',
                    command: 'editor.action.insertSnippet',
                    args: { snippet: '$CURSOR_NUMBER' },
                    when: 'editorTextFocus',
                },
                'darwin',
            ),
        ).toEqual({
            type: 'bind',
            key: 'Alt+Shift+(KeyI)',
            command: 'editor.action.insertSnippet',
            args: { snippet: '$CURSOR_NUMBER' },
            when: 'editorTextFocus',
        });
    });

    test('drops an entry whose key cannot be translated', () => {
        expect(
            normalizeOverride({ key: 'cmd+', command: 'whatever' }, 'darwin'),
        ).toBeNull();
    });
});
