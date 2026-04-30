import type { ModuleState, PatchGraph } from '@modular/core';
import type { DeferredModuleOutput } from './deferredOutput';
import type { ScopeWithLocation } from './types';
import {
    replaceDeferred,
    replaceDeferredStrings,
    replaceSignals,
} from './signalResolution';

/**
 * Build the deferred-output string-substitution map: stringified deferred ID →
 * stringified resolved-output ID, or null if the deferred was never set.
 */
export function buildDeferredStringMap(
    deferredOutputs: Map<string, DeferredModuleOutput>,
): Map<string, string | null> {
    const map = new Map<string, string | null>();
    for (const deferred of deferredOutputs.values()) {
        const deferredStr = deferred.toString();
        const resolved = deferred.resolve();
        map.set(deferredStr, resolved ? resolved.toString() : null);
    }
    return map;
}

/** Serialize all modules: replace cable refs and deferred-string substitutions. */
export function serializeModules(
    modules: Map<string, ModuleState>,
    deferredOutputs: Map<string, DeferredModuleOutput>,
    deferredStringMap: Map<string, string | null>,
): PatchGraph['modules'] {
    return Array.from(modules.values()).map((m) => {
        // First replace signals (ModuleOutput -> cable objects)
        const replacedParams = replaceDeferred(
            replaceSignals(m.params),
            deferredOutputs,
        );
        // Then replace any deferred strings with resolved strings
        const finalParams = replaceDeferredStrings(
            replacedParams,
            deferredStringMap,
        );
        return {
            ...m,
            params: finalParams as Record<string, unknown>,
        };
    });
}

/**
 * Resolve scope channels through deferred outputs, dropping any scope whose
 * deferred channels never resolved.
 */
export function serializeScopes(
    scopes: ScopeWithLocation[],
    deferredOutputs: Map<string, DeferredModuleOutput>,
): ScopeWithLocation[] {
    return scopes
        .map((scope) => {
            const resolvedChannels = scope.channels.map((ch) => {
                const deferredOutput = deferredOutputs.get(ch.moduleId);
                if (deferredOutput) {
                    const resolved = deferredOutput.resolve();
                    if (resolved) {
                        return {
                            channel: ch.channel,
                            moduleId: resolved.moduleId,
                            portName: resolved.portName,
                        };
                    }
                    return null;
                }
                return ch;
            });
            if (resolvedChannels.some((ch) => ch === null)) {
                return null;
            }
            return {
                ...scope,
                channels: resolvedChannels,
            } as ScopeWithLocation;
        })
        .filter(
            (s: ScopeWithLocation | null): s is ScopeWithLocation => s !== null,
        );
}
