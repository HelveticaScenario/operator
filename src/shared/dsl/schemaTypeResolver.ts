/**
 * Shared JSON Schema → TypeScript type expression resolver.
 *
 * Used by both the main-process type generation (typescriptLibGen)
 * and the renderer help window to display human-readable param types.
 */

import type schemas from '@modular/core/schemas.json';

/**
 * A single module schema entry, typed from the actual schemas.json data.
 * Replaces the looser hand-written `ModuleSchema` interface from N-API.
 */
export type Schema = (typeof schemas)[number];

/** The full schemas array type inferred from schemas.json. */
export type Schemas = typeof schemas;

/**
 * Structural type for a JSON Schema node.
 * Used when traversing sub-schemas within a module's `paramsSchema`.
 */
export interface JSONSchema {
    readonly type?: string | readonly string[];
    readonly $ref?: string;
    readonly oneOf?: readonly JSONSchema[];
    readonly anyOf?: readonly JSONSchema[];
    readonly allOf?: readonly JSONSchema[];
    readonly enum?: readonly unknown[];
    readonly const?: unknown;
    readonly properties?: Readonly<Record<string, JSONSchema | undefined>>;
    readonly required?: readonly string[];
    readonly items?: JSONSchema;
    readonly prefixItems?: readonly JSONSchema[];
    readonly $defs?: Readonly<Record<string, JSONSchema | undefined>>;
    readonly title?: string;
    readonly description?: string;
    readonly default?: unknown;
    readonly format?: string;
    readonly minimum?: number;
    readonly maximum?: number;
}

function isValidIdentifier(name: string): boolean {
    return /^[$A-Z_][0-9A-Z_$]*$/i.test(name);
}

function renderPropertyKey(name: string): string {
    return isValidIdentifier(name) ? name : JSON.stringify(name);
}

/**
 * Resolve a `$ref` pointer against the root schema's `$defs`.
 * Returns the well-known Signal/Poly<Signal>/Mono<Signal>/Buffer sentinel strings
 * for the three signal types, otherwise the raw resolved sub-schema.
 */
export function resolveRef(
    ref: string,
    rootSchema: JSONSchema,
):
    | JSONSchema
    | 'Signal'
    | 'Poly<Signal>'
    | 'Mono<Signal>'
    | 'Buffer'
    | 'Table'
    | 'ParsedPattern' {
    if (ref === 'Signal') {
        return 'Signal';
    }
    if (ref === 'Buffer') {
        return 'Buffer';
    }
    if (ref === 'Table') {
        return 'Table';
    }
    if (ref === 'ParsedPatternPayload') {
        return 'ParsedPattern';
    }

    const defsPrefix = '#/$defs/';
    if (!ref.startsWith(defsPrefix)) {
        throw new Error(`Unsupported $ref: ${ref}`);
    }

    const defName = ref.slice(defsPrefix.length);
    if (defName === 'Signal') {
        return 'Signal';
    }
    if (defName === 'PolySignal') {
        return 'Poly<Signal>';
    }
    if (defName === 'MonoSignal') {
        return 'Mono<Signal>';
    }
    if (defName === 'Buffer') {
        return 'Buffer';
    }
    if (defName === 'Table') {
        return 'Table';
    }
    // ParsedPatternPayload is recursive (MiniAST references itself via
    // List/Sequence/...); the DSL exposes it as an opaque `ParsedPattern`
    // named type, constructed by `$p(...)` and consumed by $cycle/$iCycle.
    if (defName === 'ParsedPatternPayload') {
        return 'ParsedPattern';
    }

    const defs = rootSchema?.$defs;
    if (!defs || typeof defs !== 'object') {
        throw new Error(`Unresolved $ref: ${ref}`);
    }

    const resolved = defs[defName];
    if (!resolved) {
        throw new Error(`Unresolved $ref: ${ref}`);
    }

    if (resolved?.title === 'Signal') {
        return 'Signal';
    }
    if (resolved?.title === 'PolySignal') {
        return 'Poly<Signal>';
    }
    if (resolved?.title === 'MonoSignal') {
        return 'Mono<Signal>';
    }
    if (resolved?.title === 'Buffer') {
        return 'Buffer';
    }
    if (resolved?.title === 'Table') {
        return 'Table';
    }
    if (resolved?.title === 'ParsedPatternPayload') {
        return 'ParsedPattern';
    }
    return resolved;
}

