/**
 * Universal command dispatch shared by the keymap and the editor context
 * menu. A command id resolves in one of two ways:
 *
 *  1. An Operator registry command (`operator.*`) → run via `executeCommand`.
 *  2. Otherwise a Monaco command — a core editing command (`cursorDown`) or
 *     an editor action (`editor.action.*`) — dispatched to the active editor
 *     via `editor.trigger`, which handles both kinds plus clipboard.
 *
 * The active editor is tracked here (set by `MonacoPatchEditor` on mount /
 * change) so window-level keybindings can reach it without prop drilling.
 */
import type { editor } from 'monaco-editor';

import { executeCommand, getCommand } from './commands';

let activeEditor: editor.ICodeEditor | null = null;

export function setActiveEditor(ed: editor.ICodeEditor | null): void {
    activeEditor = ed;
}

export function getActiveEditor(): editor.ICodeEditor | null {
    return activeEditor;
}

export interface DispatchOptions {
    /**
     * Only dispatch an editor command when the editor actually has text focus.
     * Set by the keymap (which fires on window-level keystrokes) so a binding
     * to an editor command does not swallow typing in other inputs. The
     * context menu leaves this false — it focuses the editor before dispatch.
     */
    requireEditorFocus?: boolean;
}

/**
 * Dispatch a command id with optional args. Returns true if something was
 * invoked, false if it could not be dispatched (no registry command, and no
 * focused editor to receive an editor command) so the caller can let the
 * event fall through.
 */
export function dispatchCommand(
    id: string,
    args?: unknown,
    options?: DispatchOptions,
): boolean {
    if (getCommand(id)) {
        executeCommand(id, args);
        return true;
    }
    const ed = activeEditor;
    if (!ed) {
        return false;
    }
    if (options?.requireEditorFocus && !ed.hasTextFocus()) {
        return false;
    }
    // `trigger` routes editor actions (incl. registerAction2 commands via the
    // command service), core editor commands, and clipboard; the source
    // string is informational only.
    ed.trigger('operator.keybinding', id, args);
    return true;
}
