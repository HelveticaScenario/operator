/**
 * Translate VS Code `keybindings.json` key syntax into tinykeys binding
 * strings, and normalize a user keymap (VS Code or Operator style) into the
 * internal form the loader installs.
 *
 * VS Code and tinykeys differ in three ways we bridge here:
 *
 *  - Modifiers: VS Code writes `cmd` / `ctrl` / `alt` / `shift` (and only on
 *    the platform they exist); tinykeys matches `Meta` / `Control` / `Alt` /
 *    `Shift` via `KeyboardEvent.getModifierState`. `$mod` (Operator defaults)
 *    resolves to the platform's primary modifier, same as tinykeys.
 *  - Keys: most names match case-insensitively against `event.key`
 *    (`enter`, `f1`, `home`), but the arrow keys must be spelled `ArrowUp`
 *    etc., so they are translated explicitly.
 *  - Chords: both use space-separated presses (`cmd+k cmd+i`), so only each
 *    press needs translating.
 *
 * Resolving `$mod` and the VS Code modifiers to the same canonical token set
 * (with modifiers in a fixed order) lets the loader dedupe a default and a
 * user override that denote the same physical chord — otherwise the same
 * keypress would fire two bindings.
 */
import type { KeybindingOverride } from '../../shared/ipcTypes';

export type Platform = 'darwin' | 'other';

/** Modifier output order, so equal chords produce equal strings. */
const MOD_ORDER = ['Control', 'Alt', 'Shift', 'Meta'] as const;

const MOD_ALIASES: Record<string, (typeof MOD_ORDER)[number]> = {
    cmd: 'Meta',
    command: 'Meta',
    meta: 'Meta',
    win: 'Meta',
    windows: 'Meta',
    super: 'Meta',
    ctrl: 'Control',
    control: 'Control',
    alt: 'Alt',
    option: 'Alt',
    opt: 'Alt',
    shift: 'Shift',
};

const KEY_ALIASES: Record<string, string> = {
    up: 'ArrowUp',
    down: 'ArrowDown',
    left: 'ArrowLeft',
    right: 'ArrowRight',
    esc: 'Escape',
    escape: 'Escape',
    enter: 'Enter',
    return: 'Enter',
    tab: 'Tab',
    space: 'Space',
    spacebar: 'Space',
    backspace: 'Backspace',
    delete: 'Delete',
    del: 'Delete',
    insert: 'Insert',
    ins: 'Insert',
    home: 'Home',
    end: 'End',
    pageup: 'PageUp',
    pagedown: 'PageDown',
};

function platformMod(platform: Platform): (typeof MOD_ORDER)[number] {
    return platform === 'darwin' ? 'Meta' : 'Control';
}

// US-layout `event.code` for printable punctuation, used when a modifier
// would change the produced character.
const CHAR_TO_CODE: Record<string, string> = {
    '-': 'Minus',
    '=': 'Equal',
    '[': 'BracketLeft',
    ']': 'BracketRight',
    '\\': 'Backslash',
    ';': 'Semicolon',
    "'": 'Quote',
    ',': 'Comma',
    '.': 'Period',
    '/': 'Slash',
    '`': 'Backquote',
};

function codeForChar(raw: string): string | null {
    if (/^[a-z]$/i.test(raw)) {
        return `Key${raw.toUpperCase()}`;
    }
    if (/^[0-9]$/.test(raw)) {
        return `Digit${raw}`;
    }
    return CHAR_TO_CODE[raw] ?? null;
}

