// AUTO-GENERATED — DO NOT EDIT.
// Run `yarn generate-lib` to regenerate.

import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../../runtime/graph';
import type { FactoryFunction } from '../../runtime/factory/namespaceTree';
import { createFactoryFromName } from '../../runtime/factory/createFactoryFromName';

/** Register all `oscillators` modules into `factories`. */
export function registerOscillators(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
    factories: Map<string, FactoryFunction>,
): void {
    factories.set('$sine', createFactoryFromName(builder, schemas, '$sine'));
    factories.set('$saw', createFactoryFromName(builder, schemas, '$saw'));
    factories.set('$pulse', createFactoryFromName(builder, schemas, '$pulse'));
    factories.set('$pSine', createFactoryFromName(builder, schemas, '$pSine'));
    factories.set('$pSaw', createFactoryFromName(builder, schemas, '$pSaw'));
    factories.set(
        '$pPulse',
        createFactoryFromName(builder, schemas, '$pPulse'),
    );
    factories.set('$noise', createFactoryFromName(builder, schemas, '$noise'));
    factories.set('$macro', createFactoryFromName(builder, schemas, '$macro'));
    factories.set(
        '$supersaw',
        createFactoryFromName(builder, schemas, '$supersaw'),
    );
    factories.set(
        '$wavetable',
        createFactoryFromName(builder, schemas, '$wavetable'),
    );
}
