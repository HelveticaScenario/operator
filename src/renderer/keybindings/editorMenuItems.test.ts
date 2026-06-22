/**
 * Tests for the editor context-menu builder. Like the command registry it
 * reads from, the registry + context-key service are global singletons, so
 * each test cleans up after itself.
 */
import { afterEach, describe, expect, test } from 'vitest';

import { buildEditorMenuItems } from './editorMenuItems';
import { registerCommand, unregisterCommand } from './commands';
import { contextKeys } from './contextKey';

const TEST_IDS = ['test.ctx.update', 'test.ctx.stop', 'test.ctx.when', 'test.ctx.plain'];

const CLIPBOARD = [
    { kind: 'role', role: 'cut' },
    { kind: 'role', role: 'copy' },
    { kind: 'role', role: 'paste' },
];

afterEach(() => {
    for (const id of TEST_IDS) {
        unregisterCommand(id);
    }
    contextKeys.reset();
});

describe('buildEditorMenuItems', () => {
    test('always emits the clipboard roles, even with no registry commands', () => {
        expect(buildEditorMenuItems()).toEqual(CLIPBOARD);
    });

    test('groups registry commands then clipboard roles with a separator between', () => {
        // Registered out of order to exercise the sort.
        registerCommand('test.ctx.stop', () => {}, {
            label: 'Stop',
            contextMenu: { group: '1_patch', order: 2 },
        });
        registerCommand('test.ctx.update', () => {}, {
            label: 'Update',
            contextMenu: { group: '1_patch', order: 1 },
        });

        expect(buildEditorMenuItems()).toEqual([
            { kind: 'command', commandId: 'test.ctx.update', label: 'Update', enabled: true },
            { kind: 'command', commandId: 'test.ctx.stop', label: 'Stop', enabled: true },
            null,
            ...CLIPBOARD,
        ]);
    });

    test('omits registry commands without a contextMenu placement', () => {
        registerCommand('test.ctx.plain', () => {}, { label: 'Plain' });

        // Only the clipboard group remains, so there is no separator.
        expect(buildEditorMenuItems()).toEqual(CLIPBOARD);
    });

    test('filters out commands whose when-clause is false, includes them when true', () => {
        registerCommand('test.ctx.when', () => {}, {
            label: 'Focused Only',
            when: 'editorFocused',
            contextMenu: { group: '1_patch', order: 1 },
        });

        contextKeys.set('editorFocused', false);
        expect(buildEditorMenuItems()).toEqual(CLIPBOARD);

        contextKeys.set('editorFocused', true);
        expect(buildEditorMenuItems()).toEqual([
            { kind: 'command', commandId: 'test.ctx.when', label: 'Focused Only', enabled: true },
            null,
            ...CLIPBOARD,
        ]);
    });
});
