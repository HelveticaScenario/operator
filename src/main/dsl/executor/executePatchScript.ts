import type { ModuleSchema } from '@modular/core';
import { installArrayPipe } from './arrayPipe';
import { buildContext } from './buildContext';
import { runFunction } from './runFunction';
import type { DSLExecutionOptions, DSLExecutionResult } from './types';

installArrayPipe();

/**
 * Execute a DSL script and return the resulting PatchGraph with source locations.
 */
export function executePatchScript(
    source: string,
    schemas: ModuleSchema[],
    options: DSLExecutionOptions = {},
): DSLExecutionResult {
    const { context, dslGlobals, sliders } = buildContext(schemas, options);

    const { interpolationResolutions, callSiteSpans } = runFunction(
        source,
        schemas,
        dslGlobals,
    );

    const resultBuilder = context.getBuilder();
    const patch = resultBuilder.toPatch();
    const sourceLocationMap = resultBuilder.getSourceLocationMap();

    return {
        callSiteSpans,
        interpolationResolutions,
        patch,
        sliders,
        sourceLocationMap,
    };
}
