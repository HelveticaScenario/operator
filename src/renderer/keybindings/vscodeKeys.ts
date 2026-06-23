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
 * Normalize one raw override into the internal entry form. Returns null when
 * the key cannot be translated (the entry is dropped rather than installed
 * wrong). Removal is signalled by `command: null` (Operator) or a `-` prefix
 * (VS Code).
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
        return { type: 'remove', key, command: raw.slice(1) };
    }
    return {
        type: 'bind',
        key,
        command: raw,
        ...(override.when ? { when: override.when } : {}),
        ...(override.args !== undefined ? { args: override.args } : {}),
    };
}
