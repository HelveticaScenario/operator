//! `$cycle`'s implementation of the generic per-module editor state
//! ([`crate::module_state`]), split so the audio thread never allocates.
//!
//! The editor highlights a `$cycle`'s playing step(s) by polling per-module
//! state. The audio thread writes only the live part — the active span ranges
//! per pattern source — into a pre-allocated [`SeqHighlightState`] (the module's
//! [`ModuleLiveState`]). The main thread holds the parts that don't change while
//! playing ([`SeqHighlightMeta`]: `argument_spans` and each source's `source`
//! and `all_spans`, the module's [`ModuleStateMeta`]) and builds the editor JSON
//! on poll. [`seq_state_builder`] wires both halves into the registry.

use std::any::Any;

use serde_json::{Value, json};

use crate::module_state::{ModuleLiveState, ModuleStateMeta};

/// Max pattern sources a `$cycle` highlight publishes. A plain pattern has one
/// source; a chained `$p.s(...).add(...)` has one per link. Extra sources are
/// not highlighted.
pub const MAX_SEQ_SOURCES: usize = 16;

/// Max active highlight spans published per source each callback. Usually a
/// small handful; extra spans are dropped from the highlight.
pub const MAX_SEQ_HIGHLIGHT_SPANS: usize = 32;

/// Snapshot of a `$cycle`'s currently-highlighted step spans. `Copy` and
/// allocation-free, so the audio thread can write it into a pre-allocated slot.
#[derive(Clone, Copy)]
pub struct SeqHighlightState {
    /// Active span count per source (`<= MAX_SEQ_HIGHLIGHT_SPANS`). A source with
    /// no active spans this callback has count `0`.
    pub span_counts: [u32; MAX_SEQ_SOURCES],
    /// `[start, end)` document offsets of active spans, per source.
    pub spans: [[(u32, u32); MAX_SEQ_HIGHLIGHT_SPANS]; MAX_SEQ_SOURCES],
}

impl Default for SeqHighlightState {
    fn default() -> Self {
        Self {
            span_counts: [0; MAX_SEQ_SOURCES],
            spans: [[(0, 0); MAX_SEQ_HIGHLIGHT_SPANS]; MAX_SEQ_SOURCES],
        }
    }
}

impl SeqHighlightState {
    /// Clear the active spans before writing the next snapshot.
    pub fn reset(&mut self) {
        self.span_counts = [0; MAX_SEQ_SOURCES];
    }

    /// Record one active span for `source_idx`, skipping duplicates (several
    /// voices can land on the same step). Spans past the caps are ignored, never
    /// panicking.
    pub fn push_span(&mut self, source_idx: usize, start: u32, end: u32) {
        if source_idx >= MAX_SEQ_SOURCES {
            return;
        }
        let count = (self.span_counts[source_idx] as usize).min(MAX_SEQ_HIGHLIGHT_SPANS);
        if self.spans[source_idx][..count].contains(&(start, end)) {
            return;
        }
        if count >= MAX_SEQ_HIGHLIGHT_SPANS {
            return;
        }
        self.spans[source_idx][count] = (start, end);
        self.span_counts[source_idx] = (count + 1) as u32;
    }

    /// The recorded active spans for `source_idx`.
    pub fn spans_for(&self, source_idx: usize) -> &[(u32, u32)] {
        if source_idx >= MAX_SEQ_SOURCES {
            return &[];
        }
        let count = (self.span_counts[source_idx] as usize).min(MAX_SEQ_HIGHLIGHT_SPANS);
        &self.spans[source_idx][..count]
    }
}

impl ModuleLiveState for SeqHighlightState {
    fn reset(&mut self) {
        SeqHighlightState::reset(self);
    }

