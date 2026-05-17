/**
 * Span analysis types shared between the DSL runtime and source analyzers.
 *
 * Source analyzers live outside this package (they depend on ts-morph and stay
 * in `src/main/dsl/` so the package remains renderer-safe). Runtime accepts
 * analysis results via the `analyzer` callback in `DSLExecutionOptions`.
 */

import type {
    InterpolationResolutionMap,
    SourceSpan,
} from '../../../../../../src/shared/dsl/spanTypes';

export type {
    SourceSpan,
    ResolvedInterpolation,
    InterpolationResolutionMap,
} from '../../../../../../src/shared/dsl/spanTypes';
export {
    setActiveInterpolationResolutions,
    getActiveInterpolationResolutions,
} from '../../../../../../src/shared/dsl/spanTypes';

/** Registry entry for a single call expression's argument spans */
export interface CallSiteSpans {
    /** Spans for each positional argument, keyed by argument name */
    args: Map<string, SourceSpan>;
    /** The module type being called (e.g., "seq", "sine") */
    moduleType: string;
}

/**
 * Call site key using line:column format (both 1-based).
 * Matches the format produced by both ts-morph and V8 Error.captureStackTrace.
 */
export type CallSiteKey = `${number}:${number}`;

/** Registry mapping call sites to their argument spans */
export type SpanRegistry = Map<CallSiteKey, CallSiteSpans>;

/**
 * Full source span of a call expression, in editor line numbers.
 * Used to position view zones after the closing paren of multi-line calls.
 */
export interface CallExpressionSpan {
    /** 1-based start line of the call expression (in user source, before wrapper offset) */
    startLine: number;
    /** 1-based end line of the call expression (line containing the closing paren) */
    endLine: number;
}

/**
 * Registry mapping call site keys to their full expression spans.
 * Covers DSL methods like .scope() and $slider() whose view zones need to know
 * the end line of the entire call expression.
 */
export type CallSiteSpanRegistry = Map<CallSiteKey, CallExpressionSpan>;

/**
 * Result of source analysis. Produced by the optional `analyzer` callback in
 * `DSLExecutionOptions` and consumed by the runtime to set the active span
 * registry before user code runs.
 */
export interface AnalysisResult {
    /** Registry mapping call sites to argument spans (Rust-side highlighting) */
    registry: SpanRegistry;
    /** Map from argument span key to resolved interpolations (TS-side const redirect) */
    interpolationResolutions: InterpolationResolutionMap;
    /** Registry mapping call site keys to full call expression spans */
    callSiteSpans: CallSiteSpanRegistry;
}

/** Signature of the analyzer callback passed in `DSLExecutionOptions.analyzer`. */
export type SpanAnalyzer = (
    source: string,
    schemas: import('@modular/core').ModuleSchema[],
    wrapperLineCount: number,
    firstLineColumnOffset: number,
) => AnalysisResult;

/** Empty AnalysisResult for tests / callers that don't need source spans. */
export const EMPTY_ANALYSIS_RESULT: AnalysisResult = Object.freeze({
    registry: new Map(),
    interpolationResolutions: new Map(),
    callSiteSpans: new Map(),
}) as AnalysisResult;
