/** Emitted texts kept while awaiting their echo; older ones are dropped. */
const MAX_PENDING = 32;

/**
 * Decides whether a `value` prop coming back from React state must be written
 * into the editor model.
 *
 * Each edit is emitted to the parent and returns later as `value`, possibly
 * after newer edits: an update from an event React does not recognise (such
 * as the EditContext `textupdate` that delivers typed characters) renders at
 * default priority, so its effects run in a later task. A value matching an
 * emitted text is the parent catching up and is never written; only a value
 * the editor did not produce (a disk reload, a migration) replaces the model.
 */
export interface ModelValueSync {
    /** Records text the editor emitted to the parent. */
    noteEmitted(text: string): void;
    /** Whether `value` must replace `modelText`, the model's current text. */
    shouldWrite(value: string, modelText: string): boolean;
    /** Forgets emitted texts; call when the editor switches models. */
    reset(): void;
}

export function createModelValueSync(): ModelValueSync {
    let pending: string[] = [];
    return {
        noteEmitted(text) {
            pending.push(text);
            if (pending.length > MAX_PENDING) {
                pending.shift();
            }
        },
        shouldWrite(value, modelText) {
            if (value === modelText) {
                pending = [];
                return false;
            }
            const echoed = pending.indexOf(value);
            if (echoed !== -1) {
                pending = pending.slice(echoed + 1);
                return false;
            }
            pending = [];
            return true;
        },
        reset() {
            pending = [];
        },
    };
}
