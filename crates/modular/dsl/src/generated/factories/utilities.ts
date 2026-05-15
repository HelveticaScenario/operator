// AUTO-GENERATED — DO NOT EDIT.
// Run `yarn generate-lib` to regenerate.

import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../../runtime/graph';
import type { FactoryFunction } from '../../runtime/factory/namespaceTree';
import { createFactoryFromName } from '../../runtime/factory/createFactoryFromName';

/** Register all `utilities` modules into `factories`. */
export function registerUtilities(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
    factories: Map<string, FactoryFunction>,
): void {
    factories.set('$adsr', createFactoryFromName(builder, schemas, '$adsr'));
    factories.set(
        '$bufRead',
        createFactoryFromName(builder, schemas, '$bufRead'),
    );
    factories.set(
        '$buffer',
        createFactoryFromName(builder, schemas, '$buffer'),
    );
    factories.set('$clamp', createFactoryFromName(builder, schemas, '$clamp'));
    factories.set(
        '$clockDivider',
        createFactoryFromName(builder, schemas, '$clockDivider'),
    );
    factories.set('$curve', createFactoryFromName(builder, schemas, '$curve'));
    factories.set(
        '$delayRead',
        createFactoryFromName(builder, schemas, '$delayRead'),
    );
    factories.set('$slew', createFactoryFromName(builder, schemas, '$slew'));
    factories.set(
        '$rising',
        createFactoryFromName(builder, schemas, '$rising'),
    );
    factories.set(
        '$falling',
        createFactoryFromName(builder, schemas, '$falling'),
    );
    factories.set('$math', createFactoryFromName(builder, schemas, '$math'));
    factories.set('$remap', createFactoryFromName(builder, schemas, '$remap'));
    factories.set('$sah', createFactoryFromName(builder, schemas, '$sah'));
    factories.set('$tah', createFactoryFromName(builder, schemas, '$tah'));
    factories.set('$perc', createFactoryFromName(builder, schemas, '$perc'));
    factories.set(
        '$quantizer',
        createFactoryFromName(builder, schemas, '$quantizer'),
    );
    factories.set(
        '$scaleAndShift',
        createFactoryFromName(builder, schemas, '$scaleAndShift'),
    );
    factories.set(
        '$spread',
        createFactoryFromName(builder, schemas, '$spread'),
    );
    factories.set(
        '$unison',
        createFactoryFromName(builder, schemas, '$unison'),
    );
    factories.set('$wrap', createFactoryFromName(builder, schemas, '$wrap'));
}
