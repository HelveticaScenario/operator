//! Table types — function objects usable as module params.
//!
//! A `Table` (see `crate::types::Table`) is a param type that evaluates a warp function
//! at a normalized phase `x ∈ [0, 1]`. Each variant carries `PolySignal` fields for its
//! dynamic parameters; the consumer module resolves signals via `connect()` and calls
//! `table.evaluate(x, channel)` per-sample on the audio thread.

pub mod warp;
