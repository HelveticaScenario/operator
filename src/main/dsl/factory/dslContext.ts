import type { ModuleSchema } from '@modular/core';
import type { ModuleOutput } from '../graph';
import { Collection, CollectionWithRange, GraphBuilder } from '../graph';
import { captureSourceLocation } from '../captureSourceLocation';
import { sanitizeIdentifier } from './identifiers';
import { buildNamespaceTree } from './namespaceTree';
import type { FactoryFunction, NamespaceTree } from './namespaceTree';
import { createFactory } from './createFactory';

/**
 * DSL Context holds the builder and provides factory functions.
 */
export class DSLContext {
    factories: Record<string, FactoryFunction> = {};
    namespaceTree: NamespaceTree = {};
    private builder: GraphBuilder;

    constructor(schemas: ModuleSchema[]) {
        this.builder = new GraphBuilder(schemas);

        // Build flat factory map (internal use for tree building)
        for (const schema of schemas) {
            const factoryName = sanitizeIdentifier(schema.name);
            this.factories[factoryName] = createFactory(this.builder, schema);
        }

        // Register factories with the builder for late binding so .amplitude(),
        // .shift(), .range() etc. on ModuleOutput/Collection can find them.
        const factoryMap = new Map<string, FactoryFunction>();
        for (const schema of schemas) {
            factoryMap.set(
                schema.name,
                this.factories[sanitizeIdentifier(schema.name)],
            );
        }
        this.builder.setFactoryRegistry(factoryMap);

        this.namespaceTree = buildNamespaceTree(schemas, this.factories);
    }

    getBuilder(): GraphBuilder {
        return this.builder;
    }

    scope<T extends ModuleOutput | Collection | CollectionWithRange>(
        target: T,
        config?: {
            msPerFrame?: number;
            triggerThreshold?: number;
            scale?: number;
        },
    ): T {
        const loc = captureSourceLocation();
        if (
            target instanceof Collection ||
            target instanceof CollectionWithRange
        ) {
            this.builder.addScope([...target], config, loc);
        } else {
            this.builder.addScope(target, config, loc);
        }
        return target;
    }
}