    fn clone_box(&self) -> Box<dyn ModuleLiveState> {
        Box::new(*self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The parts of a `$cycle`'s highlight output that don't change while playing,
/// held on the main thread (the module's [`ModuleStateMeta`]). Built from the
/// patch params (see [`seq_state_builder`]) and paired with the live
/// [`SeqHighlightState`] on poll.
pub struct SeqHighlightMeta {
    /// The module's `__argument_spans` (document offsets from the editor), or
    /// `Null` if the patch carried none.
    pub argument_spans: Value,
    /// One entry per pattern source, in order.
    pub sources: Vec<SeqSourceHighlight>,
}

/// Highlight metadata for one pattern source.
pub struct SeqSourceHighlight {
    /// The `param_spans` key: `"pattern"` or `"pattern.{i}"`.
    pub key: String,
    /// The pattern's source string.
    pub source: Value,
    /// Every span in the pattern.
    pub all_spans: Value,
}

impl ModuleStateMeta for SeqHighlightMeta {
    /// Pair this metadata with a live span snapshot to produce the editor JSON.
    /// Shape: `{ "argument_spans": {...}, "param_spans": { <key>: { spans,
    /// source, all_spans } } }`. Returns `Null` if `live` is not a
    /// [`SeqHighlightState`] (cannot happen — the builder pairs the two halves).
    fn build_json(&self, live: &dyn ModuleLiveState) -> Value {
        let Some(pod) = live.as_any().downcast_ref::<SeqHighlightState>() else {
            return Value::Null;
        };
        let mut param_spans = serde_json::Map::with_capacity(self.sources.len());
        for (i, src) in self.sources.iter().enumerate() {
            let spans: Vec<Value> = pod
                .spans_for(i)
                .iter()
                .map(|&(start, end)| json!([start, end]))
                .collect();
            param_spans.insert(
                src.key.clone(),
                json!({
                    "spans": spans,
                    "source": src.source,
                    "all_spans": src.all_spans,
                }),
            );
        }
        json!({
            "argument_spans": self.argument_spans,
            "param_spans": Value::Object(param_spans),
        })
    }
}

/// Build a `$cycle`'s editor-state halves from its patch params, on the main
/// thread: the empty live span slot the audio thread fills, plus the immutable
/// [`SeqHighlightMeta`]. Returns `None` if the pattern won't parse. Registered as
/// the `$cycle` [`ModuleStateBuilder`](crate::module_state::ModuleStateBuilder).
///
/// The pattern is parsed the same way the engine parses it, so the keys and each
/// source's `source`/`all_spans` match what the audio thread publishes for every
/// pattern kind. A single non-chained source is keyed `"pattern"`; anything else
/// is keyed `"pattern.{i}"`.
pub fn seq_state_builder(
    params: &Value,
) -> Option<(Box<dyn ModuleLiveState>, Box<dyn ModuleStateMeta>)> {
    let argument_spans = params
        .get("__argument_spans")
        .cloned()
        .unwrap_or(Value::Null);

    let pattern_json = params.get("pattern")?.clone();
    let pattern: super::seq_value::SeqPatternParam =
        deserr::deserialize::<_, _, crate::param_errors::ModuleParamErrors>(pattern_json).ok()?;
    let per_source = pattern.per_source();
    let num_sources = per_source.len().max(1);

    let source_json = |meta: &super::seq_value::SeqSourceMeta| SeqSourceHighlight {
        key: String::new(),
        source: Value::String(meta.source.clone()),
        all_spans: serde_json::to_value(&meta.all_spans)
            .unwrap_or_else(|_| Value::Array(Vec::new())),
    };

    let mut sources = Vec::with_capacity(num_sources);
    if !pattern.is_multi_source() && num_sources == 1 {
        if let Some(meta) = per_source.first() {
            sources.push(SeqSourceHighlight {
                key: "pattern".to_string(),
                ..source_json(meta)
            });
        }
    } else {
        for (i, meta) in per_source.iter().enumerate() {
            sources.push(SeqSourceHighlight {
                key: format!("pattern.{i}"),
                ..source_json(meta)
            });
        }
    }
    let meta = SeqHighlightMeta {
        argument_spans,
        sources,
    };
    Some((Box::new(SeqHighlightState::default()), Box::new(meta)))
}
