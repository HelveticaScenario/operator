// AUTO-GENERATED — DO NOT EDIT.
// Run `yarn generate-lib` to regenerate.

import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../../runtime/graph';
import { buildNamespaceTree as buildNamespaceTreeFromFactories } from '../../runtime/factory/namespaceTree';
import type {
    FactoryFunction,
    NamespaceTree,
} from '../../runtime/factory/namespaceTree';

import { registerCore } from './core';
import { registerDynamics } from './dynamics';
import { registerFx } from './fx';
import { registerOscillators } from './oscillators';
import { registerFilters } from './filters';
import { registerPhase } from './phase';
import { registerUtilities } from './utilities';
import { registerSeq } from './seq';
import { registerMidi } from './midi';
import { registerSamplers } from './samplers';

/** Register every category's factories into a flat name → factory map. */
export function buildAllFactories(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
): Map<string, FactoryFunction> {
    const factories = new Map<string, FactoryFunction>();
    registerCore(builder, schemas, factories);
    registerDynamics(builder, schemas, factories);
    registerFx(builder, schemas, factories);
    registerOscillators(builder, schemas, factories);
    registerFilters(builder, schemas, factories);
    registerPhase(builder, schemas, factories);
    registerUtilities(builder, schemas, factories);
    registerSeq(builder, schemas, factories);
    registerMidi(builder, schemas, factories);
    registerSamplers(builder, schemas, factories);
    return factories;
}

/** Build the user-facing nested DSL namespace tree from the flat factory map. */
export function buildNamespaceTree(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
): { factories: Map<string, FactoryFunction>; namespaceTree: NamespaceTree } {
    const factories = buildAllFactories(builder, schemas);
    const flatMap: Record<string, FactoryFunction> = {};
    for (const [name, fn] of factories) {
        flatMap[sanitizeIdentifier(name)] = fn;
    }
    return {
        factories,
        namespaceTree: buildNamespaceTreeFromFactories(schemas, flatMap),
    };
}

function sanitizeIdentifier(name: string): string {
    let id = name.replace(/[^a-zA-Z0-9_$]+(.)?/g, (_match, chr) =>
        chr ? chr.toUpperCase() : '',
    );
    if (!/^[A-Za-z_$]/.test(id)) id = `_${id}`;
    return id || '_';
}
