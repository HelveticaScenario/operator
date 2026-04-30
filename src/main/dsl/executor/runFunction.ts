import type { ModuleSchema } from '@modular/core';
import { setActiveSpanRegistry, setDSLWrapperLineOffset } from '../factory';
import { setActiveInterpolationResolutions } from '../../../shared/dsl/spanTypes';
import { analyzeSourceSpans } from '../analyzeSource';
import type { CallSiteSpanRegistry } from '../analyzeSource';
import type { InterpolationResolutionMap } from '../../../shared/dsl/spanTypes';

/** Wrapper-line constants — kept exported so other tooling can stay in sync. */
export const WRAPPER_LINE_COUNT = 4;
export const FIRST_LINE_COLUMN_OFFSET = 4;

/**
 * Run user DSL code via `new Function()`, returning span analysis results.
 * Wraps source in a `'use strict'` body and threads error context.
 */
export function runFunction(
    source: string,
    schemas: ModuleSchema[],
    dslGlobals: Record<string, unknown>,
): {
    interpolationResolutions: InterpolationResolutionMap;
    callSiteSpans: CallSiteSpanRegistry;
} {
    setDSLWrapperLineOffset(WRAPPER_LINE_COUNT);

    const {
        registry: spanRegistry,
        interpolationResolutions,
        callSiteSpans,
    } = analyzeSourceSpans(
        source,
        schemas,
        WRAPPER_LINE_COUNT,
        FIRST_LINE_COLUMN_OFFSET,
    );
    setActiveSpanRegistry(spanRegistry);
    setActiveInterpolationResolutions(interpolationResolutions);

    const functionBody = `
    'use strict';
    ${source}
  `;

    const paramNames = Object.keys(dslGlobals);
    const paramValues = Object.values(dslGlobals);

    try {
        const fn = new Function(...paramNames, functionBody);
        fn(...paramValues);
        return { interpolationResolutions, callSiteSpans };
    } catch (error) {
        if (error instanceof Error) {
            throw new Error(`DSL execution error: ${error.message}`, {
                cause: error,
            });
        }
        throw error;
    } finally {
        // Clear the span registry after execution — spans are baked into
        // module state via ARGUMENT_SPANS_KEY so the registry isn't needed.
        setActiveSpanRegistry(null);
        // NOTE: Do NOT clear interpolation resolutions here. They are read
        // asynchronously by moduleStateTracking during decoration polling and
        // must persist until the next execution replaces them.
    }
}
