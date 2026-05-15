// @modular/dsl — Operator DSL runtime + (in later PRs) generated factories.
//
// Public surface for the Electron main process. Renderer code should import
// only the type-only exports below; runtime helpers depend on `@modular/core`
// (a native module) and on `new Function()`, both main-process-only.

export { executePatchScript, validateDSLSyntax } from './runtime/executor';
export type {
    DSLExecutionResult,
    DSLExecutionOptions,
    WavsFolderNode,
} from './runtime/executor/types';

export { hz, note, bpm } from './runtime/factory/units';

export type {
    AnalysisResult,
    SpanAnalyzer,
    SpanRegistry,
    CallSiteKey,
    CallSiteSpans,
    CallSiteSpanRegistry,
    CallExpressionSpan,
    SourceSpan,
    ResolvedInterpolation,
    InterpolationResolutionMap,
} from './runtime/types/analysis';
export {
    setActiveInterpolationResolutions,
    getActiveInterpolationResolutions,
    EMPTY_ANALYSIS_RESULT,
} from './runtime/types/analysis';

export {
    ModuleOutput,
    ModuleOutputWithRange,
    Collection,
    CollectionWithRange,
    BaseCollection,
    DeferredModuleOutput,
    DeferredCollection,
    Bus,
    GraphBuilder,
    $c,
    $r,
    $cartesian,
    replaceSignals,
} from './runtime/graph';
export type {
    Signal,
    PolySignal,
    OrArray,
    BufferOutputRef,
    SourceLocation,
    StereoOutOptions,
    MonoOutOptions,
    OutGroup,
    StereoOutGroup,
    MonoOutGroup,
    SendGroup,
    OutputSchemaWithRange,
    ScopeWithLocation,
    ResolvedModuleOutput,
    FactoryFunction,
    ElementsOf,
} from './runtime/graph';
