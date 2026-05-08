// Bundle entry point for the operator-zed DSL runtime.
//
// esbuild bundles this into `OUT_DIR/dsl_runtime.js` (see build.rs). The
// resulting script is loaded once into the JsRuntime via `execute_script`
// and exposes `globalThis.modz_executePatchScript` so Rust callers can
// invoke the DSL on demand without re-bundling per execution.

import { executePatchScript } from '../../../src/main/dsl/executor';

declare const Deno: {
    core: {
        ops: Record<string, (...args: unknown[]) => unknown>;
        print?: (msg: string) => void;
    };
};

// Route console.* through op_modz_log (which writes to the host's stderr) so
// stray DSL/library logs don't pollute the --emit-graph stdout JSON.
{
    const log =
        (level: string) =>
        (...args: unknown[]) => {
            const message = args
                .map((a) =>
                    typeof a === 'string'
                        ? a
                        : (() => {
                              try {
                                  return JSON.stringify(a);
                              } catch {
                                  return String(a);
                              }
                          })(),
                )
                .join(' ');
            try {
                Deno.core.ops.op_modz_log(level, message);
            } catch {
                // best effort — drop the message rather than crash the runtime
            }
        };
    (globalThis as Record<string, unknown>).console = {
        log: log('log'),
        info: log('info'),
        warn: log('warn'),
        error: log('error'),
        debug: log('debug'),
        trace: log('trace'),
    };
}

type ModzGlobal = typeof globalThis & {
    modz_executePatchScript: (source: string, schemas: unknown[]) => unknown;
};

function clone(value: unknown): unknown {
    // Maps and Sets don't survive serde_v8 round-trips; convert to arrays.
    if (value instanceof Map) {
        const out: Array<[unknown, unknown]> = [];
        for (const [k, v] of value.entries()) {
            out.push([clone(k), clone(v)]);
        }
        return out;
    }
    if (value instanceof Set) {
        return Array.from(value, clone);
    }
    if (Array.isArray(value)) {
        return value.map(clone);
    }
    if (value && typeof value === 'object') {
        const obj: Record<string, unknown> = {};
        for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
            obj[k] = clone(v);
        }
        return obj;
    }
    return value;
}

(globalThis as ModzGlobal).modz_executePatchScript =
    function modz_executePatchScript(
        source: string,
        schemas: unknown[],
    ): unknown {
        try {
            const result = executePatchScript(
                source,
                schemas as Parameters<typeof executePatchScript>[1],
            );
            // Strip Maps/Sets so serde_v8 deserialization on the Rust side gets
            // plain JSON-shaped values.
            return {
                ok: true,
                value: clone(result),
            };
        } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            return { ok: false, error: message };
        }
    };
