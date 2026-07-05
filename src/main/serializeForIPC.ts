/**
 * Serialize a value for IPC transfer, handling non-transferable types.
 *
 * `seen` holds the current recursion path, not every visited object: each
 * entry is removed once its subtree is serialized, so a value referenced by
 * two siblings (a DAG) serializes fully both times and only a true cycle is
 * reported as '[Circular]'. Re-expanding shared subtrees is exponential in
 * the worst case, and this runs synchronously on the main process for every
 * intercepted console call, so a node budget bounds the total work — values
 * past it serialize as '[Truncated]'.
 */
const MAX_SERIALIZED_NODES = 10_000;

export function serializeForIPC(value: unknown): unknown {
    return serialize(value, new WeakSet(), {
        remaining: MAX_SERIALIZED_NODES,
    });
}

function serialize(
    value: unknown,
    seen: WeakSet<object>,
    budget: { remaining: number },
): unknown {
    if (value === null || value === undefined) {
        return value;
    }

    if (
        typeof value === 'string' ||
        typeof value === 'number' ||
        typeof value === 'boolean'
    ) {
        return value;
    }

    // The 'n' suffix distinguishes a BigInt from a plain numeric string.
    if (typeof value === 'bigint') {
        return `${value}n`;
    }

    if (typeof value === 'symbol') {
        return value.toString();
    }

    if (typeof value === 'function') {
        return `[Function: ${value.name || 'anonymous'}]`;
    }

    if (value instanceof Error) {
        return {
            __error: true,
            message: value.message,
            name: value.name,
            stack: value.stack,
        };
    }

    if (typeof value === 'object') {
        if (seen.has(value)) {
            return '[Circular]';
        }

        if (budget.remaining <= 0) {
            return '[Truncated]';
        }
        budget.remaining--;

        // Date is a leaf value, no recursion to track.
        if (value instanceof Date) {
            return value.toISOString();
        }

        seen.add(value);
        try {
            if (Array.isArray(value)) {
                return value.map((item) => serialize(item, seen, budget));
            }

            if (value instanceof Map) {
                const obj: Record<string, unknown> = { __type: 'Map' };
                for (const [k, v] of value) {
                    obj[String(k)] = serialize(v, seen, budget);
                }
                return obj;
            }

            if (value instanceof Set) {
                return {
                    __type: 'Set',
                    values: Array.from(value).map((v) =>
                        serialize(v, seen, budget),
                    ),
                };
            }

            const result: Record<string, unknown> = {};
            for (const key of Object.keys(value)) {
                result[key] = serialize(
                    (value as Record<string, unknown>)[key],
                    seen,
                    budget,
                );
            }
            return result;
        } finally {
            seen.delete(value);
        }
    }

    // Fallback (should be unreachable; all types handled above)
    return JSON.stringify(value);
}
