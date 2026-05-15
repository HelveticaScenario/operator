import type { ModuleSchema } from '@modular/core';
import type { ModuleOutput } from '../graph';
import { Collection, CollectionWithRange, GraphBuilder } from '../graph';
import { captureSourceLocation } from '../captureSourceLocation';
import { sanitizeIdentifier } from './identifiers';
import { buildNamespaceTree } from '../../generated/factories';
import type { FactoryFunction, NamespaceTree } from './namespaceTree';

/**
 * DSL Context holds the builder and provides factory functions.
 *
 * Factories are constructed by the codegen-generated
 * `generated/factories/index.ts::buildNamespaceTree`. Each register call there
 * resolves the schema by name and calls `createFactory(builder, schema)`.
 */
export class DSLContext {
    factories: Record<string, FactoryFunction> = {};
    namespaceTree: NamespaceTree = {};
    private builder: GraphBuilder;

    constructor(schemas: ModuleSchema[]) {
        this.builder = new GraphBuilder(schemas);

        const { factories, namespaceTree } = buildNamespaceTree(
            this.builder,
            schemas,
        );

        // Late-binding registry so .amplitude(), .shift(), .range() etc. on
        // ModuleOutput/Collection can resolve factories.
        this.builder.setFactoryRegistry(factories);

        // Mirror under the sanitized identifier for back-compat with callers
        // that look up factories by JS-identifier name.
        for (const [name, fn] of factories) {
            this.factories[sanitizeIdentifier(name)] = fn;
        }

        this.namespaceTree = namespaceTree;
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
