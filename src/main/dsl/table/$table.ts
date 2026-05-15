import { replaceSignals } from '../graph';

/**
 * `$table.*` DSL helpers produce phase-warp table descriptors for the
 * `$wavetable` oscillator (and any future modules that accept a `Table`).
 *
 * Each helper returns a plain JSON object whose shape matches the Rust `Table`
 * enum deserializer (`#[serde(tag = "type", rename_all = "camelCase")]`).
 *
 * Inner signal-valued fields are passed through `replaceSignals` so that
 * ModuleOutputs / Collections are converted to the same wire format used for
 * module-factory params.
 *
 * Tables are composable: each returned descriptor has a `.pipe(next)` method
 * that feeds this table's output phase into `next`. The optional second
 * argument to each helper is a shorthand for `.pipe(next)`.
 */
export function wrapTable(descriptor: Record<string, unknown>): Record<
    string,
    unknown
> & {
    pipe: <T>(fn: (self: Record<string, unknown>) => T) => T;
} {
    const t = { ...descriptor } as Record<string, unknown> & {
        pipe: <T>(fn: (self: Record<string, unknown>) => T) => T;
    };
    Object.defineProperty(t, 'pipe', {
        value: <T>(fn: (self: typeof t) => T): T => fn(t),
        enumerable: false,
        writable: false,
        configurable: false,
    });
    return t;
}

export const $table = {
    mirror: (amount: unknown, next?: unknown) => {
        const t = wrapTable({
            type: 'mirror',
            amount: replaceSignals(amount),
        });
        return next !== undefined
            ? wrapTable({ type: 'pipe', first: t, second: next })
            : t;
    },
    bend: (amount: unknown, next?: unknown) => {
        const t = wrapTable({
            type: 'bend',
            amount: replaceSignals(amount),
        });
        return next !== undefined
            ? wrapTable({ type: 'pipe', first: t, second: next })
            : t;
    },
    sync: (ratio: unknown, next?: unknown) => {
        const t = wrapTable({ type: 'sync', ratio: replaceSignals(ratio) });
        return next !== undefined
            ? wrapTable({ type: 'pipe', first: t, second: next })
            : t;
    },
    fold: (amount: unknown, next?: unknown) => {
        const t = wrapTable({
            type: 'fold',
            amount: replaceSignals(amount),
        });
        return next !== undefined
            ? wrapTable({ type: 'pipe', first: t, second: next })
            : t;
    },
    pwm: (width: unknown, next?: unknown) => {
        const t = wrapTable({ type: 'pwm', width: replaceSignals(width) });
        return next !== undefined
            ? wrapTable({ type: 'pipe', first: t, second: next })
            : t;
    },
};
