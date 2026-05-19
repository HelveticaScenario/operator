// @vitest-environment jsdom
/**
 * Tests for the Phase 2.3 keymap loader: pure merge semantics plus the
 * installer's dispatch + when-clause gating behaviour.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { KeybindingOverride } from '../../shared/ipcTypes';
import { registerCommand, unregisterCommand } from './commands';
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
    test('returns defaults verbatim when no overrides are supplied', () => {
        const merged = mergeKeymap(SAMPLE_DEFAULTS, []);
        expect(merged).toEqual([
            { key: '$mod+Enter', command: 'operator.updatePatch' },
            { key: '$mod+.', command: 'operator.stop' },
            { key: '$mod+s', command: 'operator.save' },
        ]);
    });

    test('null-command user entry removes matching default (case-insensitive)', () => {
        const overrides: KeybindingOverride[] = [
            { key: '$MOD+enter', command: null },
        ];
        const merged = mergeKeymap(SAMPLE_DEFAULTS, overrides);
        expect(merged.map((e) => e.command)).toEqual([
            'operator.stop',
            'operator.save',
        ]);
    });

    test('non-null user entry is appended after defaults', () => {
        const overrides: KeybindingOverride[] = [
            { key: '$mod+k', command: 'operator.openSettings' },
        ];
        const merged = mergeKeymap(SAMPLE_DEFAULTS, overrides);
        expect(merged.at(-1)).toEqual({
            key: '$mod+k',
            command: 'operator.openSettings',
        });
        expect(merged).toHaveLength(SAMPLE_DEFAULTS.length + 1);
    });

    test('remove + replace pattern: null entry plus new entry on same key', () => {
        const overrides: KeybindingOverride[] = [
            { key: '$mod+Enter', command: null },
            { key: '$mod+Enter', command: 'operator.stop' },
        ];
        const merged = mergeKeymap(SAMPLE_DEFAULTS, overrides);
        const enterEntries = merged.filter((e) => e.key === '$mod+Enter');
        expect(enterEntries).toEqual([
            { key: '$mod+Enter', command: 'operator.stop' },
        ]);
    });

    test('preserves when-clauses on both defaults and overrides', () => {
        const defaults: DefaultKeybinding[] = [
            {
                key: '$mod+Enter',
                command: 'operator.updatePatch',
                when: 'editorFocused',
            },
        ];
        const overrides: KeybindingOverride[] = [
            {
                key: '$mod+p',
                command: 'operator.showCommandPalette',
                when: '!inSettingsModal',
            },
        ];
        const merged = mergeKeymap(defaults, overrides);
        expect(merged).toEqual([
            {
                key: '$mod+Enter',
                command: 'operator.updatePatch',
                when: 'editorFocused',
            },
            {
                key: '$mod+p',
                command: 'operator.showCommandPalette',
                when: '!inSettingsModal',
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
        vi.restoreAllMocks();
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

    test('warns and skips dispatch when the command id is not registered', () => {
        const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
        const { dispose } = installKeymap(
            [{ key: 'a', command: 'no.such.command' }],
            target,
        );

        target.dispatchEvent(
            new KeyboardEvent('keydown', { key: 'a', bubbles: true }),
        );

        expect(warn).toHaveBeenCalled();
        dispose();
    });
});