/**
 * Information about a single variant in an enum-style JSON Schema.
 */
export interface EnumVariantInfo {
    /** The JSON-serialized const value (e.g., `"vaVcf"`) */
    value: string;
    /** The raw const value */
    rawValue: unknown;
    /** Description from the Rust `///` doc comment on the variant, if any */
    description?: string;
}

/**
 * Extract enum variant information (including descriptions) from a JSON Schema node.
 *
 * Returns an array of `EnumVariantInfo` if the schema represents an enum,
 * or `null` if it does not.  Follows `$ref` pointers, handling the
 * Signal/Poly/Mono sentinel types gracefully (returns `null` for those).
 */
export function getEnumVariants(
    schema: JSONSchema,
    rootSchema: JSONSchema,
): EnumVariantInfo[] | null {
    if (!schema || typeof schema !== 'object' || Array.isArray(schema)) {
        return null;
    }

    // Follow $ref
    if (schema.$ref) {
        const resolved = resolveRef(schema.$ref, rootSchema);
        // Sentinel strings mean it's a signal type, not an enum
        if (typeof resolved === 'string') {
            return null;
        }
        return getEnumVariants(resolved, rootSchema);
    }

    // OneOf / anyOf with all-const variants (schemars output for documented enums)
    const variants = schema.oneOf || schema.anyOf;
    if (variants !== undefined) {
        const isEnum = variants.every((v: JSONSchema) => v.const !== undefined);
        if (isEnum) {
            return variants.map((v) => ({
                description: v.description,
                rawValue: v.const,
                value: JSON.stringify(v.const),
            }));
        }
        return null;
    }

    // Bare enum array (schemars output for undocumented enums)
    if (schema.enum !== undefined && schema.enum.length > 0) {
        return schema.enum.map((v) => ({
            description: undefined,
            rawValue: v,
            value: JSON.stringify(v),
        }));
    }

    return null;
}

/**
 * Convert a JSON Schema node into a human-readable TypeScript type expression.
 *
 * Handles: `$ref` (Signal/PolySignal/MonoSignal + arbitrary defs), `oneOf`/`anyOf`
 * enum patterns, `allOf`, nullable type arrays, primitive types, object types,
 * array/tuple types, and `enum` schemas.
 */
