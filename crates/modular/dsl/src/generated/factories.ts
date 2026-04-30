// AUTO-GENERATED — DO NOT EDIT.
// Run `yarn generate-lib` to regenerate.

import type { ModuleSchema } from '@modular/core';
import type { GraphBuilder } from '../runtime/graph';
import { buildNamespaceTree as buildNamespaceTreeFromFactories } from '../runtime/factory/namespaceTree';
import type {
    FactoryFunction,
    NamespaceTree,
} from '../runtime/factory/namespaceTree';
import { createFactoryFromName } from '../runtime/factory/createFactoryFromName';

/** Register every schema's factory into a flat name → factory map. */
export function buildAllFactories(
    builder: GraphBuilder,
    schemas: ModuleSchema[],
): Map<string, FactoryFunction> {
    const factories = new Map<string, FactoryFunction>();
    factories.set('$adsr', createFactoryFromName(builder, schemas, '$adsr'));
    factories.set('$bpf', createFactoryFromName(builder, schemas, '$bpf'));
    factories.set(
        '$bufRead',
        createFactoryFromName(builder, schemas, '$bufRead'),
    );
    factories.set(
        '$buffer',
        createFactoryFromName(builder, schemas, '$buffer'),
    );
    factories.set('$cheby', createFactoryFromName(builder, schemas, '$cheby'));
    factories.set('$clamp', createFactoryFromName(builder, schemas, '$clamp'));
    factories.set(
        '$clockDivider',
        createFactoryFromName(builder, schemas, '$clockDivider'),
    );
    factories.set('$comp', createFactoryFromName(builder, schemas, '$comp'));
    factories.set('$crush', createFactoryFromName(builder, schemas, '$crush'));
    factories.set('$curve', createFactoryFromName(builder, schemas, '$curve'));
    factories.set('$cycle', createFactoryFromName(builder, schemas, '$cycle'));
    factories.set(
        '$dattorro',
        createFactoryFromName(builder, schemas, '$dattorro'),
    );
    factories.set(
        '$delayRead',
        createFactoryFromName(builder, schemas, '$delayRead'),
    );
    factories.set(
        '$falling',
        createFactoryFromName(builder, schemas, '$falling'),
    );
    factories.set(
        '$feedback',
        createFactoryFromName(builder, schemas, '$feedback'),
    );
    factories.set('$fold', createFactoryFromName(builder, schemas, '$fold'));
    factories.set('$hpf', createFactoryFromName(builder, schemas, '$hpf'));
    factories.set(
        '$iCycle',
        createFactoryFromName(builder, schemas, '$iCycle'),
    );
    factories.set('$jup6f', createFactoryFromName(builder, schemas, '$jup6f'));
    factories.set('$lpf', createFactoryFromName(builder, schemas, '$lpf'));
    factories.set('$macro', createFactoryFromName(builder, schemas, '$macro'));
    factories.set('$math', createFactoryFromName(builder, schemas, '$math'));
    factories.set(
        '$midiCC',
        createFactoryFromName(builder, schemas, '$midiCC'),
    );
    factories.set(
        '$midiCV',
        createFactoryFromName(builder, schemas, '$midiCV'),
    );
    factories.set('$mix', createFactoryFromName(builder, schemas, '$mix'));
    factories.set('$noise', createFactoryFromName(builder, schemas, '$noise'));
    factories.set(
        '$overdrive',
        createFactoryFromName(builder, schemas, '$overdrive'),
    );
    factories.set(
        '$pPulse',
        createFactoryFromName(builder, schemas, '$pPulse'),
    );
    factories.set('$pSaw', createFactoryFromName(builder, schemas, '$pSaw'));
    factories.set('$pSine', createFactoryFromName(builder, schemas, '$pSine'));
    factories.set('$perc', createFactoryFromName(builder, schemas, '$perc'));
    factories.set('$plate', createFactoryFromName(builder, schemas, '$plate'));
    factories.set(
        '$pulsar',
        createFactoryFromName(builder, schemas, '$pulsar'),
    );
    factories.set('$pulse', createFactoryFromName(builder, schemas, '$pulse'));
    factories.set(
        '$quantizer',
        createFactoryFromName(builder, schemas, '$quantizer'),
    );
    factories.set('$ramp', createFactoryFromName(builder, schemas, '$ramp'));
    factories.set('$remap', createFactoryFromName(builder, schemas, '$remap'));
    factories.set(
        '$rising',
        createFactoryFromName(builder, schemas, '$rising'),
    );
    factories.set('$sah', createFactoryFromName(builder, schemas, '$sah'));
    factories.set(
        '$sampler',
        createFactoryFromName(builder, schemas, '$sampler'),
    );
    factories.set('$saw', createFactoryFromName(builder, schemas, '$saw'));
    factories.set(
        '$scaleAndShift',
        createFactoryFromName(builder, schemas, '$scaleAndShift'),
    );
    factories.set(
        '$segment',
        createFactoryFromName(builder, schemas, '$segment'),
    );
    factories.set(
        '$signal',
        createFactoryFromName(builder, schemas, '$signal'),
    );
    factories.set('$sine', createFactoryFromName(builder, schemas, '$sine'));
    factories.set('$slew', createFactoryFromName(builder, schemas, '$slew'));
    factories.set(
        '$spread',
        createFactoryFromName(builder, schemas, '$spread'),
    );
    factories.set('$step', createFactoryFromName(builder, schemas, '$step'));
    factories.set(
        '$stereoMix',
        createFactoryFromName(builder, schemas, '$stereoMix'),
    );
    factories.set(
        '$supersaw',
        createFactoryFromName(builder, schemas, '$supersaw'),
    );
    factories.set('$tah', createFactoryFromName(builder, schemas, '$tah'));
    factories.set('$track', createFactoryFromName(builder, schemas, '$track'));
    factories.set(
        '$unison',
        createFactoryFromName(builder, schemas, '$unison'),
    );
    factories.set(
        '$wavetable',
        createFactoryFromName(builder, schemas, '$wavetable'),
    );
    factories.set('$wrap', createFactoryFromName(builder, schemas, '$wrap'));
    factories.set('$xover', createFactoryFromName(builder, schemas, '$xover'));
    factories.set('_clock', createFactoryFromName(builder, schemas, '_clock'));
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
