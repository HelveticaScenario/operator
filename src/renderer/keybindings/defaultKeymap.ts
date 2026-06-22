/**
 * Default keybindings shipped with Operator.
 *
 * Each entry is `(key, command, when?)`. `key` uses tinykeys' binding grammar
 * (https://github.com/jamiebuilds/tinykeys#keybinding-syntax): `$mod` resolves
 * to `Cmd` on macOS and `Ctrl` everywhere else, matching Electron's
 * `CmdOrCtrl` accelerator semantics.
 *
 * `when` is stored verbatim. Until Phase 2.2 (context-key service) lands,
 * the keymap loader treats unset / unparseable when-clauses as always true.
 *
 * Phase 2.3 from `~/.claude/plans/operator-is-at-its-goofy-mist.md`.
 */
import type { KeybindingOverride } from '../../shared/ipcTypes';

export type DefaultKeybinding = {
    key: string;
    command: string;
    when?: string;
};

export const DEFAULT_KEYMAP: readonly DefaultKeybinding[] = [
    { key: '$mod+Enter', command: 'operator.updatePatch' },
    { key: '$mod+Shift+Enter', command: 'operator.updatePatchNextBeat' },
    { key: '$mod+.', command: 'operator.stop' },
    { key: '$mod+n', command: 'operator.newFile' },
    { key: '$mod+o', command: 'operator.openWorkspace' },
    { key: '$mod+s', command: 'operator.save' },
    { key: '$mod+w', command: 'operator.closeBuffer' },
    { key: '$mod+,', command: 'operator.openSettings' },
    { key: 'F1', command: 'operator.showCommandPalette' },
    { key: '$mod+Shift+p', command: 'operator.showCommandPalette' },

    // Monaco's built-in editor commands. These mirror Monaco's own default
    // keybindings so the editor context menu can display the shortcut. Monaco
    // handles the key natively while the editor is focused (it stops event
    // propagation, so this binding is shadowed there, not double-dispatched);
    // the entry exists for display and as a fallback dispatch path.
    { key: 'F12', command: 'editor.action.revealDefinition' },
    { key: '$mod+Shift+o', command: 'editor.action.quickOutline' },
    { key: 'Shift+F12', command: 'editor.action.goToReferences' },
    { key: 'Alt+F12', command: 'editor.action.peekDefinition' },
    { key: 'F2', command: 'editor.action.rename' },
    { key: '$mod+F2', command: 'editor.action.changeAll' },
    { key: 'Shift+Alt+f', command: 'editor.action.formatDocument' },
];

/**
 * Convenience for tests / settings UI: produce the default map as
 * `KeybindingOverride` records so it can flow through the same merge path
 * as user overrides.
 */
export function defaultKeymapAsOverrides(): KeybindingOverride[] {
    return DEFAULT_KEYMAP.map((entry) => ({
        key: entry.key,
        command: entry.command,
        ...(entry.when ? { when: entry.when } : {}),
    }));
}
