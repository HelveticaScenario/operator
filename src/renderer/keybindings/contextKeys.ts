/**
 * Minimal context-key signal store.
 *
 * Surfaces (palette, keymap, context menus) read these keys to evaluate
 * `when` clauses. Phase 2.2 introduces a full vscode-style context-key
 * service with a when-parser; until then this module only supports
 * scalar set/get/subscribe. Keep the API stable so 2.2 can drop in
 * an extended implementation without touching call sites.
 */

export type ContextKeyValue = string | number | boolean | null | undefined;

type Listener = (key: string, value: ContextKeyValue) => void;

const state = new Map<string, ContextKeyValue>();
const listeners = new Set<Listener>();

export function setContextKey(key: string, value: ContextKeyValue): void {
    if (state.get(key) === value) return;
    state.set(key, value);
    for (const listener of listeners) {
        listener(key, value);
    }
}

export function getContextKey(key: string): ContextKeyValue {
    return state.get(key);
}

export function subscribeContextKeys(listener: Listener): () => void {
    listeners.add(listener);
    return () => {
        listeners.delete(listener);
    };
}
