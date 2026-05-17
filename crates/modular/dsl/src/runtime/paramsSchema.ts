import type { ModuleSchema } from '@modular/core';

/**
 * A small, pragmatic subset of JSON Schema (as emitted by `schemars`).
 * We intentionally keep this minimal and treat unknown shapes as `unknown`.
 */
export interface JsonSchema {
    $ref?: string;
    $schema?: string;
    title?: string;
    description?: string;

    type?:
        | 'object'
        | 'string'
        | 'number'
        | 'integer'
        | 'boolean'
        | 'array'
        | 'null';

    properties?: Record<string, JsonSchema>;
    required?: string[];

    oneOf?: JsonSchema[];
    anyOf?: JsonSchema[];
    allOf?: JsonSchema[];

    enum?: unknown[];
    const?: unknown;

    items?: JsonSchema | JsonSchema[];

    definitions?: Record<string, JsonSchema>;

    // Allow extra schemars/json-schema keywords without modeling them.
    [k: string]: any;
}

export type ParamKind =
    | 'signal'
    | 'polySignal'
    | 'signalArray'
    | 'buffer'
    | 'number'
    | 'string'
    | 'boolean'
    | 'unknown';

export type SignalType = 'pitch' | 'gate' | 'trig' | 'control';

const SIGNAL_TYPES = new Set<string>(['pitch', 'gate', 'trig', 'control']);

function isSignalType(s: string): s is SignalType {
    return SIGNAL_TYPES.has(s);
}

export interface ParamDescriptor {
    name: string;
    kind: ParamKind;
    description?: string;
    optional: boolean;
    enumValues?: string[];
    signalType?: SignalType;
    defaultValue?: number;
    minValue?: number;
    maxValue?: number;
}

export type ProcessedModuleSchema = ModuleSchema & {
    params: ParamDescriptor[];
    paramsByName: Record<string, ParamDescriptor>;
};

