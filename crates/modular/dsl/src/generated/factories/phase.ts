// AUTO-GENERATED — DO NOT EDIT.
// Run `yarn generate-lib` to regenerate.

import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../../runtime/graph';
import type { FactoryFunction } from '../../runtime/factory/namespaceTree';
import { createFactoryFromName } from '../../runtime/factory/createFactoryFromName';

/** Register all `phase` modules into `factories`. */
export function registerPhase(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
    factories: Map<string, FactoryFunction>,
): void {
    factories.set('$crush', createFactoryFromName(builder, schemas, '$crush'));
    factories.set(
        '$feedback',
        createFactoryFromName(builder, schemas, '$feedback'),
    );
    factories.set(
        '$pulsar',
        createFactoryFromName(builder, schemas, '$pulsar'),
    );
    factories.set('$ramp', createFactoryFromName(builder, schemas, '$ramp'));
}
