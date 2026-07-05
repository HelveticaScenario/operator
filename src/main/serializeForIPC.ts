/**
 * Serialize a value for IPC transfer, handling non-transferable types
 */
export function serializeForIPC(
    value: unknown,
    seen = new WeakSet<object>(),
): unknown {
    // Handle primitives
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

    // Handle BigInt by converting to string with 'n' suffix for clarity
    if (typeof value === 'bigint') {
        return `${value}n`;
    }

    // Handle symbols
    if (typeof value === 'symbol') {
        return value.toString();
    }

    // Handle functions
    if (typeof value === 'function') {
        return `[Function: ${value.name || 'anonymous'}]`;
    }

    // Handle Error objects specially
    if (value instanceof Error) {
        return {
            __error: true,
            message: value.message,
            name: value.name,
            stack: value.stack,
        };
    }

    // Handle objects and arrays
    if (typeof value === 'object') {
        // Detect circular references
        if (seen.has(value)) {
            return '[Circular]';
        }
        seen.add(value);

        // Handle arrays
        if (Array.isArray(value)) {
            return value.map((item) => serializeForIPC(item, seen));
        }

        // Handle Date
        if (value instanceof Date) {
            return value.toISOString();
        }

        // Handle Map
        if (value instanceof Map) {
            const obj: Record<string, unknown> = { __type: 'Map' };
            for (const [k, v] of value) {
                obj[String(k)] = serializeForIPC(v, seen);
            }
            return obj;
        }

        // Handle Set
        if (value instanceof Set) {
            return {
                __type: 'Set',
                values: Array.from(value).map((v) => serializeForIPC(v, seen)),
            };
        }

        // Handle plain objects
        const result: Record<string, unknown> = {};
        for (const key of Object.keys(value)) {
            result[key] = serializeForIPC(
                (value as Record<string, unknown>)[key],
                seen,
            );
        }
        return result;
    }

    // Fallback (should be unreachable; all types handled above)
    return JSON.stringify(value);
}
