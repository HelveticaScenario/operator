//! Modular synthesizer core library
//!
//! This crate provides the core DSP functionality for a modular synthesizer.
//! It is a pure library with no I/O, protocol handling, or serialization concerns.
//! Those responsibilities belong in the server layer.

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate modular_derive;

extern crate mi_plaits_dsp;
extern crate parking_lot;
extern crate serde;
extern crate serde_json;
extern crate simple_easing;

pub mod block_port;
pub mod dsp;
pub mod param_errors;
pub mod params;
pub mod patch;
pub mod pattern_system;
pub mod poly;
pub mod types;

// Re-export commonly used items
pub use block_port::BlockPort;
pub use patch::Patch;

pub use poly::{
    MonoSignal, MonoSignalExt, PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt,
};

pub use params::{
    ARGUMENT_SPANS_KEY, ArgumentSpan, CachedParams, CloneableParams, DeserializedParams,
    ParamsDeserializer, extract_argument_spans,
};

pub use types::{
    Buffer, BufferData, ExternalClockState, Module, ModuleSchema, ModuleState, PatchGraph,
    ProcessingMode, ROOT_ID, ROOT_OUTPUT_PORT, SampleBuffer, Sampleable, SampleableConstructor,
    SampleableMap, Signal, SignalExt, SignalParamSchema, Wav, WavData,
};
