// AUTO-GENERATED — DO NOT EDIT.
// Run `yarn generate-lib` to regenerate.

import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../../runtime/graph';
import type { FactoryFunction } from '../../runtime/factory/namespaceTree';
import { createFactoryFromName } from '../../runtime/factory/createFactoryFromName';

/** Register all `core` modules into `factories`. */
export function registerCore(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
    factories: Map<string, FactoryFunction>,
): void {
    factories.set(
        '$signal',
        createFactoryFromName(builder, schemas, '$signal'),
    );
    factories.set('$mix', createFactoryFromName(builder, schemas, '$mix'));
    factories.set(
        '$stereoMix',
        createFactoryFromName(builder, schemas, '$stereoMix'),
    );
    factories.set('_clock', createFactoryFromName(builder, schemas, '_clock'));
}
