/**
 * Tests for the pure keybindings-completion context detector. Drives the
 * decision of whether the cursor is in a `command` value, a `when` value, or
 * neither, and what partial token to complete.
 */
import { describe, expect, test } from 'vitest';
import { detectKeybindingCompletion, knownCommands } from './keybindingsCompletion';
import { registerCommand, unregisterCommand } from '../../keybindings/commands';

describe('detectKeybindingCompletion', () => {
    test('detects an empty command value', () => {
        expect(detectKeybindingCompletion('    "command": "')).toEqual({
            kind: 'command',
            word: '',
        });
    });

    test('detects a partial command id', () => {
        expect(
            detectKeybindingCompletion('    "command": "editor.act'),
        ).toEqual({ kind: 'command', word: 'editor.act' });
    });

    test('ignores the leading - removal marker in the command word', () => {
        expect(detectKeybindingCompletion('  "command": "-operator.s')).toEqual({
            kind: 'command',
            word: 'operator.s',
        });
    });

    test('detects an empty when value', () => {
        expect(detectKeybindingCompletion('    "when": "')).toEqual({
            kind: 'when',
            word: '',
        });
    });

    test('detects the trailing identifier of a when expression', () => {
        expect(
            detectKeybindingCompletion('  "when": "editorTextFocus && !editorR'),
        ).toEqual({ kind: 'when', word: 'editorR' });
    });

    test('when word resets after an operator + space', () => {
        expect(
            detectKeybindingCompletion('  "when": "editorTextFocus && '),
        ).toEqual({ kind: 'when', word: '' });
    });

    test('returns null once the command value string is closed', () => {
        expect(
            detectKeybindingCompletion('  "command": "operator.save",'),
        ).toBeNull();
    });

    test('returns null on an unrelated line (e.g. key)', () => {
        expect(detectKeybindingCompletion('  "key": "cmd+')).toBeNull();
    });

    test('returns null on an empty / structural line', () => {
        expect(detectKeybindingCompletion('  {')).toBeNull();
    });
});

describe('knownCommands', () => {
    test('offers an aliased registry command under its VS Code id', () => {
        registerCommand('operator.save', () => {}, { label: 'Save' });
        try {
            const commands = knownCommands();
            expect(commands).toContainEqual({
                id: 'workbench.action.files.save',
                label: 'Save',
            });
            expect(commands.some((c) => c.id === 'operator.save')).toBe(false);
        } finally {
            unregisterCommand('operator.save');
        }
    });

    test('rewrites a catalog id VS Code names differently, leaves shared ids', () => {
        const commands = knownCommands();
        // editor.action.quickOutline -> workbench.action.gotoSymbol.
        expect(commands.some((c) => c.id === 'workbench.action.gotoSymbol')).toBe(
            true,
        );
        expect(commands.some((c) => c.id === 'editor.action.quickOutline')).toBe(
            false,
        );
        // A shared editor id keeps its VS Code-identical id.
        expect(
            commands.some((c) => c.id === 'editor.action.revealDefinition'),
        ).toBe(true);
    });
});
