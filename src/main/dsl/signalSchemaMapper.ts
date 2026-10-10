type JsonSchema = Record<string, unknown>;

/** Applies `wrap` to the value at each signal position, keyed by its path. */
export type SignalMapper = (
    value: unknown,
    path: string,
    wrap: (value: unknown, path: string) => unknown,
) => unknown;

const SIGNAL_DEFS = new Set(['PolySignal', 'MonoSignal', 'Signal']);

/** Deeper than any param schema nests signals; also stops recursive defs. */
const MAX_DEPTH = 6;

function isTuple(schema: JsonSchema): boolean {
    return Array.isArray(schema.prefixItems);
}

/**
 * Build a mapper that rewrites the signal positions of a param whose schema
 * nests signals inside arrays, tuples and unions (`$mix.inputs`,
 * `$track.keyframes`, …). Returns null when the schema holds no signal.
 */
export function buildSignalMapper(
    schema: JsonSchema,
    defs: Record<string, JsonSchema> | undefined,
): SignalMapper | null {
    const build = (node: JsonSchema, depth: number): SignalMapper | null => {
        if (depth > MAX_DEPTH) {
            return null;
        }
        if (typeof node.$ref === 'string') {
            const name = node.$ref.replace('#/$defs/', '');
            if (SIGNAL_DEFS.has(name)) {
                return (value, path, wrap) => wrap(value, path);
            }
            // A scale input takes spec strings, never patterns.
            if (name === 'ScaleSignal') {
                return null;
            }
            const def = defs?.[name];
            return def ? build(def, depth + 1) : null;
        }
        if (isTuple(node)) {
            const items = (node.prefixItems as JsonSchema[]).map((item) =>
                build(item, depth + 1),
            );
            if (items.every((m) => m === null)) {
                return null;
            }
            return (value, path, wrap) =>
                Array.isArray(value)
                    ? value.map((v, i) =>
                          items[i] ? items[i](v, `${path}.${i}`, wrap) : v,
                      )
                    : value;
        }
        if (node.type === 'array' && node.items && !Array.isArray(node.items)) {
            const item = build(node.items as JsonSchema, depth + 1);
            return item
                ? (value, path, wrap) =>
                      Array.isArray(value)
                          ? value.map((v, i) => item(v, `${path}.${i}`, wrap))
                          : value
                : null;
        }
        const union = (node.anyOf ?? node.oneOf) as JsonSchema[] | undefined;
        if (union) {
            const branches = union.map((branch) => ({
                mapper: build(branch, depth + 1),
                tuple: isTuple(branch) ? branch : null,
            }));
            if (branches.every((b) => b.mapper === null)) {
                return null;
            }
            return (value, path, wrap) => {
                // A tuple branch applies to an array of exactly its length
                // whose array-typed slots hold arrays; any other value is a
                // plain signal.
                const picked = branches.find(
                    (b) =>
                        b.mapper &&
                        (b.tuple
                            ? tupleFits(b.tuple, value)
                            : !branches.some(
                                  (o) => o.tuple && tupleFits(o.tuple, value),
                              )),
                );
                return picked?.mapper
                    ? picked.mapper(value, path, wrap)
                    : value;
            };
        }
        return null;
    };
    return build(schema, 0);
}

function tupleFits(tuple: JsonSchema, value: unknown): boolean {
    const slots = tuple.prefixItems as JsonSchema[];
    return (
        Array.isArray(value) &&
        value.length === slots.length &&
        slots.every(
            (slot, i) => slot.type !== 'array' || Array.isArray(value[i]),
        )
    );
}
