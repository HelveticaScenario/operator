/**
 * Backward-compat re-exports.
 * Span analysis types live in `@modular/dsl/runtime/types/analysis.ts`; this
 * file simply forwards them so existing callers in `src/main/dsl/` (the
 * ts-morph-based analyzers) and renderer code keep working.
 */

export type {
    SourceSpan,
    ResolvedInterpolation,
    InterpolationResolutionMap,
    CallSiteSpans,
    CallSiteKey,
    SpanRegistry,
    CallExpressionSpan,
    CallSiteSpanRegistry,
    AnalysisResult,
} from '@modular/dsl';
export {
    setActiveInterpolationResolutions,
    getActiveInterpolationResolutions,
} from '@modular/dsl';