function normalizeKeyToken(
    raw: string,
    mods: { hasAlt: boolean; hasShift: boolean },
): string {
    const lower = raw.toLowerCase();
    const alias = KEY_ALIASES[lower];
    if (alias) {
        return alias;
    }
    if (/^f([1-9]|1[0-9]|2[0-4])$/.test(lower)) {
        return `F${lower.slice(1)}`;
    }
    if (raw.length === 1) {
        const isLetter = /[a-z]/i.test(raw);
        // Alt composes any printable key, and Shift composes digits/symbols,
        // so the produced `event.key` no longer equals the authored char.
        // Emit tinykeys' `(code)` regex form, which matches `event.code`
        // (layout-independent, like VS Code's keycodes). Shift+letter still
        // matches case-insensitively, so it needs no code form.
        const needsCode = mods.hasAlt || (mods.hasShift && !isLetter);
        if (needsCode) {
            const code = codeForChar(raw);
            if (code) {
                return `(${code})`;
            }
        }
        // Letters lower-cased for a canonical form (tinykeys matches case-
        // insensitively); digits and punctuation pass through unchanged.
        return isLetter ? lower : raw;
    }
    // Already a multi-char key name (e.g. `ArrowUp`) — leave as authored.
    return raw;
}

/**
 * Translate one press (`cmd+shift+k`) into a canonical tinykeys press
 * (`Shift+Meta+k`). Returns null if the press has no resolvable key (e.g. a
 * lone modifier or an unknown extra token before the key).
 */
function translatePress(press: string, platform: Platform): string | null {
    const parts = press.split('+').filter((p) => p.length > 0);
    if (parts.length === 0) {
        return null;
    }
    const mods = new Set<(typeof MOD_ORDER)[number]>();
    let keyToken: string | null = null;

    for (let i = 0; i < parts.length; i++) {
        const token = parts[i];
        const lower = token.toLowerCase();
        const isLast = i === parts.length - 1;
        if (lower === '$mod' || lower === 'mod' || lower === 'cmdorctrl') {
            mods.add(platformMod(platform));
            continue;
        }
        const modAlias = MOD_ALIASES[lower];
        if (modAlias) {
            mods.add(modAlias);
            continue;
        }
        if (isLast) {
            keyToken = token;
        } else {
            // A non-modifier token that is not the final key — unparseable.
            return null;
        }
    }

    if (keyToken === null) {
        return null;
    }
    // Normalize the key last, once the modifier set is known (Alt/Shift change
    // how the key should be matched).
    const key = normalizeKeyToken(keyToken, {
        hasAlt: mods.has('Alt'),
        hasShift: mods.has('Shift'),
    });
    const orderedMods = MOD_ORDER.filter((m) => mods.has(m));
    return [...orderedMods, key].join('+');
}

/**
 * Translate a full VS Code / Operator key string (possibly a chord) into a
 * tinykeys binding string. Returns null if any press is unparseable.
 */
export function toTinykeys(key: string, platform: Platform): string | null {
    const presses = key.trim().split(/\s+/).filter(Boolean);
    if (presses.length === 0) {
        return null;
    }
    const translated: string[] = [];
    for (const press of presses) {
        const t = translatePress(press, platform);
        if (t === null) {
            return null;
        }
        translated.push(t);
    }
    return translated.join(' ');
}

/** A binding to install, with its key already in canonical tinykeys form. */
export interface NormalizedBinding {
    key: string;
    command: string;
    when?: string;
    args?: unknown;
}

/** A removal rule. `command: null` removes every binding for `key`. */
export interface NormalizedRemoval {
    key: string;
    command: string | null;
}

export type NormalizedEntry =
    | ({ type: 'bind' } & NormalizedBinding)
    | ({ type: 'remove' } & NormalizedRemoval);

/**
 * VS Code command id → Operator dispatch id.
 *
 * Operator authors its default keymap and accepts user keymaps in VS Code's
 * command vocabulary, so a VS Code `keybindings.json` drops in. Dispatch,
 * however, resolves only an `operator.*` registry command or a Monaco editor
 * action / core command (`dispatch.ts`). This table rewrites the handful of
 * actions VS Code names differently from their Operator/Monaco dispatch id;
 * every other id — the shared `editor.action.*` set, core commands, and
 * Operator-native `operator.*` ids — has no entry and passes through unchanged.
 *
 * Two kinds of divergence are bridged:
 *   - Workbench quick-inputs that Monaco's standalone editor re-registers under
 *     an `editor.action.*` id (Go to Line, Go to Symbol).
 *   - App-level actions Operator implements as `operator.*` registry commands
 *     (save, new file, close, settings, command palette, keyboard shortcuts,
 *     open folder).
 */
