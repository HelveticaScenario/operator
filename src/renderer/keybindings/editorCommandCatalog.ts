/**
 * Curated Monaco editor commands for keybindings autocomplete that are NOT
 * returned by `editor.getSupportedActions()`:
 *
 *  - Navigation commands registered via `registerAction2` (Go to Definition,
 *    References, …) live in the command service, not the editor action map.
 *  - Core cursor / editing commands (`cursorDown`, …) are editor commands,
 *    also outside the action map.
 *
 * Everything registered via `registerEditorAction` (Rename, Format, comment,
 * move lines, fold, find, …) already comes from `getSupportedActions()` with
 * its own label, so it is intentionally absent here.
 */

export interface EditorCommandInfo {
    id: string;
    label: string;
}

export const EDITOR_COMMAND_CATALOG: readonly EditorCommandInfo[] = [
    // Navigation (registerAction2).
    { id: 'editor.action.revealDefinition', label: 'Go to Definition' },
    {
        id: 'editor.action.revealDefinitionAside',
        label: 'Open Definition to the Side',
    },
    { id: 'editor.action.peekDefinition', label: 'Peek Definition' },
    { id: 'editor.action.goToReferences', label: 'Go to References' },
    {
        id: 'editor.action.referenceSearch.trigger',
        label: 'Peek References',
    },
    {
        id: 'editor.action.goToTypeDefinition',
        label: 'Go to Type Definition',
    },
    {
        id: 'editor.action.goToImplementation',
        label: 'Go to Implementation',
    },
    { id: 'editor.action.quickOutline', label: 'Go to Symbol in Editor' },

    // Core cursor movement / selection commands (editor commands).
    { id: 'cursorUp', label: 'Cursor Up' },
    { id: 'cursorDown', label: 'Cursor Down' },
    { id: 'cursorLeft', label: 'Cursor Left' },
    { id: 'cursorRight', label: 'Cursor Right' },
    { id: 'cursorHome', label: 'Cursor Home' },
    { id: 'cursorEnd', label: 'Cursor End' },
    { id: 'cursorTop', label: 'Cursor Top' },
    { id: 'cursorBottom', label: 'Cursor Bottom' },
    { id: 'cursorWordLeft', label: 'Cursor Word Left' },
    { id: 'cursorWordRight', label: 'Cursor Word Right' },
    { id: 'cursorPageUp', label: 'Cursor Page Up' },
    { id: 'cursorPageDown', label: 'Cursor Page Down' },
    { id: 'cursorUpSelect', label: 'Cursor Up Select' },
    { id: 'cursorDownSelect', label: 'Cursor Down Select' },
    { id: 'cursorLeftSelect', label: 'Cursor Left Select' },
    { id: 'cursorRightSelect', label: 'Cursor Right Select' },
    { id: 'cursorHomeSelect', label: 'Cursor Home Select' },
    { id: 'cursorEndSelect', label: 'Cursor End Select' },
    { id: 'cursorWordLeftSelect', label: 'Cursor Word Left Select' },
    { id: 'cursorWordRightSelect', label: 'Cursor Word Right Select' },
    { id: 'cursorTopSelect', label: 'Cursor Top Select' },
    { id: 'cursorBottomSelect', label: 'Cursor Bottom Select' },
    { id: 'cursorUndo', label: 'Cursor Undo' },
    { id: 'editor.action.selectAll', label: 'Select All' },
    { id: 'undo', label: 'Undo' },
    { id: 'redo', label: 'Redo' },
];
