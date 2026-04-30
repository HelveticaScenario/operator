// AUTO-GENERATED — DO NOT EDIT.
// Run `yarn generate-lib` to regenerate.

import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../../runtime/graph';
import type { FactoryFunction } from '../../runtime/factory/namespaceTree';
import { createFactoryFromName } from '../../runtime/factory/createFactoryFromName';

/** Register all `midi` modules into `factories`. */
export function registerMidi(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
    factories: Map<string, FactoryFunction>,
): void {
    factories.set(
        '$midiCV',
        createFactoryFromName(builder, schemas, '$midiCV'),
    );
    factories.set(
        '$midiCC',
        createFactoryFromName(builder, schemas, '$midiCC'),
    );
}