export function schemaToTypeExpr(
    schema: JSONSchema,
    rootSchema: JSONSchema,
): string {
    // Handle oneOf/anyOf - check if all variants resolve to Signal
    if (schema.oneOf || schema.anyOf) {
        const variants = schema.oneOf || schema.anyOf;
        if (variants !== undefined) {
            // Filter out null variants (from Rust Option<T>); optionality is
            // Handled at the param level with `?:` in TypeScript.
            const nonNullVariants = variants.filter(
                (v: JSONSchema) =>
                    !(v && typeof v === 'object' && v.type === 'null'),
            );

            // If all non-null variants were filtered out, treat as unknown
            if (nonNullVariants.length === 0) {
                return 'unknown';
            }

            // If only one non-null variant remains, resolve it directly
            if (nonNullVariants.length === 1) {
                return schemaToTypeExpr(nonNullVariants[0], rootSchema);
            }

            // Check if this is an enum (all variants have 'const')
            const isEnum = nonNullVariants.every(
                (v: JSONSchema) => v.const !== undefined,
            );
            if (isEnum) {
                return nonNullVariants
                    .map((v: JSONSchema) => JSON.stringify(v.const))
                    .join(' | ');
            }

            const types = nonNullVariants.map((v: JSONSchema) =>
                schemaToTypeExpr(v, rootSchema),
            );
            // If all variants are Signal, return Signal
            if (types.every((t) => t === 'Signal')) {
                return 'Poly<Signal>';
            }
            // If it's a mix but includes Signal[], treat as Signal (for Poly<Signal>)
            if (types.includes('Signal') && types.includes('Signal[]')) {
                return 'Poly<Signal>';
            }

            // Otherwise, return the union of all variant types
            return types.join(' | ');
        }
    }
    if (schema.allOf) {
        return 'any';
    }
    if (schema.type !== undefined && typeof schema.type !== 'string') {
        // Handle union type arrays like ["integer", "null"] or ["string", "null"]
        const nonNullTypes = schema.type.filter((t) => t !== 'null');
        if (nonNullTypes.length === 1) {
            // This is a nullable type, treat it as the non-null type (optional in TS)
            const singleType = nonNullTypes[0];
            if (singleType === 'integer' || singleType === 'number') {
                return 'number';
            }
            if (singleType === 'string') {
                return 'string';
            }
            if (singleType === 'boolean') {
                return 'boolean';
            }
        }
        // Fall back to union of all non-null types
        const mapped = nonNullTypes.map((t) => {
            if (t === 'integer' || t === 'number') {
                return 'number';
            }
            if (t === 'string') {
                return 'string';
            }
            if (t === 'boolean') {
                return 'boolean';
            }
            return 'any';
        });
        return mapped.length > 0 ? mapped.join(' | ') : 'any';
    }

    if (schema.$ref) {
        const resolved = resolveRef(schema.$ref, rootSchema);
        if (resolved === 'Signal') {
            return 'Signal';
        }
        if (resolved === 'Poly<Signal>') {
            return 'Poly<Signal>';
        }
        if (resolved === 'Mono<Signal>') {
            return 'Mono<Signal>';
        }
        if (resolved === 'Buffer') {
            return 'BufferOutputRef';
        }
        if (resolved === 'Table') {
            return 'Table';
        }
        if (resolved === 'ParsedPattern') {
            return 'ParsedPattern';
        }
        return schemaToTypeExpr(resolved, rootSchema);
    }

    if (schema.enum) {
        if (schema.enum.length === 0) {
            throw new Error('Unsupported enum schema');
        }
        return schema.enum.map((v) => JSON.stringify(v)).join(' | ');
    }

    const { type } = schema;

    if (type === 'integer' || type === 'number') {
        return 'number';
    }
    if (type === 'string') {
        return 'string';
    }
    if (type === 'boolean') {
        return 'boolean';
    }

    const looksLikeObject =
        type === 'object' ||
        (Boolean(schema.properties) && typeof schema.properties === 'object');
    if (looksLikeObject) {
        const props = schema.properties;
        if (!props || typeof props !== 'object') {
            return '{}';
        }

        const requiredSet = new Set<string>(schema.required);
        const entries = Object.entries(props);
        if (entries.length === 0) {
            return '{}';
        }

        const parts: string[] = [];
        for (const [propName, propSchema] of entries) {
            if (!propSchema) {
                continue;
            }
            const optional = requiredSet.has(propName) ? '' : '?';
            parts.push(
                `${renderPropertyKey(propName)}${optional}: ${schemaToTypeExpr(propSchema, rootSchema)}`,
            );
        }
        return `{ ${parts.join('; ')} }`;
    }

    if (type === 'array') {
        if (schema.prefixItems !== undefined) {
            const tuple = schema.prefixItems
                .map((s) => schemaToTypeExpr(s, rootSchema))
                .join(', ');
            return `[${tuple}]`;
        }
        if (schema.items) {
            return `${schemaToTypeExpr(schema.items, rootSchema)}[]`;
        }
        throw new Error('Unsupported array schema: missing items/prefixItems');
    }

    if (type === undefined) {
        // Schema with only 'const' (used in tagged unions)
        if (schema.const !== undefined) {
            return JSON.stringify(schema.const);
        }
        console.error(
            'Schema with missing type:',
            JSON.stringify(schema, null, 2),
        );
        throw new Error('Unsupported schema: missing type');
    }

    throw new Error(`Unsupported scalar type: ${type}`);
}
