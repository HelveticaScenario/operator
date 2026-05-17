import type {
    CallSiteKey,
    SourceSpan,
    SpanRegistry,
} from '../types/analysis';
import { getDSLWrapperLineOffset } from '../captureSourceLocation';

/**
 * Key used for internal metadata field storing argument source spans.
 * Must match modular_core::types::ARGUMENT_SPANS_KEY in Rust.
 */
export const ARGUMENT_SPANS_KEY = '__argument_spans';

/**
 * Active span registry for the current DSL execution.
 * Set by executor.ts before running user code, cleared after.
 */
let activeSpanRegistry: SpanRegistry | null = null;

/**
 * Set the active span registry for argument span capture.
 * Called by executor.ts before and after DSL execution.
 */
export function setActiveSpanRegistry(registry: SpanRegistry | null): void {
    activeSpanRegistry = registry;
}

/** Type for argument spans attached to module params */
export type ArgumentSpans = Record<string, SourceSpan>;

/**
 * Look up argument spans from the active span registry using the source location.
 * Returns undefined if no registry is set or no spans found for this call site.
 */
export function captureArgumentSpans(
    sourceLocation: { line: number; column: number } | undefined,
): ArgumentSpans | undefined {
    if (!activeSpanRegistry || !sourceLocation) {
        return undefined;
    }

    // Build the call site key matching what ts-morph produced.
    // Both ts-morph and V8 stack traces use 1-based lines and columns.
    const key: CallSiteKey = `${sourceLocation.line + getDSLWrapperLineOffset()}:${sourceLocation.column}`;

    const entry = activeSpanRegistry.get(key);
    if (!entry) {
        return undefined;
    }

    // Convert Map to plain object for serialization to Rust
    const spans: ArgumentSpans = {};
    for (const [argName, span] of entry.args) {
        spans[argName] = span;
    }

    return spans;
}
