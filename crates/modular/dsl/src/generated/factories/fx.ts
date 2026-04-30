// AUTO-GENERATED — DO NOT EDIT.
// Run `yarn generate-lib` to regenerate.

import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../../runtime/graph';
import type { FactoryFunction } from '../../runtime/factory/namespaceTree';
import { createFactoryFromName } from '../../runtime/factory/createFactoryFromName';

/** Register all `fx` modules into `factories`. */
export function registerFx(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
    factories: Map<string, FactoryFunction>,
): void {
    factories.set('$fold', createFactoryFromName(builder, schemas, '$fold'));
    factories.set('$cheby', createFactoryFromName(builder, schemas, '$cheby'));
    factories.set(
        '$dattorro',
        createFactoryFromName(builder, schemas, '$dattorro'),
    );
    factories.set(
        '$overdrive',
        createFactoryFromName(builder, schemas, '$overdrive'),
    );
    factories.set('$plate', createFactoryFromName(builder, schemas, '$plate'));
    factories.set(
        '$segment',
        createFactoryFromName(builder, schemas, '$segment'),
    );
}
