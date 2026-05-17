// Public surface of the DSL graph module.
// Re-exports keep backward compat with callers that imported from `./GraphBuilder`.
export {
    PORT_MAX_CHANNELS,
    GAIN_CURVE_EXP,
    ResolvedModuleOutputSchema,
} from './types';
export type {
    ScopeWithLocation,
    OutputSchemaWithRange,
    ResolvedModuleOutput,
    OrArray,
    Signal,
    PolySignal,
    BufferOutputRef,
    StereoOutOptions,
    MonoOutOptions,
    StereoOutGroup,
    MonoOutGroup,
    OutGroup,
    SendGroup,
    SourceLocation,
    FactoryFunction,
} from './types';
export { $cartesian } from './cartesian';
export type { ElementsOf } from './cartesian';
export {
    replaceValues,
    replaceSignals,
    replaceDeferredStrings,
    replaceDeferred,
    valueToSignal,
} from './signalResolution';
export { Bus } from './bus';
export { ModuleOutput } from './moduleOutput';
export { ModuleOutputWithRange } from './moduleOutputWithRange';
export { DeferredModuleOutput } from './deferredOutput';
export { BaseCollection, Collection, $c } from './collection';
export { CollectionWithRange, $r } from './collectionWithRange';
export { DeferredCollection } from './deferredCollection';
export { ModuleNode } from './moduleNode';
export { GraphBuilder } from './builder';