function isPlainObject(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function asJsonSchema(value: unknown): JsonSchema | null {
    if (typeof value === 'boolean') {
        return { const: value };
    }
    if (!isPlainObject(value)) {
        return null;
    }
    return value as JsonSchema;
}

function getByJsonPointer(
    root: JsonSchema,
    pointer: string,
): JsonSchema | null {
    // Only support local refs.
    if (!pointer.startsWith('#/')) {
        return null;
    }
    const parts = pointer
        .slice(2)
        .split('/')
        .map((p) => p.replace(/~1/g, '/').replace(/~0/g, '~'));

    let cur: any = root;
    for (const part of parts) {
        if (!isPlainObject(cur)) {
            return null;
        }
        cur = cur[part];
        if (cur === undefined) {
            return null;
        }
    }
    return asJsonSchema(cur);
}

function deref(
    root: JsonSchema,
    schema: JsonSchema,
    seen = new Set<string>(),
): JsonSchema {
    const ref = schema.$ref;
    if (!ref) {
        return schema;
    }
    if (seen.has(ref)) {
        return schema;
    }
    const resolved = getByJsonPointer(root, ref);
    if (!resolved) {
        return schema;
    }
    seen.add(ref);
    return deref(root, resolved, seen);
}

function mergeObjectSchemas(a: JsonSchema, b: JsonSchema): JsonSchema {
    const out: JsonSchema = { ...a, ...b };
    if (a.properties || b.properties) {
        out.properties = { ...(a.properties ?? {}), ...(b.properties ?? {}) };
    }
    if (a.required || b.required) {
        out.required = Array.from(
            new Set([...(a.required ?? []), ...(b.required ?? [])]),
        );
    }
    if (!out.description) {
        out.description = a.description ?? b.description;
    }
    return out;
}

function resolveAndMerge(root: JsonSchema, schema: JsonSchema): JsonSchema {
    const resolved = deref(root, schema);

    if (
        resolved.allOf &&
        Array.isArray(resolved.allOf) &&
        resolved.allOf.length > 0
    ) {
        return resolved.allOf
            .map((s) => resolveAndMerge(root, s))
            .reduce((acc, cur) => mergeObjectSchemas(acc, cur), {
                ...resolved,
                allOf: undefined,
            });
    }

    return resolved;
}

function extractTypeTag(schema: JsonSchema): string | null {
    if (typeof schema.const === 'string') {
        return schema.const;
    }
    if (Array.isArray(schema.enum)) {
        const str = schema.enum.find((v) => typeof v === 'string');
        return typeof str === 'string' ? str : null;
    }
    return null;
}

/**
 * Detects if a schema is a Signal type.
 * Signal schema is: anyOf [number, string, SignalTaggedSchema]
 * where SignalTaggedSchema is oneOf [cable]
 */
function isSignalParamSchema(root: JsonSchema, schema: JsonSchema): boolean {
    const resolved = resolveAndMerge(root, schema);

    // Check for direct $ref to Signal
    if (schema.$ref === '#/$defs/Signal') {
        return true;
    }

    // Check for title indicating Signal
    if (resolved.title === 'Signal') {
        return true;
    }

    // Check for the actual Signal schema structure:
    // AnyOf: [number, string, SignalTaggedSchema ref]
    const union = resolved.oneOf ?? resolved.anyOf;
    if (!union || !Array.isArray(union)) {
        return false;
    }

    let hasNumber = false;
    let _hasString = false;
    let hasTaggedSchema = false;

    for (const branch of union) {
        const b = resolveAndMerge(root, branch);

        // Check for number type
        if (b.type === 'number' || b.type === 'integer') {
            hasNumber = true;
            continue;
        }

        // Check for string type
        if (b.type === 'string') {
            _hasString = true;
            continue;
        }

        // Check for SignalTaggedSchema (oneOf with cable variant)
        const taggedUnion = b.oneOf ?? b.anyOf;
        if (taggedUnion && Array.isArray(taggedUnion)) {
            const tags = new Set<string>();
            for (const taggedBranch of taggedUnion) {
                const tb = resolveAndMerge(root, taggedBranch);
                if (tb.type === 'object' && tb.properties?.type) {
                    const tag = extractTypeTag(
                        resolveAndMerge(root, tb.properties.type),
                    );
                    if (tag) {
                        tags.add(tag);
                    }
                }
            }
            // SignalTaggedSchema has a "cable" variant
            if (tags.has('cable')) {
                hasTaggedSchema = true;
            }
        }
    }

    // Signal schema should have at least number and the tagged schema
    // (string is optional for the parse_signal_string format)
    return hasNumber && hasTaggedSchema;
}

function isSignalArrayParamSchema(
    root: JsonSchema,
    schema: JsonSchema,
): boolean {
    const resolved = resolveAndMerge(root, schema);
    if (resolved.type !== 'array') {
        return false;
    }

    const { items } = resolved;
    if (!items) {
        return false;
    }

    if (Array.isArray(items)) {
        // Tuple validation; treat as signal array if all entries are signals.
        return items.every((it) => {
            const s = resolveAndMerge(root, it);
            return isSignalParamSchema(root, s);
        });
    }

    const itemSchema = resolveAndMerge(root, items);
    return isSignalParamSchema(root, itemSchema);
}

/**
 * Detects PolySignal schema: either a single Signal OR an array of Signals.
 * PolySignal in Rust serializes as anyOf with Signal and Signal[] variants.
 */
function isPolySignalParamSchema(
    root: JsonSchema,
    schema: JsonSchema,
): boolean {
    // Check for direct $ref to PolySignal
    if (schema.$ref === '#/$defs/PolySignal') {
        return true;
    }

    const resolved = resolveAndMerge(root, schema);

    // Check for title indicating PolySignal
    if (resolved.title === 'PolySignal') {
        return true;
    }

    // Check for anyOf pattern: [Signal, Signal[]]
    const union = resolved.oneOf ?? resolved.anyOf;
    if (!union || !Array.isArray(union) || union.length < 2) {
        return false;
    }

    let hasSignal = false;
    let hasSignalArray = false;

    for (const branch of union) {
        const b = resolveAndMerge(root, branch);
        if (isSignalParamSchema(root, b)) {
            hasSignal = true;
        } else if (isSignalArrayParamSchema(root, b)) {
            hasSignalArray = true;
        }
    }

    return hasSignal && hasSignalArray;
}

function isBufferParamSchema(root: JsonSchema, schema: JsonSchema): boolean {
    if (schema.$ref === '#/$defs/Buffer') {
        return true;
    }

    const resolved = resolveAndMerge(root, schema);
    if (resolved.title === 'Buffer') {
        return true;
    }

    const union = resolved.oneOf ?? resolved.anyOf;
    if (union && Array.isArray(union) && union.length === 1) {
        return isBufferParamSchema(root, union[0]);
    }

    if (resolved.type !== 'object') {
        return false;
    }

    // Heuristic: check for the buffer_ref shape { type: "buffer_ref", module, port, channels }
    const typeSchema = resolved.properties?.type
        ? resolveAndMerge(root, resolved.properties.type)
        : null;
    const moduleSchema = resolved.properties?.module
        ? resolveAndMerge(root, resolved.properties.module)
        : null;
    const portSchema = resolved.properties?.port
        ? resolveAndMerge(root, resolved.properties.port)
        : null;
    const channelsSchema = resolved.properties?.channels
        ? resolveAndMerge(root, resolved.properties.channels)
        : null;

    return (
        extractTypeTag(typeSchema ?? {}) === 'buffer_ref' &&
        moduleSchema?.type === 'string' &&
        portSchema?.type === 'string' &&
        (channelsSchema?.type === 'integer' ||
            channelsSchema?.type === 'number')
    );
}

function extractStringEnum(schema: JsonSchema): string[] | undefined {
    const resolved = schema;

    // Standard enum format
    if (
        resolved.type === 'string' &&
        Array.isArray(resolved.enum) &&
        resolved.enum.every((v) => typeof v === 'string')
    ) {
        return resolved.enum;
    }
    if (
        Array.isArray(resolved.enum) &&
        resolved.enum.every((v) => typeof v === 'string')
    ) {
        return resolved.enum;
    }

    // OneOf with const values format (serde rename_all enums)
    const union = resolved.oneOf ?? resolved.anyOf;
    if (union && Array.isArray(union)) {
        const values: string[] = [];
        for (const branch of union) {
            if (branch.type === 'string' && typeof branch.const === 'string') {
                values.push(branch.const);
            }
        }
        if (values.length === union.length && values.length > 0) {
            return values;
        }
    }

    return undefined;
}

/**
 * Checks if a schema represents a string enum (oneOf with const string values)
 */
function isStringEnumSchema(schema: JsonSchema): boolean {
    const union = schema.oneOf ?? schema.anyOf;
    if (!union || !Array.isArray(union) || union.length === 0) {
        return false;
    }

    return union.every(
        (branch) =>
            branch.type === 'string' && typeof branch.const === 'string',
    );
}

function inferKind(root: JsonSchema, schema: JsonSchema): ParamKind {
    // Check polySignal first since it's a union of Signal | Signal[]
    if (isPolySignalParamSchema(root, schema)) {
        return 'polySignal';
    }
    if (isBufferParamSchema(root, schema)) {
        return 'buffer';
    }
    if (isSignalParamSchema(root, schema)) {
        return 'signal';
    }
    if (isSignalArrayParamSchema(root, schema)) {
        return 'signalArray';
    }

    const resolved = resolveAndMerge(root, schema);

    switch (resolved.type) {
        case 'number':
        case 'integer':
            return 'number';
        case 'string':
            return 'string';
        case 'boolean':
            return 'boolean';
        default:
            break;
    }

    // If schema forgot `type` but has string enum, treat as string.
    if (
        Array.isArray(resolved.enum) &&
        resolved.enum.every((v) => typeof v === 'string')
    ) {
        return 'string';
    }

    // Check for oneOf with const string values (serde rename_all enums)
    if (isStringEnumSchema(resolved)) {
        return 'string';
    }

    return 'unknown';
}

export function processModuleSchema(
    schema: ModuleSchema,
): ProcessedModuleSchema {
    const root = asJsonSchema(schema.paramsSchema) ?? {};
    const rootResolved = resolveAndMerge(root, root);

    const obj = rootResolved.type === 'object' ? rootResolved : null;
    const properties = obj?.properties ?? {};
    const required = new Set(obj?.required ?? []);

    // Build signal param lookup from schema.signalParams
    const signalParamsByName = new Map<
        string,
        ModuleSchema['signalParams'][number]
    >();
    if (schema.signalParams) {
        for (const sp of schema.signalParams) {
            signalParamsByName.set(sp.name, sp);
        }
    }

    const params: ParamDescriptor[] = Object.entries(properties).map(
        ([name, s]) => {
            const resolved = resolveAndMerge(root, s);
            const enumValues = extractStringEnum(resolved);
            const inferedKind = inferKind(root, s);

            const signalMeta = signalParamsByName.get(name);

            return {
                description: resolved.description,
                enumValues,
                kind: inferedKind,
                name,
                optional: !required.has(name),
                ...(signalMeta && {
                    defaultValue: signalMeta.defaultValue,
                    maxValue: signalMeta.maxValue,
                    minValue: signalMeta.minValue,
                    signalType: isSignalType(signalMeta.signalType)
                        ? signalMeta.signalType
                        : 'control',
                }),
            };
        },
    );

    const paramsByName: Record<string, ParamDescriptor> = {};
    for (const p of params) {
        paramsByName[p.name] = p;
    }

    return {
        ...schema,
        params,
        paramsByName,
    };
}

export function processSchemas(
    schemas: ModuleSchema[],
): ProcessedModuleSchema[] {
    return schemas.map(processModuleSchema);
}
