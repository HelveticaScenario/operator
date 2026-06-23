/**
 * Catalog of the context keys that `when` clauses can reference, with a type
 * and human description. Operator's equivalent of VS Code's `ContextKeyInfo`
 * registry (`RawContextKey.all()`), used to power `when`-clause autocomplete
 * in the keybindings editor.
 *
 * Keep this in sync with the keys actually published by
 * `contextKeyBootstrap.ts` — a key listed here but never set always evaluates
 * falsy, and a key set there but missing here simply won't autocomplete.
 */

export interface ContextKeyInfo {
    key: string;
    type: 'boolean' | 'string';
    description: string;
}

export const CONTEXT_KEY_INFO: readonly ContextKeyInfo[] = [
    {
        key: 'editorFocused',
        type: 'boolean',
        description: 'The editor widget has focus.',
    },
    {
        key: 'editorTextFocus',
        type: 'boolean',
        description: 'The editor text input has focus (VS Code alias).',
    },
    {
        key: 'textInputFocus',
        type: 'boolean',
        description: 'A text input has focus (VS Code alias).',
    },
    {
        key: 'editorFocus',
        type: 'boolean',
        description: 'The editor has focus (VS Code alias).',
    },
    {
        key: 'inputFocus',
        type: 'boolean',
        description: 'A text input has focus (VS Code alias).',
    },
    {
        key: 'editorReadonly',
        type: 'boolean',
        description: 'The editor is read-only (always false in Operator).',
    },
    {
        key: 'editorLangId',
        type: 'string',
        description: "The editor's language id (e.g. 'javascript').",
    },
    {
        key: 'editorHasDefinitionProvider',
        type: 'boolean',
        description: 'A go-to-definition provider is available.',
    },
    {
        key: 'editorHasReferenceProvider',
        type: 'boolean',
        description: 'A find-references provider is available.',
    },
    {
        key: 'editorHasDocumentSymbolProvider',
        type: 'boolean',
        description: 'A document-symbol (Go to Symbol) provider is available.',
    },
    {
        key: 'editorHasCompletionItemProvider',
        type: 'boolean',
        description: 'A completion (suggestion) provider is available.',
    },
    {
        key: 'editorHasSignatureHelpProvider',
        type: 'boolean',
        description:
            'A signature-help (parameter hints) provider is available.',
    },
    {
        key: 'editorHasCodeActionsProvider',
        type: 'boolean',
        description:
            'A code-actions (quick fix / refactor) provider is available.',
    },
    {
        key: 'editorHasRenameProvider',
        type: 'boolean',
        description: 'A rename provider is available.',
    },
    {
        key: 'editorHasDocumentSelectionFormattingProvider',
        type: 'boolean',
        description:
            'A range-formatting (Format Selection) provider is available.',
    },
    {
        key: 'foldingEnabled',
        type: 'boolean',
        description: "Code folding is enabled by the editor's folding option.",
    },
    {
        key: 'suggestWidgetVisible',
        type: 'boolean',
        description: 'The editor suggestion widget is visible.',
    },
    {
        key: 'findWidgetVisible',
        type: 'boolean',
        description: 'The editor find widget is visible.',
    },
    {
        key: 'inSettingsModal',
        type: 'boolean',
        description: 'The settings modal is open.',
    },
    {
        key: 'fileExplorerFocused',
        type: 'boolean',
        description: 'The file explorer tree has focus.',
    },
];
