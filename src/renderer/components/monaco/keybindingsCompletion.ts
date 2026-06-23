/**
 * Autocomplete for the user keybindings.json editor buffer, modeled on VS
 * Code: `command` values complete to known command ids (VS Code sources these
 * from its command registry), and `when` values complete to context keys
 * (VS Code sources these from `RawContextKey.all()`).
 *
 * VS Code drives `command` from a JSON schema and `when` from a dedicated
 * provider; Operator unifies both into one Monaco completion provider scoped
 * to the keybindings buffer, which keeps the two sources of suggestions in
 * one place and avoids fighting the existing config.json schema registration.
 */
import type { Monaco } from '../../hooks/useCustomMonaco';
import type { editor, languages, Position } from 'monaco-editor';

import { listCommands } from '../../keybindings/commands';
import { getActiveEditor } from '../../keybindings/dispatch';
import { EDITOR_COMMAND_CATALOG } from '../../keybindings/editorCommandCatalog';
import { CONTEXT_KEY_INFO } from '../../keybindings/contextKeyInfo';

export type KeybindingCompletion =
    | { kind: 'command'; word: string }
    | { kind: 'when'; word: string }
    | null;

/**
 * Classify the cursor position from the text before it on the current line:
 * inside a `"command"` value, inside a `"when"` value, or neither. `word` is
 * the partial token to filter/replace (for `command`, the id without any
 * leading `-` removal marker; for `when`, the trailing context-key fragment).
 */
export function detectKeybindingCompletion(
    textBeforeCursor: string,
): KeybindingCompletion {
    const command = textBeforeCursor.match(/"command"\s*:\s*"-?([\w.]*)$/);
    if (command) {
        return { kind: 'command', word: command[1] };
    }
    const when = textBeforeCursor.match(/"when"\s*:\s*"([^"]*)$/);
    if (when) {
        const fragment = when[1].match(/[\w.]*$/);
        return { kind: 'when', word: fragment ? fragment[0] : '' };
    }
    return null;
}

/** Every command id offerable in a `command` value, with a display label. */
function knownCommands(): Array<{ id: string; label: string }> {
    const byId = new Map<string, string>();
    for (const { id, metadata } of listCommands()) {
        byId.set(id, metadata?.label ?? id);
    }
    const ed = getActiveEditor();
    if (ed) {
        for (const action of ed.getSupportedActions()) {
            if (action.id && !byId.has(action.id)) {
                byId.set(action.id, action.label?.trim() || action.id);
            }
        }
    }
    for (const command of EDITOR_COMMAND_CATALOG) {
        if (!byId.has(command.id)) {
            byId.set(command.id, command.label);
        }
    }
    return [...byId].map(([id, label]) => ({ id, label }));
}

function isKeybindingsModel(model: editor.ITextModel): boolean {
    // Match by basename so the provider only acts on the keybindings buffer.
    return model.uri.path.endsWith('/keybindings.json');
}

/**
 * Register the keybindings completion provider. Returns a disposable.
 */
export function registerKeybindingsCompletionProvider(monaco: Monaco): {
    dispose: () => void;
} {
    const provider: languages.CompletionItemProvider = {
        // `"` opens a value; `.` continues a command id; ` ` and `!` continue a
        // when expression after operators.
        triggerCharacters: ['"', '.', ' ', '!', '-'],
        provideCompletionItems(
            model: editor.ITextModel,
            position: Position,
        ): languages.CompletionList | undefined {
            if (!isKeybindingsModel(model)) {
                return undefined;
            }
            const lineContent = model.getLineContent(position.lineNumber);
            const before = lineContent.slice(0, position.column - 1);
            const ctx = detectKeybindingCompletion(before);
            if (!ctx) {
                return undefined;
            }
            // Insert range covers the pre-cursor partial; replace range also
            // overwrites the token after the cursor, so accepting mid-token
            // replaces the whole identifier instead of duplicating its tail.
            const startColumn = position.column - ctx.word.length;
            const trailing = lineContent
                .slice(position.column - 1)
                .match(/^[\w.]*/);
            const range = {
                insert: {
                    startLineNumber: position.lineNumber,
                    endLineNumber: position.lineNumber,
                    startColumn,
                    endColumn: position.column,
                },
                replace: {
                    startLineNumber: position.lineNumber,
                    endLineNumber: position.lineNumber,
                    startColumn,
                    endColumn: position.column + (trailing ? trailing[0].length : 0),
                },
            };

            if (ctx.kind === 'command') {
                const suggestions = knownCommands().map((command) => ({
                    label: command.id,
                    kind: monaco.languages.CompletionItemKind.Value,
                    detail: command.label,
                    insertText: command.id,
                    range,
                }));
                return { suggestions };
            }

            const suggestions = CONTEXT_KEY_INFO.map((info) => ({
                label: info.key,
                kind: monaco.languages.CompletionItemKind.Constant,
                detail: info.type,
                documentation: info.description,
                insertText: info.key,
                range,
            }));
            return { suggestions };
        },
    };

    return monaco.languages.registerCompletionItemProvider('json', provider);
}
