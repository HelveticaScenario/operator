/**
 * Build the unified command-palette item list shown by `CommandPalette`.
 *
 * Sources merged each time the palette opens:
 *   1. Every entry in the Operator command registry (label + category).
 *   2. When a Monaco editor instance exists, every action returned by
 *      `editor.getSupportedActions()` that reports `isSupported() === true`.
 *
 * Monaco actions get a fixed `"Editor"` category badge so they sort together
 * in the palette UI and so users can quickly tell them apart from Operator
 * commands.
 *
 * See `~/.claude/plans/operator-is-at-its-goofy-mist.md` Phase 2.1a.
 */
import type { editor } from 'monaco-editor';

import {
    executeCommand,
    listCommands,
    type CommandMetadata,
} from './commands';
import { getCommandBinding } from './keymap';

/**
 * A single row in the cmdk palette. `kind` discriminates between the two
 * dispatch paths so the consumer doesn't need to know about Monaco actions
 * directly.
 */
export type PaletteItem =
    | {
          kind: 'command';
          id: string;
          label: string;
          category?: string;
          /** Resolved tinykeys binding, if any (chord-capable, e.g. `Meta+k Meta+i`). */
          keybinding?: string;
          when?: string;
          run: () => void;
      }
    | {
          kind: 'editor-action';
          id: string;
          label: string;
          category: 'Editor';
          keybinding?: string;
          run: () => void;
      };

function operatorItem(
    id: string,
    metadata: CommandMetadata | undefined,
): PaletteItem {
    return {
        kind: 'command',
        id,
        label: metadata?.label ?? id,
        category: metadata?.category,
        keybinding: getCommandBinding(id),
        when: metadata?.when,
        run: () => {
            executeCommand(id);
        },
    };
}

function editorActionItem(
    activeEditor: editor.ICodeEditor,
    action: editor.IEditorAction,
): PaletteItem | null {
    if (!action.isSupported()) {
        return null;
    }
    const label = action.label?.trim() || action.id;
    return {
        kind: 'editor-action',
        id: action.id,
        label,
        category: 'Editor',
        keybinding: getCommandBinding(action.id),
        run: () => {
            // Some actions (e.g. Go to Line) open their own focused widget,
            // which needs the editor to hold focus when the action runs — the
            // palette had focus until it closed.
            activeEditor.focus();
            void action.run();
        },
    };
}

/**
 * Build the merged item list. Pass the current editor instance (if any) so
 * Monaco's editor actions can be enumerated; with zero buffers open and
 * therefore no editor, the result contains only Operator commands.
 */
export function buildPaletteItems(
    activeEditor: editor.ICodeEditor | null,
): PaletteItem[] {
    const items: PaletteItem[] = [];

    for (const { id, metadata } of listCommands()) {
        items.push(operatorItem(id, metadata));
    }

    if (activeEditor) {
        for (const action of activeEditor.getSupportedActions()) {
            const item = editorActionItem(activeEditor, action);
            if (item) {
                items.push(item);
            }
        }
    }

    return items;
}
