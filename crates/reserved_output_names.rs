/// Reserved output names that cannot be used as module output port names.
/// These names conflict with built-in properties and methods on `ModuleOutput`,
/// `ModuleOutputWithRange`, `BaseCollection`, `Collection`, `CollectionWithRange`,
/// `DeferredModuleOutput`, and `DeferredCollection`, as well as JavaScript built-in
/// property names.
///
/// This file is the single source of truth. It is `include!()`-ed by both
/// `modular_derive` (compile-time validation) and `modular` (NAPI export to JS).
/// The names are in camelCase because the TypeScript DSL converts all output names
/// to camelCase before checking. The Rust proc-macro derives snake_case variants
/// automatically from this list.
const RESERVED_OUTPUT_NAMES: &[&str] = &[
    // ModuleOutput properties
    "builder",
    "moduleId",
    "portName",
    "channel",
    // ModuleOutput methods
    "amplitude",
    "amp",
    "exp",
    "gain",
    "shift",
    "addHz",
    "mulHz",
    "scope",
    "out",
    "outMono",
    "pipe",
    "pipeMix",
    "mix",
    "toString",
    // ModuleOutputWithRange properties
    "minValue",
    "maxValue",
    // ModuleOutputWithRange methods
    "range",
    // ModuleOutput / BaseCollection internal converter methods
    "withRange",
    // BaseCollection / Collection / CollectionWithRange properties
    "items",
    "length",
    // DeferredModuleOutput / DeferredCollection methods
    "set",
    // JavaScript built-in property names
    "constructor",
    "prototype",
    "__proto__",
];
