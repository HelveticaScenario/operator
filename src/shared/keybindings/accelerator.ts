/**
 * Convert a tinykeys binding string into an Electron accelerator string, used
 * to display a command's shortcut in both the editor context menu and the
 * application (top bar) menu. Single source of truth for shortcut formatting.
 *
 * Electron accelerators describe a single chord only, and Electron renders
 * them natively (e.g. `Cmd+Shift+P` -> `⇧⌘P` on macOS). Tinykeys chord
 * sequences (`Meta+k Meta+i`) have no Electron equivalent, so they return
 * null (the shortcut is simply not shown — the binding still works).
 */

// tinykeys modifier token -> Electron modifier, in a stable display order.
const MOD_TO_ELECTRON: Array<[string, string]> = [
    ['Control', 'Ctrl'],
    ['Alt', 'Alt'],
    ['Shift', 'Shift'],
    ['Meta', 'Cmd'],
];

// Reverse of the `event.code` names emitted for composed keys (see vscodeKeys).
const CODE_TO_CHAR: Record<string, string> = {
    Minus: '-',
    Equal: '=',
    BracketLeft: '[',
    BracketRight: ']',
    Backslash: '\\',
    Semicolon: ';',
    Quote: "'",
    Comma: ',',
    Period: '.',
    Slash: '/',
    Backquote: '`',
};

// tinykeys key-name -> Electron key code.
const KEY_TO_ELECTRON: Record<string, string> = {
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    Escape: 'Esc',
};

function electronKey(token: string): string | null {
    // Code-regex form, e.g. `(KeyI)` / `(Digit2)` / `(BracketLeft)`.
    const codeMatch = token.match(/^\((.+)\)$/);
    if (codeMatch) {
        const code = codeMatch[1];
        const letter = code.match(/^Key([A-Z])$/);
        if (letter) {
            return letter[1];
        }
        const digit = code.match(/^Digit([0-9])$/);
        if (digit) {
            return digit[1];
        }
        return CODE_TO_CHAR[code] ?? null;
    }
    if (KEY_TO_ELECTRON[token]) {
        return KEY_TO_ELECTRON[token];
    }
    if (/^F([1-9]|1[0-9]|2[0-4])$/.test(token)) {
        return token;
    }
    if (token.length === 1) {
        return /[a-z]/i.test(token) ? token.toUpperCase() : token;
    }
    // Named keys Electron accepts verbatim (Enter, Tab, Space, Home, …).
    return token;
}

/**
 * Format a single tinykeys binding as an Electron accelerator, or null if it
 * is a chord sequence or its key cannot be represented.
 */
export function toElectronAccelerator(binding: string): string | null {
    const trimmed = binding.trim();
    if (trimmed.length === 0 || /\s/.test(trimmed)) {
        // Empty or a multi-press chord — not representable.
        return null;
    }
    const tokens = trimmed.split('+').filter((t) => t.length > 0);
    if (tokens.length === 0) {
        return null;
    }
    const keyToken = tokens[tokens.length - 1];
    const mods = new Set(tokens.slice(0, -1));
    const key = electronKey(keyToken);
    if (key === null) {
        return null;
    }
    const parts: string[] = [];
    for (const [token, electron] of MOD_TO_ELECTRON) {
        if (mods.has(token)) {
            parts.push(electron);
        }
    }
    parts.push(key);
    return parts.join('+');
}
