/**
 * Default keybindings shipped with Operator.
 *
 * Each entry is `(key, command, when?)`. `key` uses tinykeys' binding grammar
 * (https://github.com/jamiebuilds/tinykeys#keybinding-syntax): `$mod` resolves
 * to `Cmd` on macOS and `Ctrl` everywhere else, matching Electron's
 * `CmdOrCtrl` accelerator semantics.
 *
 * `command` is authored in VS Code's command-id vocabulary — the same ids a
 * user's imported `keybindings.json` would name — so the defaults and an
 * import are one vocabulary. At load, `mergeKeymap` runs every command through
 * `aliasCommand` (see `vscodeKeys`), rewriting the ids VS Code names
 * differently from their dispatch target (e.g. `workbench.action.gotoLine` →
 * `editor.action.gotoLine`, `workbench.action.files.save` → `operator.save`).
 * Operator-native actions with no VS Code analogue (the transport commands)
 * keep their `operator.*` id, which `aliasCommand` passes through untouched.
 *
 * `when` is stored verbatim and evaluated against the context-key service at
 * dispatch. An unset or empty clause is always true; a malformed clause is
 * treated as not matching, so the binding falls through.
 */
export type DefaultKeybinding = {
    key: string;
    /**
     * macOS-specific binding, used instead of `key` on darwin. Lets a default
     * faithfully encode Monaco's per-platform bindings (e.g. fold is
     * Ctrl+Shift+[ on Windows/Linux but Cmd+Alt+[ on macOS). An empty `key`
     * with a `mac` means the binding exists only on macOS.
     */
    mac?: string;
    command: string;
    when?: string;
};

