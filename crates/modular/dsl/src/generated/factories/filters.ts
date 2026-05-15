// AUTO-GENERATED — DO NOT EDIT.
// Run `yarn generate-lib` to regenerate.

import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../../runtime/graph';
import type { FactoryFunction } from '../../runtime/factory/namespaceTree';
import { createFactoryFromName } from '../../runtime/factory/createFactoryFromName';

/** Register all `filters` modules into `factories`. */
export function registerFilters(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
    factories: Map<string, FactoryFunction>,
): void {
    factories.set('$lpf', createFactoryFromName(builder, schemas, '$lpf'));
    factories.set('$hpf', createFactoryFromName(builder, schemas, '$hpf'));
    factories.set('$bpf', createFactoryFromName(builder, schemas, '$bpf'));
    factories.set('$jup6f', createFactoryFromName(builder, schemas, '$jup6f'));
}
