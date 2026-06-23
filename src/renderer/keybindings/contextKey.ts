/**
 * Minimal context-key service.
 *
 * Modeled on vscode's `ContextKeyService` but stripped to a single
 * global, flat key-value store. No DOM-tree scoping, no per-editor
 * contexts. Surfaces that need "is this command available right now?"
 * (cmdk palette, editor context menu, tinykeys keymap) parse a when-
 * clause via `parseWhen` and evaluate it against this service.
 *
 * Bootstrap keys wired in by callers (typed for autocomplete):
 *   - editorFocused          (Monaco editor has focus)
 *   - suggestWidgetVisible   (Monaco suggest widget open)
 *   - findWidgetVisible      (Monaco find widget open)
 *   - inSettingsModal        (settings modal is open)
 *   - fileExplorerFocused    (file explorer tree has focus)
 */

import { parseWhen, type IContextReader, type WhenExpr } from './whenParser';

export type ContextKeyValue = boolean | number | string | null | undefined;

class ContextKeyService implements IContextReader {
    private readonly values = new Map<string, ContextKeyValue>();

    get(key: string): ContextKeyValue {
        return this.values.get(key);
    }

    set(key: string, value: ContextKeyValue): void {
        this.values.set(key, value);
    }

    /**
     * Batch update — sets every entry. Convenient when several keys flip in
     * response to one focus / modal / model transition.
     */
    setMany(entries: Record<string, ContextKeyValue>): void {
        for (const key of Object.keys(entries)) {
            this.values.set(key, entries[key]);
        }
    }

    /** Remove a key entirely. */
    unset(key: string): void {
        this.values.delete(key);
    }

    /** Test hook — drops every key. */
    reset(): void {
        this.values.clear();
    }
}

/**
 * Global singleton. Surfaces (Monaco focus bridge, modal mount/unmount,
 * arborist focus events) import `contextKeys` and call `set` directly.
 */
export const contextKeys = new ContextKeyService();

/**
 * Evaluate a when-clause source string against the global service.
 * Caches parsed expressions internally via `parseWhen`. Returns true
 * for empty / null / undefined inputs.
 */
export function evaluateWhen(source: string | undefined | null): boolean {
    try {
        return parseWhen(source).evaluate(contextKeys);
    } catch (err) {
        // A malformed when-clause (e.g. an unsupported operator in a
        // user's keybindings.json) must never break dispatch — treat it as
        // not-applicable so the binding falls through.
        console.warn('[contextKey] invalid when-clause:', source, err);
        return false;
    }
}

export type { WhenExpr };
