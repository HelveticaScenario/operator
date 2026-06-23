/**
 * Tests for the editor context-menu builder. The command registry and the
 * context-key service are global singletons, so each test cleans up after
 * itself. The editor is stubbed to a minimal `getAction`.
 */
import { afterEach, describe, expect, test } from 'vitest';
import type { editor } from 'monaco-editor';

import { buildEditorMenuItems } from './editorMenuItems';
import { registerCommand, unregisterCommand } from './commands';
import { contextKeys } from './contextKey';

const TEST_IDS = ['test.ctx.update', 'test.ctx.palette', 'test.ctx.when'];

afterEach(() => {
    for (const id of TEST_IDS) {
        unregisterCommand(id);
    }
    contextKeys.reset();
});

/**
 * Stub editor whose `getAction` reports every requested action as supported
 * with a label derived from the id, unless an override map says otherwise.
 */
function fakeEditor(
    overrides: Record<string, { label?: string; supported?: boolean } | null> = {},
): editor.ICodeEditor {
    return {
        getAction: (id: string) => {
            if (id in overrides && overrides[id] === null) {
                return null;
            }
            const o = overrides[id] ?? {};
            return {
                id,
                label: o.label ?? id,
                isSupported: () => o.supported ?? true,
                run: () => Promise.resolve(),
            };
        },
    } as unknown as editor.ICodeEditor;
}

function commandIds(items: ReturnType<typeof buildEditorMenuItems>): string[] {
    return items
        .filter((i) => i && i.kind === 'command')
        .map((i) => (i as { commandId: string }).commandId);
}

describe('buildEditorMenuItems', () => {
    test('includes navigation, modification, a Peek submenu, and clipboard roles', () => {
        const items = buildEditorMenuItems(fakeEditor());
        const ids = commandIds(items);
        expect(ids).toContain('editor.action.revealDefinition');
        expect(ids).toContain('editor.action.goToReferences');
        expect(ids).toContain('editor.action.quickOutline');
        expect(ids).toContain('editor.action.rename');
        expect(ids).toContain('editor.action.formatDocument');

        const submenu = items.find((i) => i && i.kind === 'submenu');
        expect(submenu).toBeDefined();
        if (!submenu || submenu.kind !== 'submenu') {
            throw new Error('expected a Peek submenu');
        }
        expect(submenu.label).toBe('Peek');
        expect(
            submenu.items.map((i) => (i as { commandId: string }).commandId),
        ).toEqual([
            'editor.action.peekDefinition',
            'editor.action.referenceSearch.trigger',
        ]);

        const roles = items.filter((i) => i && i.kind === 'role');
        expect(roles).toHaveLength(3);
    });

    test('reflects label and enabled state from the live editor action', () => {
        const items = buildEditorMenuItems(
            fakeEditor({
                'editor.action.rename': { label: 'Rename Symbol', supported: false },
            }),
        );
        const rename = items.find(
            (i) => i && i.kind === 'command' && i.commandId === 'editor.action.rename',
        ) as { label: string; enabled: boolean };
        expect(rename.label).toBe('Rename Symbol');
        expect(rename.enabled).toBe(false);
    });

    test('gates a registerAction2 nav item (getAction null) on its provider context key', () => {
        const find = (items: ReturnType<typeof buildEditorMenuItems>) =>
            items.find(
                (i) =>
                    i &&
                    i.kind === 'command' &&
                    i.commandId === 'editor.action.quickOutline',
            ) as { label: string; enabled: boolean };

        // getAction returns null for quickOutline; enabled follows the
        // editorHasDocumentSymbolProvider context key.
        const stub = fakeEditor({ 'editor.action.quickOutline': null });

        contextKeys.set('editorHasDocumentSymbolProvider', false);
        const disabled = find(buildEditorMenuItems(stub));
        expect(disabled.label).toBe('Go to Symbol...');
        expect(disabled.enabled).toBe(false);

        contextKeys.set('editorHasDocumentSymbolProvider', true);
        expect(find(buildEditorMenuItems(stub)).enabled).toBe(true);
    });

    test('places registry commands by group: patch first, palette last', () => {
        registerCommand('test.ctx.update', () => {}, {
            label: 'Update Patch',
            contextMenu: { group: '1_patch', order: 1 },
        });
        registerCommand('test.ctx.palette', () => {}, {
            label: 'Command Palette',
            contextMenu: { group: 'z_commands', order: 1 },
        });

        const items = buildEditorMenuItems(fakeEditor());
        const ids = commandIds(items);
        expect(ids[0]).toBe('test.ctx.update');
        expect(ids.at(-1)).toBe('test.ctx.palette');
    });

    test('filters a registry command whose when-clause is false', () => {
        registerCommand('test.ctx.when', () => {}, {
            label: 'Focused Only',
            when: 'editorFocused',
            contextMenu: { group: '1_patch', order: 1 },
        });

        contextKeys.set('editorFocused', false);
        expect(commandIds(buildEditorMenuItems(fakeEditor()))).not.toContain(
            'test.ctx.when',
        );

        contextKeys.set('editorFocused', true);
        expect(commandIds(buildEditorMenuItems(fakeEditor()))).toContain(
            'test.ctx.when',
        );
    });
});
