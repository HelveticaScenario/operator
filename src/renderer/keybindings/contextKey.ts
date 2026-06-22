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
 *
 * See `~/.claude/plans/operator-is-at-its-goofy-mist.md` Phase 2.2.
 */

import { parseWhen, type IContextReader, type WhenExpr } from './whenParser';

export type ContextKeyValue = boolean | number | string | null | undefined;

/** Listener invoked once per `set` call. `changedKeys` is non-empty. */
export type ContextChangeListener = (changedKeys: ReadonlySet<string>) => void;

export interface Disposable {
    dispose(): void;
}

class ContextKeyService implements IContextReader {
    private readonly values = new Map<string, ContextKeyValue>();
    private readonly listeners = new Set<ContextChangeListener>();

    get(key: string): ContextKeyValue {
        return this.values.get(key);
    }

    set(key: string, value: ContextKeyValue): void {
        const prev = this.values.get(key);
        if (Object.is(prev, value)) return;
        this.values.set(key, value);
        this.emit(new Set([key]));
    }

    /**
     * Batch update — emits a single change event listing every key that
     * actually changed. Useful when several keys flip in response to one
     * focus / modal transition.
     */
    setMany(entries: Record<string, ContextKeyValue>): void {
        const changed = new Set<string>();
        for (const key of Object.keys(entries)) {
            const value = entries[key];
            const prev = this.values.get(key);
            if (!Object.is(prev, value)) {
                this.values.set(key, value);
                changed.add(key);
            }
        }
        if (changed.size > 0) this.emit(changed);
    }

    /** Remove a key entirely. Treated as a change to `undefined`. */
    unset(key: string): void {
        if (!this.values.has(key)) return;
        this.values.delete(key);
        this.emit(new Set([key]));
    }

    onDidChange(listener: ContextChangeListener): Disposable {
        this.listeners.add(listener);
        return {
            dispose: () => {
                this.listeners.delete(listener);
            },
        };
    }

    /** Test hook — drops every key and every listener. */
    reset(): void {
        this.values.clear();
        this.listeners.clear();
    }

    /** Snapshot for inspection (devtools, palette filter debugging). */
    snapshot(): Record<string, ContextKeyValue> {
        const out: Record<string, ContextKeyValue> = {};
        for (const [key, value] of this.values) out[key] = value;
        return out;
    }

    private emit(changedKeys: ReadonlySet<string>): void {
        for (const listener of this.listeners) {
            try {
                listener(changedKeys);
            } catch (err) {
                console.error('[contextKey] listener threw', err);
            }
        }
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
    return parseWhen(source).evaluate(contextKeys);
}

export type { WhenExpr };
