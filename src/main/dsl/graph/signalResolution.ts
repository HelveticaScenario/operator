import { BaseCollection } from './collection';
import { ModuleOutput } from './moduleOutput';
import { DeferredModuleOutput } from './deferredOutput';
import { ResolvedModuleOutputSchema } from './types';

type Replacer = (key: string, value: unknown) => unknown;

export function replaceValues(input: unknown, replacer: Replacer): unknown {
    function walk(key: string, value: unknown): unknown {
        const replaced = replacer(key, value);

        // Match JSON.stringify behavior
        if (replaced === undefined) {
            return undefined;
        }

        if (typeof replaced !== 'object' || replaced === null) {
            return replaced;
        }

        if (Array.isArray(replaced)) {
            return replaced
                .map((v, i) => walk(String(i), v))
                .filter((v) => v !== undefined);
        }

        const out: Record<string, unknown> = {};
        for (const [k, v] of Object.entries(replaced)) {
            const entryVal = walk(k, v);
            if (entryVal !== undefined) {
                out[k] = entryVal;
            }
        }
        return out;
    }

    // JSON.stringify starts with key ""
    return walk('', input);
}

export function valueToSignal(value: unknown): unknown {
    if (value instanceof ModuleOutput) {
        return {
            channel: value.channel,
            module: value.moduleId,
            port: value.portName,
            type: 'cable',
        };
    } else if (value === null || value === undefined) {
        // Silence: 0 becomes Signal::Volts(0.0) in Rust
        return 0;
    }
    return value;
}

export function replaceSignals(input: unknown): unknown {
    return replaceValues(input, (_key, value) => {
        if (value instanceof BaseCollection) {
            return [...value];
        }
        return valueToSignal(value);
    });
}

/**
 * Recursively replace deferred output strings with resolved output strings in params.
 * Handles cases where a DeferredModuleOutput was stringified (e.g., in pattern strings).
 */
export function replaceDeferredStrings(
    input: unknown,
    deferredStringMap: Map<string, string | null>,
): unknown {
    if (typeof input === 'string') {
        let result = input;
        for (const [deferredStr, resolvedStr] of deferredStringMap) {
            const splitResult = result.split(deferredStr);
            if (splitResult.length > 1) {
                if (resolvedStr === null) {
                    throw new Error(
                        `Unset DeferredModuleOutput used in string: "${input}"`,
                    );
                }
                result = splitResult.join(resolvedStr);
            }
        }
        return result;
    }

    if (Array.isArray(input)) {
        return input.map((item) =>
            replaceDeferredStrings(item, deferredStringMap),
        );
    }

    if (typeof input === 'object' && input !== null) {
        const result: Record<string, unknown> = {};
        for (const [key, value] of Object.entries(input)) {
            result[key] = replaceDeferredStrings(value, deferredStringMap);
        }
        return result;
    }

    return input;
}

/**
 * Replace cable refs that point at deferred outputs with their resolved targets.
 * Used internally by GraphBuilder.toPatch().
 */
export function replaceDeferred(
    input: unknown,
    deferredOutputs: Map<string, DeferredModuleOutput>,
): unknown {
    function replace(value: unknown): unknown {
        const maybeResolvedModuleOutput =
            ResolvedModuleOutputSchema.safeParse(value);
        if (maybeResolvedModuleOutput.success) {
            const resolved = deferredOutputs.get(
                maybeResolvedModuleOutput.data.module,
            );
            if (resolved) {
                return valueToSignal(resolved.resolve());
            }
            return maybeResolvedModuleOutput.data;
        }
        return value;
    }
    return replaceValues(input, (_key, value) => {
        if (value instanceof BaseCollection) {
            return [...value];
        }
        return replace(value);
    });
}
