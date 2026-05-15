export {
    ARGUMENT_SPANS_KEY,
    setActiveSpanRegistry,
    captureArgumentSpans,
} from './spanRegistry';
export type { ArgumentSpans } from './spanRegistry';
export {
    sanitizeIdentifier,
    toCamelCase,
    sanitizeOutputName,
    RESERVED_OUTPUT_NAMES,
} from './identifiers';
export { buildNamespaceTree } from './namespaceTree';
export type {
    FactoryFunction,
    NamespaceTree,
    ModuleReturn,
    MultiOutput,
} from './namespaceTree';
export { createFactory } from './createFactory';
export { DSLContext } from './dslContext';
export { hz, note, bpm } from './units';

// Re-export source-location helpers for backward compat with `from './factories'`.
export {
    captureSourceLocation,
    setDSLWrapperLineOffset,
    getDSLWrapperLineOffset,
} from '../captureSourceLocation';
