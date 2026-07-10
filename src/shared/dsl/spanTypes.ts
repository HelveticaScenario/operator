/**
 * Shared types and state for source span analysis.
 *
 * This file is deliberately free of ts-morph (or any Node.js-only) imports
 * so it can be consumed from the renderer process without pulling in
 * Node.js built-ins via webpack.
 */

/**
 * The DSL executor's function-body template indents the first line of user
 * source by this many columns, so V8 stack columns (and therefore
 * `captureSourceLocation` results and call-site span keys) are shifted by
 * this amount on line 1 only. Consumers mapping a captured column back to an
 * editor column must subtract it for line-1 locations.
 */
export const FIRST_LINE_COLUMN_OFFSET = 4;

/**
 * Span representing a character range in source code
 */
export interface SourceSpan {
    /** Absolute start offset (0-based) */
    start: number;
    /** Absolute end offset (exclusive) */
    end: number;
}

/**
 * Describes a resolved interpolation within a template literal.
 * When `${someConst}` appears in a template and `someConst` is a const with a
 * literal initializer, this records the mapping from the interpolation's
 * evaluated position range to the const literal's document span.
 *
 * For recursive resolution (const with template literal initializer that itself
 * has const interpolations), `nestedResolutions` contains the inner resolutions.
 */
export interface ResolvedInterpolation {
    /** Evaluated character offset where this interpolation's result starts */
    evaluatedStart: number;
    /** Length of the interpolation's evaluated result */
    evaluatedLength: number;
    /** Document span of the const literal (including quotes) */
    constLiteralSpan: SourceSpan;
    /** Nested resolutions within the const literal (for recursive template consts) */
    nestedResolutions?: ResolvedInterpolation[];
}

/**
 * Map from argument span key (`${start}:${end}`) to its resolved interpolations.
 * The key is the SourceSpan of the argument as recorded in the span registry.
 */
export type InterpolationResolutionMap = Map<string, ResolvedInterpolation[]>;

/**
 * Active interpolation resolution map, set during DSL analysis.
 * Read by moduleStateTracking to redirect highlights into const declarations.
 */
let _activeInterpolationResolutions: InterpolationResolutionMap | null = null;

/**
 * Set the active interpolation resolution map.
 * Called by executor.ts after analysis and before/after execution.
 */
export function setActiveInterpolationResolutions(
    map: InterpolationResolutionMap | null,
): void {
    _activeInterpolationResolutions = map;
}

/**
 * Get the active interpolation resolution map.
 * Read by moduleStateTracking.ts during decoration polling.
 */
export function getActiveInterpolationResolutions(): InterpolationResolutionMap | null {
    return _activeInterpolationResolutions;
}
