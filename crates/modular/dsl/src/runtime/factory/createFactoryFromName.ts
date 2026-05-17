import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../graph';
import type { FactoryFunction } from './namespaceTree';
import { createFactory } from './createFactory';

/**
 * Look up `moduleName` in `schemas` and return a factory bound to `builder`.
 * Throws if the schema isn't present.
 *
 * Used by the codegen-generated `factories/<category>.ts` files: each register
 * call passes a hardcoded module name and the codegen-time guarantee that
 * `schemas` contains it.
 */
export function createFactoryFromName(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
    moduleName: string,
): FactoryFunction {
    const schema = schemas.find((s) => s.name === moduleName);
    if (!schema) {
        throw new Error(
            `createFactoryFromName: schema not found for module "${moduleName}"`,
        );
    }
    return createFactory(builder, schema);
}
