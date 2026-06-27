//! Pre-deserialized params infrastructure.
//!
//! Types and utilities for deserializing module params on the main thread
//! and applying them cheaply on the audio thread.

use napi_derive::napi;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Argument spans
// ---------------------------------------------------------------------------

/// Key used for internal metadata field storing argument source spans.
/// This constant is shared across Rust validation, derive macros, and TypeScript.
pub const ARGUMENT_SPANS_KEY: &str = "__argument_spans";

/// Represents a character span in source code, used for argument highlighting.
/// Start and end are absolute character offsets (0-based, end exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
#[napi(object)]
pub struct ArgumentSpan {
    /// Absolute start offset (0-based)
    pub start: u32,
    /// Absolute end offset (exclusive)
    pub end: u32,
}

// ---------------------------------------------------------------------------
// CloneableParams trait
// ---------------------------------------------------------------------------

/// Object-safe trait for cloning type-erased params boxes.
///
/// Blanket-implemented for all `T: Clone + Send + 'static + Connect`, so
/// concrete params structs only need to derive `Clone` and `Connect`. Every
/// module's params struct already does both — `Connect` is required to
/// resolve cable references at runtime and now also so the type-erased box
/// can expose [`collect_cables`](Self::collect_cables) for SCC analysis.
pub trait CloneableParams: Send + 'static {
    fn clone_box(&self) -> Box<dyn CloneableParams>;
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any>;

    /// Walk the params and push every producer module ID it references
    /// (cables, buffer sources, table-internal signals) into `sink`.
    ///
    /// Used to build the cable adjacency map before module construction so
    /// `graph_analysis::analyze` can decide whether each module needs `Block`
    /// or `Sample` processing and compute the cache-efficient processing
    /// order, without requiring the audio thread to know the params type.
    fn collect_cables(&self, sink: &mut Vec<String>);
}

impl<T: Clone + Send + 'static + crate::types::Connect> CloneableParams for T {
    fn clone_box(&self) -> Box<dyn CloneableParams> {
        Box::new(self.clone())
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
    fn collect_cables(&self, sink: &mut Vec<String>) {
        <T as crate::types::Connect>::collect_cables(self, sink)
    }
}

impl Clone for Box<dyn CloneableParams> {
    fn clone(&self) -> Self {
        (**self).clone_box()
    }
}

// ---------------------------------------------------------------------------
// Deserialized / cached params
// ---------------------------------------------------------------------------

/// Pre-deserialized module params, sent through the ring buffer from the main
/// thread to the audio thread.
#[derive(Clone)]
pub struct DeserializedParams {
    /// Type-erased concrete params (e.g. `Box<SineOscillatorParams>`).
    pub params: Box<dyn CloneableParams>,
    /// Derived output channel count for this module.
    pub channel_count: usize,
}

/// Cached portion of deserialized params (excludes argument spans).
///
/// Stored in the LRU cache keyed by `(module_type, stripped_params_json)`.
/// Argument spans are excluded because they depend on source positions,
/// not param values — identical params at different source locations must
/// share the same cache entry.
#[derive(Clone)]
pub struct CachedParams {
    /// Type-erased concrete params.
    pub params: Box<dyn CloneableParams>,
    /// Derived output channel count.
    pub channel_count: usize,
}

/// Function that deserializes a JSON value (with `__argument_spans` already
/// stripped) into a `CachedParams`.
pub type ParamsDeserializer =
    fn(serde_json::Value) -> Result<CachedParams, crate::param_errors::ModuleParamErrors>;

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Remove `__argument_spans` (the deserializers reject it). The spans are read
/// from the raw params elsewhere, so they aren't returned here.
pub fn strip_argument_spans(params: serde_json::Value) -> serde_json::Value {
    match params {
        serde_json::Value::Object(mut obj) => {
            obj.remove(ARGUMENT_SPANS_KEY);
            serde_json::Value::Object(obj)
        }
        other => other,
    }
}
