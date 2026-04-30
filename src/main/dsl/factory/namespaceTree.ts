import type { ModuleSchema } from '@modular/core';
import type { Collection, CollectionWithRange, ModuleOutput } from '../graph';
import { sanitizeIdentifier } from './identifiers';

type SingleOutput = ModuleOutput;
type PolyOutput = Collection | CollectionWithRange;
export type MultiOutput = (SingleOutput | PolyOutput) &
    Record<string, ModuleOutput | Collection | CollectionWithRange>;
export type ModuleReturn = SingleOutput | PolyOutput | MultiOutput;

export type FactoryFunction = (...args: any[]) => ModuleReturn;

export interface NamespaceTree {
    [key: string]: NamespaceTree | FactoryFunction;
}

/**
 * Build a nested namespace tree from module schemas.
 * Mirrors the logic in typescriptLibGen.ts buildTreeFromSchemas().
 */
export function buildNamespaceTree(
    schemas: ModuleSchema[],
    factoryMap: Record<string, FactoryFunction>,
): NamespaceTree {
    const tree: NamespaceTree = {};

    for (const schema of schemas) {
        const fullName = schema.name.trim();
        const parts = fullName.split('.').filter((p) => p.length > 0);

        const factoryName = sanitizeIdentifier(fullName);
        const factory = factoryMap[factoryName];

        if (parts.length === 1) {
            // No namespace, add to root
            tree[parts[0]] = factory;
        } else {
            // Navigate/create namespace hierarchy
            const className = parts[parts.length - 1];
            const namespacePath = parts.slice(0, -1);

            let current: NamespaceTree = tree;
            for (const ns of namespacePath) {
                if (!current[ns]) {
                    current[ns] = {};
                } else if (typeof current[ns] === 'function') {
                    throw new Error(
                        `Namespace collision: ${ns} is both a module and a namespace`,
                    );
                }
                current = current[ns];
            }

            if (
                current[className] &&
                typeof current[className] !== 'function'
            ) {
                throw new Error(
                    `Module name collision: ${className} already exists as a namespace`,
                );
            }
            current[className] = factory;
        }
    }

    return tree;
}