export const COMMAND_ALIASES: Readonly<Record<string, string>> = {
    // Monaco standalone re-registers these workbench quick-inputs as editor
    // actions, so the VS Code id never resolves without the rewrite.
    'workbench.action.gotoLine': 'editor.action.gotoLine',
    'workbench.action.gotoSymbol': 'editor.action.quickOutline',
    // App-level commands Operator owns in its registry.
    'workbench.action.files.save': 'operator.save',
    'workbench.action.files.newUntitledFile': 'operator.newFile',
    'workbench.action.closeActiveEditor': 'operator.closeBuffer',
    'workbench.action.showCommands': 'operator.showCommandPalette',
    'workbench.action.openSettings': 'operator.openSettings',
    // File (JSON) variant first — it matches Operator's "Open Keyboard
    // Shortcuts (JSON)" behaviour and is the id autocomplete offers; the UI
    // variant (the common Cmd+K Cmd+S default) still aliases on import.
    'workbench.action.openGlobalKeybindingsFile': 'operator.openKeybindings',
    'workbench.action.openGlobalKeybindings': 'operator.openKeybindings',
    // Operator's "open" is a folder/workspace picker (main.ts FS_SELECT_WORKSPACE),
    // matching VS Code's Open Folder; the mac "Open…" id maps here too.
    'workbench.action.files.openFolder': 'operator.openWorkspace',
    'workbench.action.files.openFileFolder': 'operator.openWorkspace',
};

/**
 * Resolve an authored command id (VS Code or Operator vocabulary) to the id
 * dispatch understands. Identity for any id without an alias — including ids
 * that are already in Operator/Monaco dispatch form.
 */
export function aliasCommand(command: string): string {
    return COMMAND_ALIASES[command] ?? command;
}

/**
 * Reverse of `COMMAND_ALIASES`: dispatch id → the VS Code id to author it as.
 * When several VS Code ids alias to one dispatch id, the first listed in
 * `COMMAND_ALIASES` wins, so order that table most-preferred-first.
 */
function invertAliases(): Readonly<Record<string, string>> {
    const out: Record<string, string> = {};
    for (const [vscodeId, dispatchId] of Object.entries(COMMAND_ALIASES)) {
        if (!(dispatchId in out)) {
            out[dispatchId] = vscodeId;
        }
    }
    return out;
}

const DISPATCH_TO_VSCODE = invertAliases();

/**
 * The id a keybindings file should author a command as: the VS Code id when
 * the command has one, else the id unchanged. Inverse of `aliasCommand` — what
 * the keybindings autocomplete offers, so a user writes (and pastes) VS Code's
 * vocabulary instead of Operator's internal dispatch id.
 */
export function authoringId(command: string): string {
    return DISPATCH_TO_VSCODE[command] ?? command;
}

/**
 * Normalize one raw override into the internal entry form. Returns null when
 * the key cannot be translated (the entry is dropped rather than installed
 * wrong). The command id is aliased to its dispatch form (see `aliasCommand`)
 * so VS Code ids resolve. Removal is signalled by `command: null` (Operator)
 * or a `-` prefix (VS Code); the removed id is aliased too, so a `-`-prefixed
 * VS Code id cancels a default authored in the same VS Code vocabulary.
 */
export function normalizeOverride(
    override: KeybindingOverride,
    platform: Platform,
): NormalizedEntry | null {
    const key = toTinykeys(override.key, platform);
    if (key === null) {
        return null;
    }
    const raw = override.command;
    if (raw === null) {
        return { type: 'remove', key, command: null };
    }
    if (raw.startsWith('-')) {
        return { type: 'remove', key, command: aliasCommand(raw.slice(1)) };
    }
    return {
        type: 'bind',
        key,
        command: aliasCommand(raw),
        ...(override.when ? { when: override.when } : {}),
        ...(override.args !== undefined ? { args: override.args } : {}),
    };
}
