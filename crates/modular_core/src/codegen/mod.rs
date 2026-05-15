//! Codegen for `@modular/dsl`.
//!
//! Reads `dsp::schema()` and emits TypeScript factories, the Monaco lib, and
//! supporting metadata. Driven by the `generate-dsl` bin. Only compiled when
//! the bin pulls these modules in (no impact on the audio engine).

pub mod type_resolver;
