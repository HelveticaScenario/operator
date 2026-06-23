// @vitest-environment jsdom
/**
 * Tests for the keymap loader: pure merge semantics plus the installer's
 * dispatch + when-clause gating behaviour.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { KeybindingOverride } from '../../shared/ipcTypes';
import { registerCommand, unregisterCommand } from './commands';
import { setActiveEditor } from './dispatch';
import type { editor } from 'monaco-editor';
import type { DefaultKeybinding } from './defaultKeymap';
import {
    installKeymap,
    mergeKeymap,
    setWhenEvaluator,
} from './keymap';

const SAMPLE_DEFAULTS: DefaultKeybinding[] = [
    { key: '$mod+Enter', command: 'operator.updatePatch' },
    { key: '$mod+.', command: 'operator.stop' },
    { key: '$mod+s', command: 'operator.save' },
];

describe('mergeKeymap', () => {
    test('canonicalizes default keys ($mod -> platform modifier) on darwin', () => {
        const merged = mergeKeymap(SAMPLE_DEFAULTS, [], 'darwin');
        expect(merged).toEqual([
            { key: 'Meta+Enter', command: 'operator.updatePatch' },
            { key: 'Meta+.', command: 'operator.stop' },
            { key: 'Meta+s', command: 'operator.save' },
        ]);
    });

    test('$mod resolves to Control off darwin', () => {
        const merged = mergeKeymap(SAMPLE_DEFAULTS, [], 'other');
        expect(merged.map((e) => e.key)).toEqual([
            'Control+Enter',
            'Control+.',
            'Control+s',
        ]);
    });

    test('null-command user entry removes the matching default', () => {
        const overrides: KeybindingOverride[] = [
            { key: '$MOD+enter', command: null },
        ];
        const merged = mergeKeymap(SAMPLE_DEFAULTS, overrides, 'darwin');
        expect(merged.map((e) => e.command)).toEqual([
            'operator.stop',
            'operator.save',
        ]);
    });

    test('a VS Code key collapses onto the same canonical chord as a $mod default', () => {
        // `cmd+enter` (VS Code) and `$mod+Enter` (default) are one chord on
        // darwin, so the `-` removal cancels the default.
        const overrides: KeybindingOverride[] = [
            { key: 'cmd+enter', command: '-operator.updatePatch' },
        ];
        const merged = mergeKeymap(SAMPLE_DEFAULTS, overrides, 'darwin');
        expect(merged.some((e) => e.command === 'operator.updatePatch')).toBe(
            false,
        );
    });

    test('non-null user entry is appended after defaults (key translated)', () => {
        const overrides: KeybindingOverride[] = [
            { key: 'cmd+k', command: 'operator.openSettings' },
        ];
        const merged = mergeKeymap(SAMPLE_DEFAULTS, overrides, 'darwin');
        expect(merged.at(-1)).toEqual({
            key: 'Meta+k',
            command: 'operator.openSettings',
        });
        expect(merged).toHaveLength(SAMPLE_DEFAULTS.length + 1);
    });

    test('uses the mac override on darwin and the key elsewhere', () => {
        const defaults: DefaultKeybinding[] = [
            { key: '$mod+shift+[', mac: '$mod+alt+[', command: 'editor.fold' },
        ];
        // Alt / Shift compose punctuation, so the key matches by event.code.
        expect(mergeKeymap(defaults, [], 'darwin')[0].key).toBe(
            'Alt+Meta+(BracketLeft)',
        );
        expect(mergeKeymap(defaults, [], 'other')[0].key).toBe(
            'Control+Shift+(BracketLeft)',
        );
    });

    test('an empty key with a mac override binds only on darwin', () => {
        const defaults: DefaultKeybinding[] = [
            { key: '', mac: 'ctrl+t', command: 'editor.action.transposeLetters' },
        ];
        expect(mergeKeymap(defaults, [], 'darwin')).toEqual([
            { key: 'Control+t', command: 'editor.action.transposeLetters' },
        ]);
        expect(mergeKeymap(defaults, [], 'other')).toEqual([]);
    });

    test('a mac-only line-insert default never shadows the Ctrl transport key off darwin', () => {
        const defaults: DefaultKeybinding[] = [
            { key: 'Control+Enter', command: 'operator.updatePatch' },
            {
                key: '',
                mac: '$mod+enter',
                command: 'editor.action.insertLineAfter',
                when: '!editorReadonly && editorTextFocus',
            },
        ];
        // Non-darwin: the line-insert default is dropped, so Ctrl+Enter is
        // Update Patch only.
        expect(
            mergeKeymap(defaults, [], 'other').map((e) => e.command),
        ).toEqual(['operator.updatePatch']);
        // darwin: both bound, on distinct physical keys.
        const mac = Object.fromEntries(
            mergeKeymap(defaults, [], 'darwin').map((e) => [e.key, e.command]),
        );
        expect(mac['Control+Enter']).toBe('operator.updatePatch');
        expect(mac['Meta+Enter']).toBe('editor.action.insertLineAfter');
    });

    test('order-sensitive removal: a later -command cancels an earlier bind', () => {
        const overrides: KeybindingOverride[] = [
            { key: 'cmd+e', command: 'editor.action.showHover' },
            { key: 'cmd+e', command: '-editor.action.showHover' },
        ];
        const merged = mergeKeymap(SAMPLE_DEFAULTS, overrides, 'darwin');
        expect(merged.some((e) => e.key === 'Meta+e')).toBe(false);
    });

    test('remove + replace pattern leaves only the replacement', () => {
        const overrides: KeybindingOverride[] = [
            { key: '$mod+Enter', command: null },
            { key: '$mod+Enter', command: 'operator.stop' },
        ];
        const merged = mergeKeymap(SAMPLE_DEFAULTS, overrides, 'darwin');
        const enterEntries = merged.filter((e) => e.key === 'Meta+Enter');
        expect(enterEntries).toEqual([
            { key: 'Meta+Enter', command: 'operator.stop' },
        ]);
    });

    test('preserves when-clauses and args on both defaults and overrides', () => {
        const defaults: DefaultKeybinding[] = [
            {
                key: '$mod+Enter',
                command: 'operator.updatePatch',
                when: 'editorFocused',
            },
        ];
        const overrides: KeybindingOverride[] = [
            {
                key: 'alt+shift+i',
                command: 'editor.action.insertSnippet',
                args: { snippet: '$CURSOR' },
                when: 'editorTextFocus',
            },
        ];
        const merged = mergeKeymap(defaults, overrides, 'darwin');
        expect(merged).toEqual([
            {
                key: 'Meta+Enter',
                command: 'operator.updatePatch',
                when: 'editorFocused',
            },
            {
                key: 'Alt+Shift+(KeyI)',
                command: 'editor.action.insertSnippet',
                args: { snippet: '$CURSOR' },
                when: 'editorTextFocus',
            },
        ]);
    });
});

describe('installKeymap', () => {
    const cmdId = 'test.keymap.fire';
    let target: HTMLDivElement;

    beforeEach(() => {
        target = document.createElement('div');
        // tinykeys' DOM-target path requires the node to dispatch events
        // through itself; appending to the body is not necessary but keeps
        // event behaviour realistic.
        document.body.appendChild(target);
        setWhenEvaluator(() => true);
    });

    afterEach(() => {
        unregisterCommand(cmdId);
        target.remove();
        setWhenEvaluator(() => true);
        setActiveEditor(null);
        vi.restoreAllMocks();
    });

    function fakeEditor(focused: boolean, trigger: () => void) {
        return {
            hasTextFocus: () => focused,
            trigger,
        } as unknown as editor.ICodeEditor;
    }

    test('editor command is not dispatched (event not consumed) when the editor lacks focus', () => {
        const trigger = vi.fn();
        setActiveEditor(fakeEditor(false, trigger));
        const { dispose } = installKeymap(
            [{ key: 'a', command: 'editor.action.fooBar' }],
            target,
        );

        const event = new KeyboardEvent('keydown', {
            key: 'a',
            bubbles: true,
            cancelable: true,
        });
        target.dispatchEvent(event);

        expect(trigger).not.toHaveBeenCalled();
        expect(event.defaultPrevented).toBe(false);
        dispose();
    });

    test('editor command dispatches to the focused editor and consumes the event', () => {
        const trigger = vi.fn();
        setActiveEditor(fakeEditor(true, trigger));
        const { dispose } = installKeymap(
            [{ key: 'a', command: 'editor.action.fooBar' }],
            target,
        );

        const event = new KeyboardEvent('keydown', {
            key: 'a',
            bubbles: true,
            cancelable: true,
        });
        target.dispatchEvent(event);

        expect(trigger).toHaveBeenCalledWith(
            'operator.keybinding',
            'editor.action.fooBar',
            undefined,
        );
        expect(event.defaultPrevented).toBe(true);
        dispose();
    });

    test('dispatches the bound command on a matching keydown', () => {
        const handler = vi.fn();
        registerCommand(cmdId, handler, { label: 'Fire' });
        const { dispose } = installKeymap(
            [{ key: 'a', command: cmdId }],
            target,
        );

        target.dispatchEvent(
            new KeyboardEvent('keydown', { key: 'a', bubbles: true }),
        );

        expect(handler).toHaveBeenCalledTimes(1);
        dispose();
    });

    test('skips dispatch when the when-clause evaluator rejects the entry', () => {
        const handler = vi.fn();
        registerCommand(cmdId, handler, { label: 'Fire' });
        setWhenEvaluator((when) => when !== 'never');
        const { dispose } = installKeymap(
            [{ key: 'a', command: cmdId, when: 'never' }],
            target,
        );

        target.dispatchEvent(
            new KeyboardEvent('keydown', { key: 'a', bubbles: true }),
        );

        expect(handler).not.toHaveBeenCalled();
        dispose();
    });

    test('later (override) entries shadow earlier (default) entries on the same key', () => {
        const a = vi.fn();
        const b = vi.fn();
        registerCommand('test.keymap.fire', a, { label: 'A' });
        registerCommand('test.keymap.fire.b', b, { label: 'B' });
        const { dispose } = installKeymap(
            [
                { key: 'a', command: 'test.keymap.fire' },
                { key: 'a', command: 'test.keymap.fire.b' },
            ],
            target,
        );

        target.dispatchEvent(
            new KeyboardEvent('keydown', { key: 'a', bubbles: true }),
        );

        expect(b).toHaveBeenCalledTimes(1);
        expect(a).not.toHaveBeenCalled();
        unregisterCommand('test.keymap.fire.b');
        dispose();
    });

    test('an unknown command with no active editor is a no-op (event not consumed)', () => {
        const { dispose } = installKeymap(
            [{ key: 'a', command: 'no.such.command' }],
            target,
        );

        const event = new KeyboardEvent('keydown', {
            key: 'a',
            bubbles: true,
            cancelable: true,
        });
        target.dispatchEvent(event);

        // Nothing dispatched it, so the keymap leaves the event alone for
        // other listeners rather than preventing its default.
        expect(event.defaultPrevented).toBe(false);
        dispose();
    });
});
