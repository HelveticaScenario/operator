import { getReservedOutputNames } from '@modular/core';

export function sanitizeIdentifier(name: string): string {
    let id = name.replace(
        /[^a-zA-Z0-9_$]+(.)?/g,
        (_match, chr: string | undefined) => (chr ? chr.toUpperCase() : ''),
    );
    if (!/^[A-Za-z_$]/.test(id)) {
        id = `_${id}`;
    }
    return id || '_';
}

/** Convert snake_case to camelCase */
export function toCamelCase(str: string): string {
    return str.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
}

/**
 * Reserved property names that conflict with ModuleOutput, Collection, or
 * CollectionWithRange methods/properties. Output names matching these will be
 * suffixed with an underscore.
 *
 * Single source of truth: `crates/reserved_output_names.rs`.
 */
export const RESERVED_OUTPUT_NAMES: ReadonlySet<string> = new Set(
    getReservedOutputNames(),
);

/**
 * Sanitize output name to avoid conflicts with reserved properties/methods.
 * Appends underscore if the camelCase name is reserved.
 */
export function sanitizeOutputName(name: string): string {
    const camelName = toCamelCase(name);
    return RESERVED_OUTPUT_NAMES.has(camelName) ? `${camelName}_` : camelName;
}
