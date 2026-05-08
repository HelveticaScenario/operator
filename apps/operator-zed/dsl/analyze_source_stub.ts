// Stub for `src/main/dsl/analyzeSource.ts`.
//
// The real implementation creates a ts-morph Project to walk the user's
// source TypeScript and extract argument spans + interpolation resolutions.
// ts-morph drags in the entire TypeScript compiler (~14 MB) plus node:fs /
// node:path, none of which run in deno_core.
//
// HANDOFF.md item 2 (op_argument_spans / oxc_parser) replaces this with a
// Rust-side implementation. Until that lands, the stub returns empty
// registries — argument-span highlighting and interpolation redirects will
// be inactive in the Zed shell, but the DSL itself executes and the
// resulting PatchGraph is correct.

import type {
    AnalysisResult,
    SpanRegistry,
    CallSiteSpanRegistry,
} from '../../../src/main/dsl/sourceAnalysisTypes';
import type { InterpolationResolutionMap } from '../../../src/shared/dsl/spanTypes';

// Re-export the shared types so consumers that import them from
// `./analyzeSource` keep working under the alias.
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
} from '../../../src/main/dsl/sourceAnalysisTypes';
export {
    setActiveInterpolationResolutions,
    getActiveInterpolationResolutions,
} from '../../../src/shared/dsl/spanTypes';

export function analyzeSourceSpans(
    _source: string,
    _schemas: unknown[],
    _lineOffset: number = 0,
    _firstLineColumnOffset: number = 0,
): AnalysisResult {
    const registry: SpanRegistry = new Map();
    const callSiteSpans: CallSiteSpanRegistry = new Map();
    const interpolationResolutions: InterpolationResolutionMap = new Map();
    return { callSiteSpans, interpolationResolutions, registry };
}
