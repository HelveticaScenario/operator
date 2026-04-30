// AUTO-GENERATED — DO NOT EDIT.
// Run `yarn generate-lib` to regenerate.

import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../../runtime/graph';
import type { FactoryFunction } from '../../runtime/factory/namespaceTree';
import { createFactoryFromName } from '../../runtime/factory/createFactoryFromName';

/** Register all `seq` modules into `factories`. */
export function registerSeq(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
    factories: Map<string, FactoryFunction>,
): void {
    factories.set('$cycle', createFactoryFromName(builder, schemas, '$cycle'));
    factories.set('$track', createFactoryFromName(builder, schemas, '$track'));
    factories.set(
        '$iCycle',
        createFactoryFromName(builder, schemas, '$iCycle'),
    );
    factories.set('$step', createFactoryFromName(builder, schemas, '$step'));
}
