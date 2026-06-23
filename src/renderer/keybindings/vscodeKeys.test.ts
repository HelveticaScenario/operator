/**
 * Tests for VS Code -> tinykeys key translation and override normalization.
 * Cases are drawn from the shapes in a real VS Code keybindings.json.
 */
import { describe, expect, test } from 'vitest';
import { normalizeOverride, toTinykeys } from './vscodeKeys';

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

describe('normalizeOverride', () => {
    test('a plain entry becomes a bind with translated key', () => {
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
