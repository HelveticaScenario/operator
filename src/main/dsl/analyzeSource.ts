/**
 * Source Analysis Orchestration
 *
 * Thin entry point that creates a ts-morph Project/SourceFile once and
 * delegates to the argument span analyzer and call site span analyzer.
 * Returns a combined AnalysisResult.
 */

import { Project, ts } from 'ts-morph';
import type { ModuleSchema } from '@modular/core';

import type { AnalysisResult, SpanRegistry } from './sourceAnalysisTypes';
import { analyzeArgumentSpans } from './argumentSpanAnalyzer';
import { analyzeCallSiteSpans } from './callSiteSpanAnalyzer';

// Re-export types and utilities so existing consumers can import from here
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
} from './sourceAnalysisTypes';
export {
    setActiveInterpolationResolutions,
    getActiveInterpolationResolutions,
} from './sourceAnalysisTypes';

/**
 * Analyze DSL source code and build registries for argument locations
 * and call expression spans.
 *
 * @param source - The DSL source code to analyze
 * @param schemas - Module schemas to determine which calls to track
 * @param lineOffset - Line offset to add (for the executor's script wrapper lines)
 * @param firstLineColumnOffset - Column offset for the first line
 * @returns Analysis result with span registry, interpolation resolution map,
 *          and call site span registry
 */
export function analyzeSourceSpans(
    source: string,
    schemas: ModuleSchema[],
    lineOffset: number = 0,
    firstLineColumnOffset: number = 0,
): AnalysisResult {
    // Create an in-memory TypeScript project
    const project = new Project({
        compilerOptions: {
            allowJs: true,
            checkJs: false,
            noEmit: true,
            target: ts.ScriptTarget.ESNext,
        },
        useInMemoryFileSystem: true,
    });

    // Add source as a virtual file
    const sourceFile = project.createSourceFile('dsl.ts', source);

    // Pass 1: Argument spans for factory calls
    const { registry, interpolationResolutions } = analyzeArgumentSpans(
        sourceFile,
        schemas,
        lineOffset,
        firstLineColumnOffset,
    );

    // Pass 2: Full call expression spans for DSL methods
    const callSiteSpans = analyzeCallSiteSpans(
        sourceFile,
        firstLineColumnOffset,
    );

    return { callSiteSpans, interpolationResolutions, registry };
}
