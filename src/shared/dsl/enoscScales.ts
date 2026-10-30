import type { ModuleSchema } from '@modular/core';

/** A preset `$enosc` scale: its `type` tag and its description. */
export interface EnoscScalePreset {
    name: string;
    description: string;
}

/**
 * The preset scales `$enosc`'s `scale` param accepts, read from the engine's
 * `EnoscScale` schema so the DSL can never offer a name the engine rejects.
 * Excludes `custom`, which takes notes. Empty when `$enosc` is absent.
 */
export function enoscScalePresets(schemas: ModuleSchema[]): EnoscScalePreset[] {
    const schema = schemas.find((s) => s.name === '$enosc');
    const params = schema?.paramsSchema as
        | { $defs?: Record<string, { oneOf?: unknown[] }> }
        | undefined;
    const variants = params?.$defs?.EnoscScale?.oneOf ?? [];
    return variants.flatMap((variant) => {
        const v = variant as {
            description?: string;
            properties?: { type?: { const?: unknown } };
        };
        const name = v.properties?.type?.const;
        if (typeof name !== 'string' || name === 'custom') {
            return [];
        }
        return [{ name, description: v.description ?? '' }];
    });
}
