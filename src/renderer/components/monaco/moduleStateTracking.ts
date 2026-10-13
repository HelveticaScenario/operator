/**
 * Generic Module State Tracking
 *
 * A unified system for tracking module state and creating Monaco decorations
 * based on argument spans and internal source spans. Works for any module
 * with `#[args]` and optional `param_spans` in its state.
 *
 * Key concepts:
 * - `argument_spans`: Document offsets for each positional argument (from ts-morph analysis)
 * - `param_spans`: Map of param name -> { spans, source } for internal highlighting
 * - Combining them: document_offset = argument_spans[paramName].start + param_spans[paramName].spans[i]
 *
 * For template literals with interpolations, the system maps evaluated positions
 * back to source positions so highlighting works correctly.
 *
 * IMPORTANT: This system uses Monaco's tracked decorations with stickiness so that
 * decorations automatically move when the user types. Tracked decorations are
 * created for every span on the first poll after a patch evaluates — while
 * `argument_spans` (evaluation-time document offsets) still match the document,
 * before any edit can shift it. The decorations are owned by the model (not an
 * editor) and the cache is keyed by model, so anchors survive polling restarts,
 * tab switches, and editor recreation. During polling,
 * model.getDecorationRange() supplies the current (tracked) positions.
 * This applies to both interpolated and non-interpolated spans.
 */

import type React from 'react';
import type { editor } from 'monaco-editor';
import type { Monaco } from '../../hooks/useCustomMonaco';
import type {
    ResolvedInterpolation,
    SourceSpan,
} from '../../../shared/dsl/spanTypes';
import { getActiveInterpolationResolutions } from '../../../shared/dsl/spanTypes';

/**
 * Argument spans as they come from module state (document offsets)
 */
export type ArgumentSpans = Record<string, SourceSpan>;

/**
 * Internal span info for a single parameter
 */
export interface ParamSpanInfo {
    /** Currently active spans within this argument (offsets relative to argument content) */
    spans: [number, number][];
    /** The evaluated source string (for interpolation mapping) */
    source: string;
    /** All leaf spans in the pattern (for creating tracked decorations at patch time).
     * This is computed once when the pattern is parsed and doesn't change during playback.
     */
    all_spans?: [number, number][];
}

/**
 * Map of parameter name to its span info
 */
export type ParamSpans = Record<string, ParamSpanInfo>;

/**
 * Generic module state structure
 */
export interface ModuleStateWithSpans {
    /** Spans for positional arguments (document offsets) */
    argument_spans?: ArgumentSpans;
    /** Map of param name -> { spans, source, all_spans } for internal highlighting */
    param_spans?: ParamSpans;
    /** Any other state fields */
    [key: string]: unknown;
}

/**
 * Interpolation region for template literals
 */
interface InterpolationRegion {
    sourceStart: number;
    sourceEnd: number;
    sourceLen: number;
    evaluatedStart: number;
    evaluatedLen: number;
}

/**
 * Extract interpolation regions from a template literal.
 * Maps ${...} regions in source to their evaluated result positions.
 */