export const DEFAULT_KEYMAP: readonly DefaultKeybinding[] = [
    // Transport commands are Operator-native (no VS Code analogue) and use
    // physical Control on every platform (Ctrl+Enter, not Cmd+Enter on macOS) —
    // the long-standing Operator convention, and it leaves Cmd+Enter free for
    // Monaco's "insert line below".
    { key: 'Control+Enter', command: 'operator.updatePatch' },
    { key: 'Control+Shift+Enter', command: 'operator.updatePatchNextBeat' },
    { key: 'Control+.', command: 'operator.stop' },
    // File / app commands use $mod (Cmd on macOS) and are named with their VS
    // Code command id; `aliasCommand` rewrites each to the `operator.*` it
    // dispatches to. Operator's "Open Folder" is a workspace/folder picker.
    { key: '$mod+n', command: 'workbench.action.files.newUntitledFile' },
    { key: '$mod+o', command: 'workbench.action.files.openFolder' },
    { key: '$mod+s', command: 'workbench.action.files.save' },
    { key: '$mod+w', command: 'workbench.action.closeActiveEditor' },
    { key: '$mod+,', command: 'workbench.action.openSettings' },
    { key: 'F1', command: 'workbench.action.showCommands' },
    { key: '$mod+Shift+p', command: 'workbench.action.showCommands' },

    // VS Code editor command ids that mirror VS Code's own default keybindings
    // so the editor context menu can display the shortcut. Monaco handles the
    // key natively while the editor is focused (it stops event propagation, so
    // this binding is shadowed there, not double-dispatched); the entry exists
    // for display and as a fallback dispatch path. Most ids are shared verbatim
    // with Monaco; `gotoSymbol` is aliased to Monaco's standalone
    // `editor.action.quickOutline`.
    { key: 'F12', command: 'editor.action.revealDefinition' },
    { key: '$mod+Shift+o', command: 'workbench.action.gotoSymbol' },
    { key: 'Shift+F12', command: 'editor.action.goToReferences' },
    { key: 'Alt+F12', command: 'editor.action.peekDefinition' },
    { key: 'F2', command: 'editor.action.rename' },
    { key: '$mod+F2', command: 'editor.action.changeAll' },
    { key: 'Shift+Alt+f', command: 'editor.action.formatDocument' },
    // Go to Line/Column — physical Ctrl+G on every platform (VS Code's and
    // Monaco's macOS default). VS Code names it `workbench.action.gotoLine`;
    // `aliasCommand` rewrites it to Monaco's `editor.action.gotoLine`.
    { key: 'Control+g', command: 'workbench.action.gotoLine' },

    // Remaining Monaco editor-action default bindings, extracted from the
    // monaco-editor source (kbOpts). They give every palette-visible editor
    // action a displayed, rebindable shortcut. `when` clauses are preserved:
    // an action only dispatches through this keymap when its context is active
    // (and tracked); otherwise it falls through to Monaco's own handling.
    // Context-multiplexed bare keys (suggest/find widgets, core cursor
    // commands) and clipboard are intentionally excluded.
    { key: '$mod+u', command: 'cursorUndo', when: 'textInputFocus' },
    {
        key: '$mod+k $mod+c',
        command: 'editor.action.addCommentLine',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: '$mod+d',
        command: 'editor.action.addSelectionToNextFindMatch',
        when: 'editorFocus',
    },
    {
        key: 'alt+shift+.',
        mac: '$mod+alt+.',
        command: 'editor.action.autoFix',
        when: '!editorReadonly && textInputFocus',
    },
    {
        key: 'shift+alt+a',
        command: 'editor.action.blockComment',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: '$mod+/',
        command: 'editor.action.commentLine',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: 'alt+shift+down',
        command: 'editor.action.copyLinesDownAction',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: 'alt+shift+up',
        command: 'editor.action.copyLinesUpAction',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: '$mod+shift+k',
        command: 'editor.action.deleteLines',
        when: '!editorReadonly && textInputFocus',
    },
    {
        key: '$mod+k $mod+f',
        command: 'editor.action.formatSelection',
        when: '!editorReadonly && editorHasDocumentSelectionFormattingProvider && editorTextFocus',
    },
    {
        key: '$mod+shift+.',
        command: 'editor.action.inPlaceReplace.down',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: '$mod+shift+,',
        command: 'editor.action.inPlaceReplace.up',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: '$mod+]',
        command: 'editor.action.indentLines',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: '$mod+alt+up',
        command: 'editor.action.insertCursorAbove',
        when: 'editorTextFocus',
    },
    {
        key: 'shift+alt+i',
        command: 'editor.action.insertCursorAtEndOfEachLineSelected',
        when: 'editorTextFocus',
    },
    {
        key: '$mod+alt+down',
        command: 'editor.action.insertCursorBelow',
        when: 'editorTextFocus',
    },
    // mac-only: on Windows/Linux $mod resolves to Control, which would
    // collapse onto the transport bindings (Ctrl+Enter = Update Patch,
    // Ctrl+Shift+Enter = Next Beat) and shadow them while editing. On macOS
    // these are Cmd+Enter / Cmd+Shift+Enter, distinct from the Ctrl transport
    // keys. (On non-mac, Monaco's own Ctrl+Enter is pre-empted by the
    // capture-phase Update Patch binding anyway.)
    {
        key: '',
        mac: '$mod+enter',
        command: 'editor.action.insertLineAfter',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: '',
        mac: '$mod+shift+enter',
        command: 'editor.action.insertLineBefore',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: '$mod+shift+\\',
        command: 'editor.action.jumpToBracket',
        when: 'editorTextFocus',
    },
    {
        key: '$mod+shift+f2',
        command: 'editor.action.linkedEditing',
        when: '!editorReadonly && editorHasRenameProvider && editorTextFocus',
    },
    {
        key: 'alt+f8',
        command: 'editor.action.marker.next',
        when: 'editorFocus',
    },
    {
        key: 'f8',
        command: 'editor.action.marker.nextInFiles',
        when: 'editorFocus',
    },
    {
        key: 'shift+alt+f8',
        command: 'editor.action.marker.prev',
        when: 'editorFocus',
    },
    {
        key: 'shift+f8',
        command: 'editor.action.marker.prevInFiles',
        when: 'editorFocus',
    },
    {
        key: 'alt+down',
        command: 'editor.action.moveLinesDownAction',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: 'alt+up',
        command: 'editor.action.moveLinesUpAction',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: '$mod+k $mod+d',
        command: 'editor.action.moveSelectionToNextFindMatch',
        when: 'editorFocus',
    },
    {
        key: '$mod+f3',
        command: 'editor.action.nextSelectionMatchFindAction',
        when: 'editorFocus',
    },
    {
        key: 'shift+alt+o',
        command: 'editor.action.organizeImports',
        when: '!editorReadonly && textInputFocus',
    },
    {
        key: '$mod+[',
        command: 'editor.action.outdentLines',
        when: '!editorReadonly && editorTextFocus',
    },
    {
        key: '$mod+shift+f3',
        command: 'editor.action.previousSelectionMatchFindAction',
        when: 'editorFocus',
    },
    // mac-only: on non-mac $mod+. resolves to Ctrl+. which is Operator's Stop
    // transport binding; keep Stop authoritative there.
    {
        key: '',
        mac: '$mod+.',
        command: 'editor.action.quickFix',
        when: '!editorReadonly && editorHasCodeActionsProvider && textInputFocus',
    },
    {
        key: '$mod+shift+r',
        mac: 'ctrl+shift+r',
        command: 'editor.action.refactor',
        when: '!editorReadonly && editorHasCodeActionsProvider && textInputFocus',
    },
    {
        key: '$mod+alt+backspace',
        command: 'editor.action.removeBrackets',
        when: 'editorTextFocus',
    },
    {
        key: '$mod+k $mod+u',
        command: 'editor.action.removeCommentLine',
        when: '!editorReadonly && editorTextFocus',
    },
    // selectionAnchorSet / hasWordHighlights (below) are transient states
    // Monaco tracks on its own internal context-key service, which Operator
    // can't observe. We gate only on editorTextFocus and let editor.trigger
    // run: the action's Monaco precondition makes it a safe no-op when no
    // anchor is set / no highlights are shown.
    {
        key: '$mod+k $mod+k',
        command: 'editor.action.selectFromAnchorToCursor',
        when: 'editorTextFocus',
    },
    {
        key: '$mod+shift+l',
        command: 'editor.action.selectHighlights',
        when: 'editorFocus',
    },
    {
        key: '$mod+k $mod+b',
        command: 'editor.action.setSelectionAnchor',
        when: 'editorTextFocus',
    },
    {
        key: '$mod+k $mod+i',
        command: 'editor.action.showHover',
        when: 'editorTextFocus',
    },
    {
        key: 'shift+alt+right',
        mac: '$mod+ctrl+shift+right',
        command: 'editor.action.smartSelect.expand',
        when: 'editorTextFocus',
    },
    {
        key: 'shift+alt+left',
        mac: '$mod+ctrl+shift+left',
        command: 'editor.action.smartSelect.shrink',
        when: 'editorTextFocus',
    },
    {
        key: '',
        mac: 'ctrl+t',
        command: 'editor.action.transposeLetters',
        when: '!editorReadonly && textInputFocus',
    },
    {
        key: '$mod+shift+space',
        command: 'editor.action.triggerParameterHints',
        when: 'editorHasSignatureHelpProvider && editorTextFocus',
    },
    {
        key: '$mod+space',
        mac: 'ctrl+space',
        command: 'editor.action.triggerSuggest',
        when: '!editorReadonly && editorHasCompletionItemProvider && !suggestWidgetVisible && textInputFocus',
    },
    {
        key: '$mod+k $mod+x',
        command: 'editor.action.trimTrailingWhitespace',
        when: '!editorReadonly && editorTextFocus',
    },
    // Gated on editorTextFocus only; Monaco self-gates on its internal
    // hasWordHighlights (see selectFromAnchorToCursor above).
    {
        key: 'f7',
        command: 'editor.action.wordHighlight.next',
        when: 'editorTextFocus',
    },
    {
        key: 'shift+f7',
        command: 'editor.action.wordHighlight.prev',
        when: 'editorTextFocus',
    },
    {
        key: '$mod+k $mod+,',
        command: 'editor.createFoldingRangeFromSelection',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+shift+[',
        mac: '$mod+alt+[',
        command: 'editor.fold',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+0',
        command: 'editor.foldAll',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+/',
        command: 'editor.foldAllBlockComments',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+-',
        command: 'editor.foldAllExcept',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+8',
        command: 'editor.foldAllMarkerRegions',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+1',
        command: 'editor.foldLevel1',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+2',
        command: 'editor.foldLevel2',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+3',
        command: 'editor.foldLevel3',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+4',
        command: 'editor.foldLevel4',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+5',
        command: 'editor.foldLevel5',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+6',
        command: 'editor.foldLevel6',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+7',
        command: 'editor.foldLevel7',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+[',
        command: 'editor.foldRecursively',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+.',
        command: 'editor.removeManualFoldingRanges',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+l',
        command: 'editor.toggleFold',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+shift+l',
        command: 'editor.toggleFoldRecursively',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+shift+]',
        mac: '$mod+alt+]',
        command: 'editor.unfold',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+j',
        command: 'editor.unfoldAll',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+=',
        command: 'editor.unfoldAllExcept',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+9',
        command: 'editor.unfoldAllMarkerRegions',
        when: 'foldingEnabled && editorTextFocus',
    },
    {
        key: '$mod+k $mod+]',
        command: 'editor.unfoldRecursively',
        when: 'foldingEnabled && editorTextFocus',
    },
];