function extractInterpolationRegions(
    sourceContent: string,
    evaluatedContent: string,
): InterpolationRegion[] | null {
    const interpolationRegex = /\$\{/g;
    const regions: InterpolationRegion[] = [];
    let match;

    while ((match = interpolationRegex.exec(sourceContent)) !== null) {
        const startIdx = match.index;
        let depth = 1;
        let endIdx = startIdx + 2;

        while (endIdx < sourceContent.length && depth > 0) {
            if (sourceContent[endIdx] === '{') {
                depth++;
            } else if (sourceContent[endIdx] === '}') {
                depth--;
            }
            endIdx++;
        }

        if (depth === 0) {
            regions.push({
                evaluatedLen: 0,
                evaluatedStart: 0,
                sourceEnd: endIdx,
                sourceLen: endIdx - startIdx,
                sourceStart: startIdx,
            });
        }
    }

    if (regions.length === 0) {
        return null;
    }

    // Build literal pieces for mapping
    const literalPieces: {
        text: string;
        sourceStart: number;
        sourceEnd: number;
    }[] = [];
    let pos = 0;

    for (const region of regions) {
        if (pos < region.sourceStart) {
            literalPieces.push({
                sourceEnd: region.sourceStart,
                sourceStart: pos,
                text: sourceContent.slice(pos, region.sourceStart),
            });
        }
        pos = region.sourceEnd;
    }

    if (pos < sourceContent.length) {
        literalPieces.push({
            sourceEnd: sourceContent.length,
            sourceStart: pos,
            text: sourceContent.slice(pos),
        });
    }

    // Match literal pieces in evaluated string
    let evalPos = 0;
    let regionIdx = 0;

    for (let i = 0; i < literalPieces.length; i++) {
        const piece = literalPieces[i];
        const pieceIdx = evaluatedContent.indexOf(piece.text, evalPos);

        if (pieceIdx === -1) {
            return null;
        }

        const interpolationBeforeThisPiece =
            regionIdx < regions.length &&
            (i === 0 ? regions[0].sourceStart < piece.sourceStart : true);

        if (interpolationBeforeThisPiece) {
            regions[regionIdx].evaluatedStart = evalPos;
            regions[regionIdx].evaluatedLen = pieceIdx - evalPos;
            regionIdx++;
        }

        evalPos = pieceIdx + piece.text.length;
    }

    if (regionIdx < regions.length) {
        regions[regionIdx].evaluatedStart = evalPos;
        regions[regionIdx].evaluatedLen = evaluatedContent.length - evalPos;
    }

    return regions;
}

/**
 * Build interpolation regions using accurate data from the interpolation
 * resolution map (computed by sourceSpanAnalyzer via ts-morph).
 *
 * This avoids the fragile indexOf-based text matching in extractInterpolationRegions,
 * which can fail when the interpolated content contains substrings matching the
 * template's literal text (e.g., `${interpolated} 2 3` where interpolated = '0 2 3 ...').
 */
function buildInterpolationRegionsFromResolutions(
    sourceContent: string,
    resolutions: ResolvedInterpolation[],
): InterpolationRegion[] | null {
    // Find ${...} in source to get source-side positions
    const interpolationRegex = /\$\{/g;
    const sourceRegions: { sourceStart: number; sourceEnd: number }[] = [];
    let match;

    while ((match = interpolationRegex.exec(sourceContent)) !== null) {
        const startIdx = match.index;
        let depth = 1;
        let endIdx = startIdx + 2;

        while (endIdx < sourceContent.length && depth > 0) {
            if (sourceContent[endIdx] === '{') {
                depth++;
            } else if (sourceContent[endIdx] === '}') {
                depth--;
            }
            endIdx++;
        }

        if (depth === 0) {
            sourceRegions.push({ sourceEnd: endIdx, sourceStart: startIdx });
        }
    }

    if (
        sourceRegions.length === 0 ||
        sourceRegions.length !== resolutions.length
    ) {
        return null;
    }

    return sourceRegions.map((sr, i) => ({
        evaluatedLen: resolutions[i].evaluatedLength,
        evaluatedStart: resolutions[i].evaluatedStart,
        sourceEnd: sr.sourceEnd,
        sourceLen: sr.sourceEnd - sr.sourceStart,
        sourceStart: sr.sourceStart,
    }));
}

/**
 * Build a position mapper from evaluated to source positions
 */
function buildPositionMapper(
    regions: InterpolationRegion[],
): (evalPos: number) => number | null {
    return (evalPos: number): number | null => {
        let sourceOffset = 0;
        let evalOffset = 0;

        for (const region of regions) {
            const evalRegionStart = region.evaluatedStart;

            if (evalPos < evalRegionStart) {
                return evalPos + (sourceOffset - evalOffset);
            }

            if (evalPos < evalRegionStart + region.evaluatedLen) {
                return null; // Inside interpolation result
            }

            sourceOffset = region.sourceEnd;
            evalOffset = evalRegionStart + region.evaluatedLen;
        }

        return evalPos + (sourceOffset - evalOffset);
    };
}

/** Strip a surrounding quote/backtick pair from a literal's source text. */
function stripQuotes(text: string): string {
    return text.startsWith('`') || text.startsWith('"') || text.startsWith("'")
        ? text.slice(1, -1)
        : text;
}

/**
 * Resolve an evaluated position that falls inside an interpolation result
 * to a document offset by looking up the interpolation resolution map.
 *
 * When a template literal contains `${someConst}` and the position mapper
 * returns null (position is inside the interpolation result), this function
 * redirects the highlight to the original const literal's location in the document.
 *
 * Handles recursive resolution: if the const is itself a template with
 * interpolations, recurses into nested resolutions and re-maps positions in
 * the nested template's own literal text from evaluated to raw offsets.
 *
 * Span ends are exclusive, so a position on the boundary of two adjacent
 * interpolation results belongs to the earlier one when it is a span end and
 * to the later one when it is a span start; `bias` selects the boundary side.
 *
 * @param evalPos - Position in evaluated string that fell inside an interpolation
 * @param resolutions - Resolved interpolations for this argument span
 * @param getTextInSpan - Reads the document text covered by a span
 * @param bias - Whether evalPos is a span start or a span end
 * @returns Document offset to highlight, or null if no resolution found
 */
function resolveInterpolatedPosition(
    evalPos: number,
    resolutions: ResolvedInterpolation[],
    getTextInSpan: (span: SourceSpan) => string,
    bias: 'start' | 'end',
): number | null {
    for (const r of resolutions) {
        const rEnd = r.evaluatedStart + r.evaluatedLength;
        const inRegion =
            bias === 'start'
                ? evalPos >= r.evaluatedStart && evalPos < rEnd
                : evalPos > r.evaluatedStart && evalPos <= rEnd;
        if (!inRegion) {
            continue;
        }
        const offsetInResult = evalPos - r.evaluatedStart;

        // If the const has nested resolutions (it's a template with interpolations),
        // Check if this offset falls inside one of the nested interpolations
        if (r.nestedResolutions && r.nestedResolutions.length > 0) {
            const nestedResult = resolveInterpolatedPosition(
                offsetInResult,
                r.nestedResolutions,
                getTextInSpan,
                bias,
            );
            if (nestedResult !== null) {
                return nestedResult;
            }

            // Position in the nested template's own literal text: each nested
            // ${...} occupies a different width in the raw literal than in its
            // evaluated result, so the evaluated offset must be re-mapped
            // through the nested template's literal regions.
            const rawContent = stripQuotes(getTextInSpan(r.constLiteralSpan));
            const nestedRegions = buildInterpolationRegionsFromResolutions(
                rawContent,
                r.nestedResolutions,
            );
            if (nestedRegions) {
                const rawOffset =
                    buildPositionMapper(nestedRegions)(offsetInResult);
                if (rawOffset !== null) {
                    return r.constLiteralSpan.start + 1 + rawOffset;
                }
            }
        }

        // Plain string const: evaluated offsets equal literal offsets
        // +1 to skip the opening quote character
        return r.constLiteralSpan.start + 1 + offsetInResult;
    }
    return null;
}

/**
 * Cache entry for a single parameter's state.
 * Stores tracked decoration IDs for spans within this parameter.
 */
interface ParamCache {
    /** The argument span in document (used to detect when patch changes) */
    argumentSpan?: SourceSpan;
    /** Source content from document (for detecting changes) */
    sourceContent?: string;
    /** Whether source has interpolations */
    hasInterpolations: boolean;
    /** Position mapper for interpolation handling */
    positionMapper?: (evalPos: number) => number | null;
    /** The evaluated content this mapper was built for */
    evaluatedContentForMapper?: string;
    /**
     * Map of span ID (e.g., "0:5") to Monaco decoration ID.
     * These decorations are tracked and automatically move with text edits.
     * They are owned by the model (not an editor) so they survive editor
     * disposal and model detach/re-attach across tab switches.
     * Used for both interpolated and non-interpolated spans.
     */
    trackedDecorationIds?: Map<string, string>;
    /**
     * Whether we've already created tracked decorations for all_spans.
     * This prevents re-creating them on every poll.
     */
    trackedDecorationsCreated?: boolean;
    /**
     * The evaluated pattern `source` the tracked decorations were built from.
     * Any edit to the pattern changes this — including a same-width value edit
     * (e.g. `1` -> `7`) that leaves the leaf offsets (`all_spans`) and the
     * argument literal's document bounds (`argument_spans`) unchanged. Such an
     * edit rewrites the step's text, which can collapse that step's tracked
     * Monaco decoration (NeverGrowsWhenTypingAtEdges) so `getDecorationRange`
     * returns an empty range and the step stops highlighting. Comparing this
     * detects the edit and rebuilds the decorations from `argSpan` + the fresh
     * `all_spans` leaf offsets, so the edited step highlights again without a
     * restart.
     */
    lastSource?: string;
}

/**
 * Cache for a module (maps param name -> ParamCache)
 */
type ModuleCache = Map<string, ParamCache>;

/**
 * Cache for all modules
 */
type GlobalCache = Map<string, ModuleCache>;

/**
 * Per-model caches. Keyed by model so tracked anchor state survives polling
 * restarts and editor recreation; anchoring only happens while the cached
 * span data still matches the document, never re-derived from stale
 * evaluation-time offsets after edits.
 */
const modelCaches = new WeakMap<editor.ITextModel, GlobalCache>();

/** Remove a param's tracked anchor decorations from the model. */
function clearTrackedDecorations(
    model: editor.ITextModel,
    paramCache: ParamCache,
): void {
    if (paramCache.trackedDecorationIds) {
        model.deltaDecorations(
            [...paramCache.trackedDecorationIds.values()],
            [],
        );
    }
    paramCache.trackedDecorationIds = undefined;
    paramCache.trackedDecorationsCreated = false;
}

/**
 * Parameters for starting module state polling
 */
export interface ModuleStatePollingParams {
    editor: editor.IStandaloneCodeEditor;
    monaco: Monaco;
    currentFile?: string;
    runningBufferId?: string | null;
    activeDecorationRef: React.MutableRefObject<editor.IEditorDecorationsCollection | null>;
    getModuleStates: () => Promise<Record<string, unknown>>;
    /** CSS class for active spans (default: 'active-seq-step') */
    activeClassName?: string;
    /** Polling interval in ms (default: 50) */
    pollInterval?: number;
}

/**
 * Start polling for module states and create decorations.
 *
 * This is a fully generic system that works with any module that has:
 * - `argument_spans`: Document offsets for positional arguments
 * - `param_spans`: Map of param name -> { spans, source }
 *
 * For each param with spans, it finds the corresponding argument_span,
 * handles interpolation mapping if needed, and creates Monaco decorations.
 *
 * IMPORTANT: For non-interpolated spans, we use Monaco's tracked decorations
 * with stickiness so they automatically move when the user types. We only
 * create these decorations once (when we first see the argument_spans),
 * then during polling we use model.getDecorationRange() to get current positions.
 */
export function startModuleStatePolling({
    editor,
    monaco,
    currentFile,
    runningBufferId,
    activeDecorationRef,
    getModuleStates,
    activeClassName = 'active-seq-step',
    pollInterval = 50,
}: ModuleStatePollingParams): () => void {
    // The active-highlight collection is bound to a single editor instance,
    // and this session may target a recreated editor. Drop any collection
    // from a previous session so the first poll creates one on this editor.
    if (activeDecorationRef.current) {
        activeDecorationRef.current.clear();
        activeDecorationRef.current = null;
    }

    // Only track if viewing the running buffer
    if (currentFile !== runningBufferId) {
        return () => {};
    }

    const interval = setInterval(async () => {
        try {
            const states = await getModuleStates();
            const newDecorations: editor.IModelDeltaDecoration[] = [];
            const model = editor.getModel();
            if (!model) {
                return;
            }

            // Reuse the model's cache: its tracked decorations are live in
            // the model and already follow edits, whereas rebuilding from
            // argument_spans would resolve evaluation-time offsets against a
            // possibly-edited document.
            let globalCache = modelCaches.get(model);
            if (!globalCache) {
                globalCache = new Map();
                modelCaches.set(model, globalCache);
            }

            const getTextInSpan = (span: SourceSpan): string => {
                const startPos = model.getPositionAt(span.start);
                const endPos = model.getPositionAt(span.end);
                return model.getValueInRange({
                    endColumn: endPos.column,
                    endLineNumber: endPos.lineNumber,
                    startColumn: startPos.column,
                    startLineNumber: startPos.lineNumber,
                });
            };

            // Clean up cache entries for modules that no longer exist in the patch.
            // Without this, tracked decorations from removed modules would linger.
            for (const [cachedModuleId, moduleCache] of globalCache) {
                if (!(cachedModuleId in states)) {
                    for (const paramCache of moduleCache.values()) {
                        clearTrackedDecorations(model, paramCache);
                    }
                    globalCache.delete(cachedModuleId);
                }
            }

            for (const [moduleId, state] of Object.entries(states)) {
                const typedState = state as ModuleStateWithSpans;

                // Need both argument_spans and param_spans
                const argumentSpans = typedState.argument_spans;
                const paramSpans = typedState.param_spans;

                if (!argumentSpans || !paramSpans) {
                    continue;
                }

                // Get or create module cache
                let moduleCache = globalCache.get(moduleId);
                if (!moduleCache) {
                    moduleCache = new Map();
                    globalCache.set(moduleId, moduleCache);
                }

                // Process each param that has spans
                for (const [paramName, paramInfo] of Object.entries(
                    paramSpans,
                )) {
                    const { spans, source: evaluatedSource } = paramInfo;

                    // A param with no currently active spans (e.g. an arrange
                    // section that has not started playing) still gets its
                    // tracked decorations created below: anchoring must
                    // happen on the first poll after evaluate, while the
                    // offsets still match the document.
                    const activeSpans = spans ?? [];

                    // Get the document position for this argument
                    const argSpan = argumentSpans[paramName];
                    if (!argSpan) {
                        continue;
                    }

                    // Get or create param cache
                    let paramCache = moduleCache.get(paramName);
                    if (!paramCache) {
                        paramCache = { hasInterpolations: false };
                        moduleCache.set(paramName, paramCache);
                    }

                    // Check if argument span changed (new patch was submitted)
                    const argSpanChanged =
                        !paramCache.argumentSpan ||
                        paramCache.argumentSpan.start !== argSpan.start ||
                        paramCache.argumentSpan.end !== argSpan.end;

                    // Check if the pattern's evaluated source changed (a step
                    // was edited). A same-width value edit leaves both argSpan
                    // and all_spans unchanged, but still rewrites the text and
                    // can collapse the edited step's tracked decoration — so the
                    // source must be compared independently of argSpanChanged.
                    const sourceChanged =
                        paramCache.lastSource !== evaluatedSource;

                    if (argSpanChanged || sourceChanged) {
                        // Clear old tracked decorations if any
                        clearTrackedDecorations(model, paramCache);
                        paramCache.lastSource = evaluatedSource;

                        paramCache.argumentSpan = argSpan;
                        paramCache.positionMapper = undefined;
                        paramCache.evaluatedContentForMapper = undefined;

                        // Extract source content from document
                        const startPos = model.getPositionAt(argSpan.start);
                        const endPos = model.getPositionAt(argSpan.end);
                        const sourceText = model.getValueInRange({
                            endColumn: endPos.column,
                            endLineNumber: endPos.lineNumber,
                            startColumn: startPos.column,
                            startLineNumber: startPos.lineNumber,
                        });

                        // Check if it's a template literal with interpolations
                        paramCache.hasInterpolations =
                            sourceText.includes('${');
                        paramCache.sourceContent = sourceText;
                    }

                    // Process spans with or without interpolation mapping
                    if (paramCache.hasInterpolations && evaluatedSource) {
                        // Look up interpolation resolutions once for this param
                        const interpolationResolutions =
                            getActiveInterpolationResolutions();
                        const spanKey = `${argSpan.start}:${argSpan.end}`;
                        const resolutions =
                            interpolationResolutions?.get(spanKey);

                        // Build mapper if needed (evaluated source changed)
                        if (
                            paramCache.evaluatedContentForMapper !==
                            evaluatedSource
                        ) {
                            // Strip quotes from source content for mapping
                            const sourceWithoutQuotes = stripQuotes(
                                paramCache.sourceContent || '',
                            );

                            // Prefer building regions from resolution data (accurate)
                            // Over indexOf-based text matching (can fail when
                            // Interpolated content contains literal piece substrings)
                            let regions: InterpolationRegion[] | null = null;
                            if (resolutions) {
                                regions =
                                    buildInterpolationRegionsFromResolutions(
                                        sourceWithoutQuotes,
                                        resolutions,
                                    );
                            }
                            if (!regions) {
                                regions = extractInterpolationRegions(
                                    sourceWithoutQuotes,
                                    evaluatedSource,
                                );
                            }
                            if (regions) {
                                paramCache.positionMapper =
                                    buildPositionMapper(regions);
                            } else {
                                paramCache.positionMapper = undefined;
                            }
                            paramCache.evaluatedContentForMapper =
                                evaluatedSource;

                            // Mapper changed — tracked decorations need recreating
                            clearTrackedDecorations(model, paramCache);
                        }

                        if (!paramCache.positionMapper) {
                            continue;
                        }

                        // Create tracked decorations once for all interpolated spans,
                        // Mapping each evaluated position to its document position
                        // (either in the template literal source or a const literal).
                        const allSpans = paramInfo.all_spans;

                        if (
                            !paramCache.trackedDecorationsCreated &&
                            allSpans &&
                            allSpans.length > 0
                        ) {
                            const decorationsToCreate: editor.IModelDeltaDecoration[] =
                                [];
                            const spanIds: string[] = [];

                            for (const [evalStart, evalEnd] of allSpans) {
                                const sourceStart =
                                    paramCache.positionMapper(evalStart);
                                const sourceEnd =
                                    paramCache.positionMapper(evalEnd);

                                let startOffset: number | null = null;
                                let endOffset: number | null = null;

                                if (
                                    sourceStart !== null &&
                                    sourceEnd !== null
                                ) {
                                    // Positions map to source text within the template literal
                                    startOffset =
                                        argSpan.start + 1 + sourceStart;
                                    endOffset = argSpan.start + 1 + sourceEnd;
                                } else if (resolutions) {
                                    // Positions inside interpolation result — redirect to const literal
                                    const resolvedStart =
                                        resolveInterpolatedPosition(
                                            evalStart,
                                            resolutions,
                                            getTextInSpan,
                                            'start',
                                        );
                                    const resolvedEnd =
                                        resolveInterpolatedPosition(
                                            evalEnd,
                                            resolutions,
                                            getTextInSpan,
                                            'end',
                                        );
                                    if (
                                        resolvedStart !== null &&
                                        resolvedEnd !== null
                                    ) {
                                        startOffset = resolvedStart;
                                        endOffset = resolvedEnd;
                                    }
                                }

                                if (
                                    startOffset !== null &&
                                    endOffset !== null
                                ) {
                                    const spanId = `${evalStart}:${evalEnd}`;
                                    spanIds.push(spanId);

                                    const startPos =
                                        model.getPositionAt(startOffset);
                                    const endPos =
                                        model.getPositionAt(endOffset);

                                    decorationsToCreate.push({
                                        options: {
                                            stickiness:
                                                monaco.editor
                                                    .TrackedRangeStickiness
                                                    .NeverGrowsWhenTypingAtEdges,
                                        },
                                        range: new monaco.Range(
                                            startPos.lineNumber,
                                            startPos.column,
                                            endPos.lineNumber,
                                            endPos.column,
                                        ),
                                    });
                                }
                            }

                            if (decorationsToCreate.length > 0) {
                                const ids = model.deltaDecorations(
                                    [],
                                    decorationsToCreate,
                                );
                                paramCache.trackedDecorationIds = new Map();
                                for (let i = 0; i < spanIds.length; i++) {
                                    paramCache.trackedDecorationIds.set(
                                        spanIds[i],
                                        ids[i],
                                    );
                                }
                            }

                            paramCache.trackedDecorationsCreated = true;
                        }

                        // Use tracked decorations for active spans
                        if (paramCache.trackedDecorationIds) {
                            for (const [spanStart, spanEnd] of activeSpans) {
                                const spanId = `${spanStart}:${spanEnd}`;
                                const decoId =
                                    paramCache.trackedDecorationIds.get(spanId);
                                if (!decoId) {
                                    continue;
                                }

                                const range = model.getDecorationRange(decoId);
                                if (!range || range.isEmpty()) {
                                    continue;
                                }

                                newDecorations.push({
                                    options: {
                                        className: activeClassName,
                                        isWholeLine: false,
                                    },
                                    range,
                                });
                            }
                        }
                    } else {
                        // No interpolations - use tracked decorations with all_spans
                        // Create tracked decorations for ALL spans once (when we first see this param)
                        // Then during polling, just look up which ones are currently active

                        const allSpans = paramInfo.all_spans;

                        // Create tracked decorations if we haven't yet and we have all_spans
                        if (
                            !paramCache.trackedDecorationsCreated &&
                            allSpans &&
                            allSpans.length > 0
                        ) {
                            const decorationsToCreate: editor.IModelDeltaDecoration[] =
                                [];
                            const spanIds: string[] = [];

                            for (const [spanStart, spanEnd] of allSpans) {
                                const spanId = `${spanStart}:${spanEnd}`;
                                spanIds.push(spanId);

                                // +1 to skip opening quote in document
                                const startOffset =
                                    argSpan.start + 1 + spanStart;
                                const endOffset = argSpan.start + 1 + spanEnd;

                                const startPos =
                                    model.getPositionAt(startOffset);
                                const endPos = model.getPositionAt(endOffset);

                                decorationsToCreate.push({
                                    options: {
                                        // Use stickiness so decorations track with text edits
                                        stickiness:
                                            monaco.editor.TrackedRangeStickiness
                                                .NeverGrowsWhenTypingAtEdges,
                                        // No visual style - these are invisible tracking decorations
                                    },
                                    range: new monaco.Range(
                                        startPos.lineNumber,
                                        startPos.column,
                                        endPos.lineNumber,
                                        endPos.column,
                                    ),
                                });
                            }

                            const ids = model.deltaDecorations(
                                [],
                                decorationsToCreate,
                            );

                            // Build span ID -> decoration ID map
                            paramCache.trackedDecorationIds = new Map();
                            for (let i = 0; i < spanIds.length; i++) {
                                paramCache.trackedDecorationIds.set(
                                    spanIds[i],
                                    ids[i],
                                );
                            }

                            paramCache.trackedDecorationsCreated = true;
                        }

                        // If we have tracked decorations, use them to get current positions for active spans
                        if (paramCache.trackedDecorationIds) {
                            for (const [spanStart, spanEnd] of activeSpans) {
                                const spanId = `${spanStart}:${spanEnd}`;
                                const decoId =
                                    paramCache.trackedDecorationIds.get(spanId);

                                if (!decoId) {
                                    // This span wasn't in all_spans - shouldn't happen but skip
                                    continue;
                                }

                                // Get the current (tracked) range of this decoration
                                const range = model.getDecorationRange(decoId);
                                if (!range || range.isEmpty()) {
                                    continue;
                                }

                                newDecorations.push({
                                    options: {
                                        className: activeClassName,
                                        isWholeLine: false,
                                    },
                                    range,
                                });
                            }
                        }
                    }
                }
            }

            // Update active decorations (the visual highlighting)
            if (activeDecorationRef.current) {
                activeDecorationRef.current.set(newDecorations);
            } else {
                activeDecorationRef.current =
                    editor.createDecorationsCollection(newDecorations);
            }
        } catch {
            // Ignore polling errors
        }
    }, pollInterval);

    // Tracked anchor decorations are owned by the model and cached per model,
    // so a later session reuses their live (edit-tracked) ranges;
    // evaluation-time offsets are stale after any edit.
    return () => {
        clearInterval(interval);
    };
}
