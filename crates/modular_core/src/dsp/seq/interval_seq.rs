//! IntervalSeq module - A scale-degree sequencer with additive patterns.
//!
//! This module sequences scale degrees using one or more mini notation patterns
//! combined via left-fold `app_left` addition (matching Strudel's `.add.in`).
//! The first pattern determines rhythmic structure; subsequent patterns add
//! their values at each event.
//!
//! The sequencer outputs:
//! - CV: V/Oct pitch (quantized to scale)
//! - Gate: High while note is active
//! - Trig: Short pulse at note onset

use std::cmp::Ordering;

use arrayvec::ArrayVec;
use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    MonoSignal, Patch,
    dsp::{
        utilities::quantizer::ScaleParam,
        utils::{TempGate, TempGateState, midi_to_voct_f64, min_gate_samples},
    },
    pattern_system::Pattern,
    poly::{MonoSignalExt, PORT_MAX_CHANNELS, PolyOutput},
    types::Connect,
};

/// Value type for interval patterns: either a degree or rest.
#[derive(Clone, Debug)]
pub enum IntervalValue {
    /// Scale degree (can be negative for downward movement)
    Degree(i32),
    /// Rest - no output, gate low
    Rest,
}

impl IntervalValue {
    pub fn is_rest(&self) -> bool {
        matches!(self, IntervalValue::Rest)
    }

    pub fn degree(&self) -> Option<i32> {
        match self {
            IntervalValue::Degree(d) => Some(*d),
            IntervalValue::Rest => None,
        }
    }
}

impl crate::pattern_system::mini::convert::FromMiniAtom for IntervalValue {
    fn from_atom(
        atom: &crate::pattern_system::mini::ast::AtomValue,
    ) -> Result<Self, crate::pattern_system::mini::convert::ConvertError> {
        use crate::pattern_system::mini::ast::AtomValue;
        use crate::pattern_system::mini::convert::ConvertError;
        match atom {
            AtomValue::Number(n) => {
                if !n.is_finite() || n.fract() != 0.0 {
                    return Err(ConvertError::InvalidAtom(format!(
                        "IntervalValue requires integer scale degrees, got {n}"
                    )));
                }
                Ok(IntervalValue::Degree(*n as i32))
            }
            AtomValue::Hz(_) => Err(ConvertError::InvalidAtom(
                "IntervalValue does not accept Hz atoms; $iCycle interprets atoms as scale-degree integers (use $cycle for unquantized pitch)".into(),
            )),
            AtomValue::Note { .. } => Err(ConvertError::InvalidAtom(
                "IntervalValue does not accept note atoms; $iCycle interprets atoms as scale-degree integers (use $cycle for unquantized pitch)".into(),
            )),
        }
    }

    fn from_list(
        atoms: &[crate::pattern_system::mini::ast::AtomValue],
    ) -> Result<Self, crate::pattern_system::mini::convert::ConvertError> {
        if atoms.len() == 1 {
            Self::from_atom(&atoms[0])
        } else {
            Err(crate::pattern_system::mini::convert::ConvertError::ListNotSupported)
        }
    }

    fn combine_with_head(
        _head_atoms: &[crate::pattern_system::mini::ast::AtomValue],
        _tail: &Self,
    ) -> Result<Self, crate::pattern_system::mini::convert::ConvertError> {
        Err(crate::pattern_system::mini::convert::ConvertError::ListNotSupported)
    }

    fn rest_value() -> Option<Self> {
        Some(IntervalValue::Rest)
    }

    fn supports_rest() -> bool {
        true
    }
}

impl crate::pattern_system::mini::convert::HasRest for IntervalValue {
    fn rest_value() -> Self {
        IntervalValue::Rest
    }
}

/// Source representation for interval patterns: either a single parsed
/// payload or an array of payloads combined via `app_left` addition.
///
/// The wire shape is the `ParsedPatternPayload` `{ ast, source, all_spans }`
/// emitted by the TypeScript `$p(...)` helper, or an array of those for
/// `$iCycle([$p(...), $p(...)])`.
#[derive(Clone, Debug, JsonSchema)]
#[serde(untagged)]
pub enum IntervalPatternSource {
    Single(crate::dsp::seq::seq_value::ParsedPatternPayload),
    Multiple(Vec<crate::dsp::seq::seq_value::ParsedPatternPayload>),
}

impl Default for IntervalPatternSource {
    fn default() -> Self {
        Self::Single(crate::dsp::seq::seq_value::ParsedPatternPayload::default())
    }
}

impl IntervalPatternSource {
    /// Get the individual payloads.
    fn payloads(&self) -> Vec<&crate::dsp::seq::seq_value::ParsedPatternPayload> {
        match self {
            Self::Single(p) => vec![p],
            Self::Multiple(v) => v.iter().collect(),
        }
    }
}

/// Per-source metadata retained for span tracking.
#[derive(Clone, Debug, Default)]
pub struct SourceMeta {
    source: String,
    all_spans: Vec<(usize, usize)>,
}

/// Flat span entry — encodes (pattern_idx, start, end) without nested Vecs.
/// Stored in a per-cycle arena (`CycleStorage::span_arena`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlatSpan {
    pub pattern_idx: u8,
    pub start: u32,
    pub end: u32,
}

/// Per-cycle storage for IntervalSeq. Combined-degree haps + flat span arena.
pub(crate) type CycleStorage = super::cache::CycleStorage<CombinedHap, FlatSpan>;

/// A pattern parameter for interval/degree patterns.
///
/// Accepts either a single pattern string or an array of strings.
/// Multiple strings are parsed individually then combined via `app_left`
/// addition (left-fold), matching Strudel's `.add.in` behavior.
#[derive(Clone, Debug)]
pub struct IntervalPatternParam {
    /// The source value (string or array of strings) — drives the JSON schema
    #[allow(dead_code)]
    source: IntervalPatternSource,

    /// The combined pattern (after left-fold for Multiple)
    combined_pattern: Option<Pattern<IntervalValue>>,

    /// Per-source metadata for span tracking
    per_source: Vec<SourceMeta>,

    /// Number of source strings that contributed to the combined pattern
    num_sources: usize,

    /// Pre-computed combined haps for cycles 0..PARAM_CACHE_CYCLES.
    cached_haps: Vec<CycleStorage>,

    /// Hint for sizing audio-thread module_cache slots — max hap count seen
    /// across sampled cycles at parse time.
    max_haps_per_cycle: usize,

    /// Hint for sizing the per-cycle span arena.
    max_spans_per_cycle: usize,
}

impl Default for IntervalPatternParam {
    fn default() -> Self {
        Self {
            source: IntervalPatternSource::default(),
            combined_pattern: None,
            per_source: Vec::new(),
            num_sources: 0,
            cached_haps: Vec::new(),
            max_haps_per_cycle: 0,
            max_spans_per_cycle: 0,
        }
    }
}

impl IntervalPatternParam {
    /// Lower a parsed payload to a `Pattern<IntervalValue>`. Spans
    /// already collected client-side; just pass them through.
    fn convert_one(
        payload: &crate::dsp::seq::seq_value::ParsedPatternPayload,
    ) -> Result<Pattern<IntervalValue>, String> {
        crate::pattern_system::mini::convert::<IntervalValue>(&payload.ast)
            .map_err(|e| e.to_string())
    }

    /// Build from an `IntervalPatternSource`, lowering and combining patterns.
    fn from_source(source: IntervalPatternSource) -> Result<Self, String> {
        let payloads = source.payloads();

        // Filter out empty payloads (source == "")
        let non_empty: Vec<&crate::dsp::seq::seq_value::ParsedPatternPayload> =
            payloads.iter().copied().filter(|p| !p.source.is_empty()).collect();

        if non_empty.is_empty() {
            return Ok(Self {
                per_source: payloads
                    .iter()
                    .map(|p| SourceMeta {
                        source: p.source.clone(),
                        all_spans: p.all_spans.clone(),
                    })
                    .collect(),
                num_sources: payloads.len(),
                source,
                combined_pattern: None,
                cached_haps: Vec::new(),
                max_haps_per_cycle: 0,
                max_spans_per_cycle: 0,
            });
        }

        // Lower each payload
        let mut parsed: Vec<Pattern<IntervalValue>> = Vec::new();
        let mut per_source: Vec<SourceMeta> = Vec::new();

        for p in &payloads {
            if p.source.is_empty() {
                per_source.push(SourceMeta {
                    source: p.source.clone(),
                    all_spans: p.all_spans.clone(),
                });
            } else {
                let pattern = Self::convert_one(p)?;
                per_source.push(SourceMeta {
                    source: p.source.clone(),
                    all_spans: p.all_spans.clone(),
                });
                parsed.push(pattern);
            }
        }

        // Left-fold the parsed patterns with app_left + add_interval_values.
        // strip_modifier_spans() ensures that internal modifier spans from
        // sub-expressions (e.g. euclidean notation) don't leak into the
        // positional index that extract_pattern_spans relies on.
        let mut combined = parsed[0].strip_modifier_spans();
        for p in &parsed[1..] {
            combined = combined.app_left(&p.strip_modifier_spans(), add_interval_values);
        }

        let num_sources = payloads.len();

        // Pre-compute and cache combined haps for cycles 0..PARAM_CACHE_CYCLES.
        let mut cached_haps: Vec<CycleStorage> = Vec::with_capacity(PARAM_CACHE_CYCLES);
        for cycle in 0..PARAM_CACHE_CYCLES as i64 {
            let haps = combined.query_cycle_all(cycle);
            let mut storage = CycleStorage::with_capacity(haps.len(), haps.len() * SPANS_RESERVE_PER_HAP);
            for hap in &haps {
                let span_offset = storage.span_arena.len() as u32;
                extract_pattern_spans_into(&hap.context, num_sources, &mut storage.span_arena);
                let span_len = storage.span_arena.len() as u32 - span_offset;
                storage.haps.push(CombinedHap {
                    whole_begin: hap.whole_begin,
                    whole_end: hap.whole_end,
                    part_begin: hap.part_begin,
                    part_end: hap.part_end,
                    degree: hap.value.degree(),
                    has_onset: hap.has_onset(),
                    span_offset,
                    span_len,
                });
            }
            cached_haps.push(storage);
        }

        // Derive audio-thread capacity hints from the cached cycles. With
        // PARAM_CACHE_CYCLES samples already on hand we have plenty of data
        // to size module_cache slots without extra queries.
        let max_haps = cached_haps.iter().map(|c| c.haps.len()).max().unwrap_or(0);
        let max_spans = cached_haps
            .iter()
            .map(|c| c.span_arena.len())
            .max()
            .unwrap_or(0);
        // Add headroom so occasional larger cycles don't realloc on audio thread.
        let max_haps_per_cycle = (max_haps.max(MIN_HAPS_CAP_HINT) * 3) / 2;
        let max_spans_per_cycle = (max_spans.max(MIN_SPANS_CAP_HINT) * 3) / 2;

        Ok(Self {
            source,
            combined_pattern: Some(combined),
            per_source,
            num_sources,
            cached_haps,
            max_haps_per_cycle,
            max_spans_per_cycle,
        })
    }

    /// Get the combined pattern.
    pub fn pattern(&self) -> Option<&Pattern<IntervalValue>> {
        self.combined_pattern.as_ref()
    }

    /// Number of source patterns that were combined.
    pub fn num_sources(&self) -> usize {
        self.num_sources
    }

    /// Whether the source was an array (Multiple variant).
    /// Used to determine param_spans key format: array sources always
    /// use indexed keys ("patterns.0") even with a single element,
    /// while a plain string source uses the bare key ("patterns").
    pub fn is_array_source(&self) -> bool {
        matches!(self.source, IntervalPatternSource::Multiple(_))
    }

    /// Per-source metadata for span tracking.
    pub fn per_source(&self) -> &[SourceMeta] {
        &self.per_source
    }

    /// Get the pre-computed cached storage for cycles 0..PARAM_CACHE_CYCLES.
    pub(crate) fn cached_haps(&self) -> &[CycleStorage] {
        &self.cached_haps
    }

    /// Capacity hint for pre-allocating module_cache slots.
    pub(crate) fn max_haps_per_cycle(&self) -> usize {
        self.max_haps_per_cycle
    }

    pub(crate) fn max_spans_per_cycle(&self) -> usize {
        self.max_spans_per_cycle
    }
}

impl JsonSchema for IntervalPatternParam {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        IntervalPatternSource::schema_name()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        IntervalPatternSource::json_schema(generator)
    }
}

impl Connect for IntervalPatternParam {
    fn connect(&mut self, _patch: &Patch) {
        // IntervalPatternParam has no signals to connect
    }
    fn collect_cables(&self, _sink: &mut Vec<String>) {}
    fn inject_index_ptr(&mut self, _ptr: *const std::cell::Cell<usize>) {}
}

impl<E: deserr::DeserializeError> deserr::Deserr<E> for IntervalPatternSource {
    fn deserialize_from_value<V: deserr::IntoValue>(
        value: deserr::Value<V>,
        location: deserr::ValuePointerRef<'_>,
    ) -> std::result::Result<Self, E> {
        match &value {
            deserr::Value::Map(_) => {
                let p = crate::dsp::seq::seq_value::ParsedPatternPayload::deserialize_from_value(
                    value, location,
                )?;
                Ok(IntervalPatternSource::Single(p))
            }
            deserr::Value::Sequence(_) => {
                let v = Vec::<crate::dsp::seq::seq_value::ParsedPatternPayload>::deserialize_from_value(value, location)?;
                Ok(IntervalPatternSource::Multiple(v))
            }
            _ => Err(deserr::take_cf_content(E::error::<V>(
                None,
                deserr::ErrorKind::IncorrectValueKind {
                    actual: value,
                    accepted: &[deserr::ValueKind::Map, deserr::ValueKind::Sequence],
                },
                location,
            ))),
        }
    }
}

impl<E: deserr::DeserializeError> deserr::Deserr<E> for IntervalPatternParam {
    fn deserialize_from_value<V: deserr::IntoValue>(
        value: deserr::Value<V>,
        location: deserr::ValuePointerRef<'_>,
    ) -> std::result::Result<Self, E> {
        let source = IntervalPatternSource::deserialize_from_value(value, location)?;
        Self::from_source(source).map_err(|e| {
            deserr::take_cf_content(E::error::<V>(
                None,
                deserr::ErrorKind::Unexpected { msg: e },
                location,
            ))
        })
    }
}

/// Cached hap info for voice assignment. Holds only scalars — no Arc, no
/// pattern lookup — so the audio-thread `contains()` and dedup checks are
/// pointer-free.
#[derive(Clone, Copy, Debug, Default)]
struct CachedIntervalHap {
    /// Index of this hap within the cycle's storage.
    hap_index: u32,
    /// The cycle this hap belongs to.
    cached_cycle: i64,
    /// Cached `whole_begin` for the release check.
    whole_begin: f64,
    /// Cached `whole_end` for the release check.
    whole_end: f64,
}

impl CachedIntervalHap {
    fn contains(&self, playhead: f64) -> bool {
        playhead >= self.whole_begin && playhead < self.whole_end
    }
}

/// Per-voice state for polyphonic interval sequencer.
#[derive(Clone, Debug, Default)]
struct IntervalVoiceState {
    /// Cached hap info for this voice (scalar-only)
    cached_hap: Option<CachedIntervalHap>,
    /// Quantized voltage cached at voice allocation time
    cached_voltage: f64,
    /// Gate generator for this voice
    gate: TempGate,
    /// Trigger generator for this voice
    trigger: TempGate,
    /// Whether this voice is currently active
    active: bool,
    /// Timestamp when this voice was last assigned (for LRU stealing)
    last_assigned: f64,
}

fn default_channels() -> usize {
    4
}

#[derive(Clone, Deserr, ChannelCount, JsonSchema, Connect, Debug, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
pub struct IntervalSeqParams {
    /// patterns to combine (left-fold with appLeft addition); accepts a single
    /// pattern string or an array of pattern strings
    patterns: IntervalPatternParam,
    /// scale for quantizing degrees to pitches (supports optional octave, e.g. "c3(major)")
    scale: ScaleParam,
    /// playhead position
    #[default_connection(module = RootClock, port = "playhead", channels = [0, 1])]
    #[signal(range = (0.0, 1.0))]
    #[deserr(default)]
    playhead: Option<MonoSignal>,
    /// number of polyphonic voices (1–16)
    #[serde(default = "default_channels")]
    #[deserr(default = default_channels())]
    pub channels: usize,
}

/// Channel count derivation for IntervalSeq.
///
/// Queries the pre-built combined pattern and uses a sweep-line algorithm
/// to find the maximum number of simultaneous events.
pub fn interval_seq_derive_channel_count(params: &IntervalSeqParams) -> usize {
    // If channels was explicitly set (non-default), use that
    if params.channels != default_channels() {
        return params.channels.clamp(1, PORT_MAX_CHANNELS);
    }

    derive_combined_polyphony(&params.patterns)
}

/// Derive polyphony from a single `IntervalPatternParam` whose combined
/// pattern is already built at parse time.
fn derive_combined_polyphony(param: &IntervalPatternParam) -> usize {
    const MAX_POLYPHONY: usize = 16;

    let cached = param.cached_haps();
    if cached.is_empty() {
        return 1;
    }

    // Sweep line algorithm using f64 coordinates from cached combined haps
    let mut events: Vec<(f64, i32)> = Vec::new();

    for cycle_storage in cached {
        for hap in cycle_storage.haps.iter() {
            if hap.degree.is_none() {
                continue; // Skip rests
            }
            events.push((hap.part_begin, 1));
            events.push((hap.part_end, -1));
        }
    }

    if events.is_empty() {
        return 1;
    }

    events.sort_by(
        |a, b| match a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal) {
            Ordering::Equal => a.1.cmp(&b.1),
            other => other,
        },
    );

    let mut current: usize = 0;
    let mut max_simultaneous: usize = 0;

    for (_time, delta) in events {
        if delta > 0 {
            current += 1;
            max_simultaneous = max_simultaneous.max(current);
            if max_simultaneous >= MAX_POLYPHONY {
                return MAX_POLYPHONY;
            }
        } else {
            current = current.saturating_sub(1);
        }
    }

    max_simultaneous.max(1)
}

/// Add two `IntervalValue`s. Rest + anything = Rest.
fn add_interval_values(a: &IntervalValue, b: &IntervalValue) -> IntervalValue {
    match (a.degree(), b.degree()) {
        (Some(da), Some(db)) => IntervalValue::Degree(da + db),
        _ => IntervalValue::Rest,
    }
}

/// Extract per-pattern source spans from a combined hap's context, pushing
/// into a flat arena. Returns nothing — caller diffs `arena.len()` before/after
/// to record (offset, length).
///
/// After a left-fold of N patterns via `app_left`, the merged `HapContext` has:
/// - `source_span` + `source_extra_spans` = pattern 0's spans
/// - `modifier_spans[i]` + `modifier_extra_spans[i]` = pattern (i+1)'s spans
fn extract_pattern_spans_into(
    context: &crate::pattern_system::HapContext,
    num_patterns: usize,
    arena: &mut Vec<FlatSpan>,
) {
    if num_patterns == 0 {
        return;
    }

    // Pattern 0: source_span + source_extra_spans
    for s in context.source_span.iter() {
        let (start, end) = s.to_tuple();
        arena.push(FlatSpan {
            pattern_idx: 0,
            start: start as u32,
            end: end as u32,
        });
    }
    for s in &context.source_extra_spans {
        let (start, end) = s.to_tuple();
        arena.push(FlatSpan {
            pattern_idx: 0,
            start: start as u32,
            end: end as u32,
        });
    }

    // Patterns 1..N: modifier_spans[i] + modifier_extra_spans[i]
    let modifier_limit = context
        .modifier_spans
        .len()
        .min(num_patterns.saturating_sub(1));
    for i in 0..modifier_limit {
        let (start, end) = context.modifier_spans[i].to_tuple();
        arena.push(FlatSpan {
            pattern_idx: (i + 1) as u8,
            start: start as u32,
            end: end as u32,
        });
        if let Some(extras) = context.modifier_extra_spans.get(i) {
            for s in extras {
                let (start, end) = s.to_tuple();
                arena.push(FlatSpan {
                    pattern_idx: (i + 1) as u8,
                    start: start as u32,
                    end: end as u32,
                });
            }
        }
    }
}

/// Arena-context variant of [`extract_pattern_spans_into`]. Walks the
/// tree-shaped [`crate::pattern_system::ArenaHapContext`] and emits one
/// `FlatSpan` per source span keyed by pattern index.
fn extract_pattern_spans_from_arena_into(
    context: &crate::pattern_system::ArenaHapContext<'_>,
    num_patterns: usize,
    arena: &mut Vec<FlatSpan>,
) {
    if num_patterns == 0 {
        return;
    }
    // An `Empty` context has no spans to walk; short-circuit.
    if matches!(context, crate::pattern_system::ArenaHapContext::Empty) {
        return;
    }
    let pattern_cap = num_patterns as u8;
    context.walk(&mut |pattern_idx, span| {
        if pattern_idx >= pattern_cap {
            return;
        }
        let (start, end) = span.to_tuple();
        arena.push(FlatSpan {
            pattern_idx,
            start: start as u32,
            end: end as u32,
        });
    });
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct IntervalSeqOutputs {
    #[output("cv", "pitch output in V/Oct (quantized to scale)", default)]
    cv: PolyOutput,
    #[output("gate", "high (5 V) while a note is active, low (0 V) otherwise", range = (0.0, 5.0))]
    gate: PolyOutput,
    #[output("trig", "short pulse (5 V) at the start of each note", range = (0.0, 5.0))]
    trig: PolyOutput,
}

const CAP: usize = 12;

#[allow(unused_imports)]
use super::cache::{
    MAX_MODULE_CYCLES, MIN_HAPS_CAP_HINT, MIN_SPANS_CAP_HINT, PARAM_CACHE_CYCLES,
    SPANS_RESERVE_PER_HAP,
};

/// Scale-degree sequencer using a compact text syntax ported
/// from TidalCycles/Strudel.
///
/// Works with **scale degree numbers** instead of note names. One or more
/// **patterns** are combined by recursively folding the patterns into each other.
/// This is adapted from the default way that patterns are combined in Strudel:
/// 2 patterns are aligned in a cycle and the events of the second pattern are applied to the first.
/// Here this happens recursively (where n pattern is applied to n-1), adding
/// the values of those patterns' events together. The result is a single combined
/// pattern of scale degrees that can be sampled at the current playhead position to produce output CV/gate/trig.
/// Scale degrees outside the configured **scale** are automatically wrapped into the appropriate octave.
///
/// ## Cycles
///
/// A **cycle** is one full traversal of a pattern. The playhead position
/// determines timing: its integer part selects the current cycle number and
/// the fractional part selects the position within that cycle.
/// All patterns share the same cycle clock.
///
/// ## Scale degrees
///
/// Values are **0-indexed** degrees of the chosen scale. `0` is the root,
/// `1` is the second scale tone, `2` the third, and so on. Negative values
/// move downward; values beyond the scale length wrap into higher/lower
/// octaves automatically.
///
/// ## Mini-notation
///
/// | Syntax | Meaning | Example |
/// |--------|---------|---------|
/// | Bare number | Scale degree (0-indexed) | `0`, `2`, `4` |
/// | `~` | Rest (gate low, no change in pitch) | `'0 ~ 2 ~'` |
/// | `[a b c]` | Fast subsequence — subdivides parent time slot | `'[0 2 4]'` |
/// | `<a b c>` | Slow / alternating — one element per cycle | `'<0 4 7>'` |
/// | `a\|b\|c` | Random choice each time the slot is reached | `'0\|2\|4'` |
/// | `a, b` | Stack — comma-separated patterns play simultaneously | `'0 2, 4 7'` |
///
/// Grouping, stacks, and random choice nest arbitrarily.
///
/// ## Per-element modifiers
///
/// Modifiers attach directly to an element (no spaces). Multiple modifiers
/// can be chained in any order.
///
/// | Modifier | Syntax | Meaning |
/// |----------|--------|---------|
/// | Weight | `@n` | Relative duration within a sequence (default 1). `0@2 2` gives `0` twice the time. |
/// | Speed up | `*n` | Repeat/subdivide `n` times within the slot. `0*3` plays degree 0 three times. |
/// | Slow down | `/n` | Stretch over `n` cycles. `0/2` plays every other cycle. |
/// | Replicate | `!n` | Duplicate the element `n` times (default 2). `0!3` is equivalent to `0 0 0`. |
/// | Degrade | `?` or `?n` | Randomly drop the element. `0?` drops ~50 % of the time; `0?0.8` drops 80 %. |
/// | Euclidean | `(k,n)` or `(k,n,offset)` | Distribute `k` pulses over `n` steps (Bjorklund algorithm). |
///
/// Modifier operands can also be subpatterns: `0*[2 3]` alternates between
/// doubling and tripling each slot.
///
/// ## Polyphony
///
/// The first pattern's structure is preserved. When subsequent patterns
/// contain stacks (simultaneous events), one combined
/// event is created per left×right pair, all sharing the first pattern's timing. This
/// can create polyphonic output.
///
/// ```js
/// // first pattern: one note per slot
/// // second pattern: two simultaneous offsets → two voices per slot
/// $iCycle(["0 2 4", "0,4"], "c4(major)")
/// ```
///
/// ```js
/// // slow alternation in second pattern shifts the chord each cycle
/// $iCycle(["0,2,4", "<0 3>"], "c4(major)")
/// ```
///
/// ## Outputs
///
/// - **cv** — V/Oct pitch quantized to the scale (C4 = 0 V).
/// - **gate** — 5 V while a note is active, 0 V otherwise.
/// - **trig** — single-sample 5 V pulse at each note onset.
#[module(
    name = "$iCycle",
    channels_derive = interval_seq_derive_channel_count,
    args(patterns, scale),
    stateful,
    patch_update,
)]

pub struct IntervalSeq {
    outputs: IntervalSeqOutputs,
    params: IntervalSeqParams,
    state: IntervalSeqState,
}

/// State for the IntervalSeq module.
struct IntervalSeqState {
    /// Per-voice state array
    voices: [IntervalVoiceState; PORT_MAX_CHANNELS],
    /// Round-robin voice index for allocation
    next_voice: usize,
    /// Current cycle number
    current_cycle: Option<i64>,
    /// Module-level cache for cycles >= PARAM_CACHE_CYCLES.
    /// Always sized to MAX_MODULE_CYCLES so the audio thread never resizes
    /// the outer Vec. Each slot's inner Vecs are pre-sized to the
    /// `max_haps_per_cycle`/`max_spans_per_cycle` hints from the param.
    module_cache: Vec<CycleStorage>,
    /// Populated flag per module_cache slot.
    module_cache_populated: Vec<bool>,
    /// Cached scale intervals for degree-to-semitone conversion (no audio-thread allocs)
    scale_intervals: ArrayVec<i8, CAP>,
    /// Base MIDI note for degree 0 (includes root pitch class + octave)
    base_midi: i32,
    /// Cached tuning table: V/Oct offset of each chromatic step above the root.
    /// 12-TET by default; non-equal for just / Pythagorean scales.
    tuning: [f64; 12],
    /// Last CV voltage per channel — holds through rest periods and state transfers
    last_cv: [f32; PORT_MAX_CHANNELS],
    /// Scratch buffer for onset events. Holds scalars copied out of the
    /// cycle storage so voice allocation runs without borrowing the cache.
    events_to_process: ArrayVec<PendingEvent, PORT_MAX_CHANNELS>,
    /// Bumpalo arena reused across `ensure_cycle_cached` calls. Reset
    /// before each miss-path query so the pattern_system combinator chain
    /// allocates intermediates from a single chunk.
    query_arena: bumpalo::Bump,
}

/// Onset event awaiting voice allocation. All scalars — no heap reference.
#[derive(Clone, Copy, Debug)]
struct PendingEvent {
    hap_index: u32,
    degree: i32,
    whole_begin: f64,
    whole_end: f64,
}

impl Default for IntervalSeqState {
    fn default() -> Self {
        Self {
            voices: std::array::from_fn(|_| IntervalVoiceState::default()),
            next_voice: 0,
            current_cycle: None,
            module_cache: Vec::new(),
            module_cache_populated: Vec::new(),
            scale_intervals: [0, 2, 4, 5, 7, 9, 11].into_iter().collect(), // Default major scale
            base_midi: 60,                                                 // C4
            tuning: std::array::from_fn(|i| i as f64 / 12.0),              // 12-TET
            last_cv: [0.0; PORT_MAX_CHANNELS],
            events_to_process: ArrayVec::new(),
            query_arena: bumpalo::Bump::new(),
        }
    }
}

/// A combined hap from the folded pattern, ready for voice allocation.
/// Pattern spans live in a parallel arena (`CycleStorage::span_arena`),
/// addressed by `span_offset`/`span_len`.
#[derive(Clone, Debug, Default)]
pub(crate) struct CombinedHap {
    pub whole_begin: f64,
    pub whole_end: f64,
    pub part_begin: f64,
    pub part_end: f64,
    /// Combined degree, None if rest
    pub degree: Option<i32>,
    pub has_onset: bool,
    /// Range into the owning `CycleStorage::span_arena`.
    pub span_offset: u32,
    pub span_len: u32,
}

impl IntervalSeq {
    /// Invalidate the cycle cache. Keeps allocated Vec capacities so the
    /// audio thread can re-fill without reallocation.
    ///
    /// Voices are left untouched so any sounding note can still be released
    /// by its `whole_end` after a patch update.
    fn invalidate_cache(&mut self) {
        self.state.current_cycle = None;
        super::cache::invalidate_module_cache(
            &mut self.state.module_cache,
            &mut self.state.module_cache_populated,
        );
    }

    /// Resize the module_cache to MAX_MODULE_CYCLES with each slot pre-allocated
    /// to the param's capacity hints. Called on patch update from the main thread.
    fn rebuild_module_cache(&mut self) {
        super::cache::rebuild_module_cache(
            &mut self.state.module_cache,
            &mut self.state.module_cache_populated,
            self.params.patterns.max_haps_per_cycle(),
            self.params.patterns.max_spans_per_cycle(),
        );
    }

    /// Ensure that the given cycle's haps are available in either the
    /// param cache or the module cache. Audio-thread entry point — uses
    /// pre-sized capacities so the common path needs no heap allocation.
    fn ensure_cycle_cached(&mut self, cycle: i64) {
        if cycle < PARAM_CACHE_CYCLES as i64 {
            return; // Already in param cache
        }

        let module_idx = (cycle - PARAM_CACHE_CYCLES as i64) as usize;
        if module_idx >= self.state.module_cache.len() {
            return; // Beyond cache horizon — caller will re-query each frame
        }
        if self.state.module_cache_populated[module_idx] {
            return;
        }

        let Some(pattern) = self.params.patterns.pattern() else {
            return;
        };
        let num_patterns = self.params.patterns.num_sources();
        let slot = &mut self.state.module_cache[module_idx];
        super::cache::populate_cycle_storage(
            pattern,
            cycle,
            &mut self.state.query_arena,
            slot,
            |hap, haps, span_arena| {
                let span_offset = span_arena.len() as u32;
                extract_pattern_spans_from_arena_into(&hap.context, num_patterns, span_arena);
                let span_len = span_arena.len() as u32 - span_offset;
                haps.push(CombinedHap {
                    whole_begin: hap.whole_begin_f64(),
                    whole_end: hap.whole_end_f64(),
                    part_begin: hap.part_begin_f64(),
                    part_end: hap.part_end_f64(),
                    degree: hap.value.degree(),
                    has_onset: hap.has_onset(),
                    span_offset,
                    span_len,
                });
            },
        );
        self.state.module_cache_populated[module_idx] = true;
    }

    /// Look up the storage for `cycle` from param cache or module cache.
    /// Returns None for cycles past the cache horizon.
    fn get_cycle_storage(&self, cycle: i64) -> Option<&CycleStorage> {
        super::cache::get_cycle_storage(
            cycle,
            self.params.patterns.cached_haps(),
            &self.state.module_cache,
            &self.state.module_cache_populated,
        )
    }

    /// Convert a scale degree to V/Oct voltage.
    fn degree_to_voltage(&self, degree: i32) -> f64 {
        if self.state.scale_intervals.is_empty() {
            // Chromatic fallback
            return midi_to_voct_f64(60.0 + degree as f64);
        }

        let scale_len = self.state.scale_intervals.len() as i32;

        // Handle negative degrees with proper wrapping
        let (octave, wrapped_degree) = if degree >= 0 {
            (degree / scale_len, (degree % scale_len) as usize)
        } else {
            // For negative: -1 in 7-note scale is degree 6 in octave -1
            let adj_degree = degree + 1;
            let octave = (adj_degree / scale_len) - 1;
            let wrapped = ((degree % scale_len) + scale_len) % scale_len;
            (octave, wrapped as usize)
        };

        // Get semitone offset within octave from scale intervals
        let semitone_in_scale = self
            .state
            .scale_intervals
            .get(wrapped_degree)
            .copied()
            .unwrap_or(0) as i32;

        // Voltage = root + degree octave + the tuning table's offset for this step.
        // Under 12-TET this is identical to midi_to_voct_f64(base_midi + octave*12 + step).
        // `semitone_in_scale` is always 0..11 (normalized scale intervals); `get`
        // keeps a stray value from panicking on the audio thread.
        let root_v = (self.state.base_midi - 60) as f64 / 12.0;
        let step_v = self
            .state
            .tuning
            .get(semitone_in_scale as usize)
            .copied()
            .unwrap_or(0.0);
        root_v + octave as f64 + step_v
    }

    /// Update cached scale info from params.
    fn update_scale_cache(&mut self) {
        let scale = &self.params.scale;
        self.state.base_midi = scale.base_midi();
        if let Some(snapper) = scale.snapper() {
            self.state.scale_intervals = snapper.scale_intervals().clone();
            self.state.tuning = *snapper.tuning();
        } else {
            // Chromatic - all 12 semitones, 12-TET
            self.state.scale_intervals = (0i8..CAP as i8).into_iter().collect();
            self.state.tuning = std::array::from_fn(|i| i as f64 / 12.0);
        }
    }
}

impl IntervalSeq {
    fn update(&mut self, sample_rate: f32) {
        let playhead = self.params.playhead.value_or_zero() as f64;
        let hold = min_gate_samples(sample_rate);
        let num_channels = self.channel_count();

        // Release voices whose haps have ended
        self.release_ended_voices(playhead, num_channels);

        // Check if we have a combined pattern
        if self.params.patterns.pattern().is_none() {
            for ch in 0..num_channels {
                self.outputs.cv.set(ch, 0.0);
                self.outputs
                    .gate
                    .set(ch, self.state.voices[ch].gate.process());
                self.outputs
                    .trig
                    .set(ch, self.state.voices[ch].trigger.process());
            }
            return;
        }

        // Check if we crossed a cycle boundary
        let current_cycle = playhead.floor() as i64;
        if self.state.current_cycle != Some(current_cycle) {
            self.ensure_cycle_cached(current_cycle);
            self.state.current_cycle = Some(current_cycle);
        }

        // Collect events to process. Split-borrow self.state so we can hold a
        // &CycleStorage from module_cache while pushing into events_to_process
        // (different fields, no aliasing).
        {
            let IntervalSeqState {
                module_cache,
                module_cache_populated,
                voices,
                events_to_process,
                ..
            } = &mut self.state;
            let storage: Option<&CycleStorage> = if current_cycle < PARAM_CACHE_CYCLES as i64 {
                self.params.patterns.cached_haps().get(current_cycle as usize)
            } else {
                let idx = (current_cycle - PARAM_CACHE_CYCLES as i64) as usize;
                if idx < module_cache.len() && module_cache_populated[idx] {
                    Some(&module_cache[idx])
                } else {
                    None
                }
            };
            events_to_process.clear();
            if let Some(storage) = storage {
                for (hap_index, combined) in storage.haps.iter().enumerate() {
                    if !combined.has_onset {
                        continue;
                    }
                    if playhead < combined.part_begin || playhead >= combined.part_end {
                        continue;
                    }
                    let Some(degree) = combined.degree else {
                        continue;
                    };
                    let hap_index_u32 = hap_index as u32;
                    let already_assigned = voices[..num_channels].iter().any(|v| {
                        v.cached_hap.is_some_and(|c| {
                            c.hap_index == hap_index_u32 && c.cached_cycle == current_cycle
                        })
                    });
                    if already_assigned {
                        continue;
                    }
                    if events_to_process.remaining_capacity() == 0 {
                        break;
                    }
                    events_to_process.push(PendingEvent {
                        hap_index: hap_index_u32,
                        degree,
                        whole_begin: combined.whole_begin,
                        whole_end: combined.whole_end,
                    });
                }
            }
        }

        // Process collected events
        for idx in 0..self.state.events_to_process.len() {
            let event = self.state.events_to_process[idx];
            let Some(voice_idx) = self.allocate_voice(playhead, num_channels) else {
                continue;
            };
            let voltage = self.degree_to_voltage(event.degree);

            let voice = &mut self.state.voices[voice_idx];
            voice.cached_hap = Some(CachedIntervalHap {
                hap_index: event.hap_index,
                cached_cycle: current_cycle,
                whole_begin: event.whole_begin,
                whole_end: event.whole_end,
            });
            voice.cached_voltage = voltage;
            voice.active = true;
            voice
                .gate
                .set_state(TempGateState::Low, TempGateState::High, hold);
            voice
                .trigger
                .set_state(TempGateState::High, TempGateState::Low, hold);
        }

        // Output all voices
        for ch in 0..num_channels {
            let voice = &mut self.state.voices[ch];

            if voice.active {
                self.state.last_cv[ch] = voice.cached_voltage as f32;
            }
            self.outputs.cv.set(ch, self.state.last_cv[ch]);

            self.outputs.gate.set(ch, voice.gate.process());
            self.outputs.trig.set(ch, voice.trigger.process());
        }
    }

    fn allocate_voice(&mut self, playhead: f64, num_channels: usize) -> Option<usize> {
        for i in 0..num_channels {
            let voice_idx = (self.state.next_voice + i) % num_channels;
            if !self.state.voices[voice_idx].active {
                self.state.next_voice = (voice_idx + 1) % num_channels;
                self.state.voices[voice_idx].last_assigned = playhead;
                return Some(voice_idx);
            }
        }

        // All voices occupied — skip this event rather than stealing
        None
    }

    fn release_ended_voices(&mut self, playhead: f64, num_channels: usize) {
        for i in 0..num_channels {
            if let Some(cached) = self.state.voices[i].cached_hap {
                if !cached.contains(playhead) {
                    self.state.voices[i].active = false;
                    self.state.voices[i].cached_hap = None;
                    self.state.voices[i]
                        .gate
                        .set_state(TempGateState::Low, TempGateState::Low, 0);
                }
            }
        }
    }
}

impl crate::types::StatefulModule for IntervalSeq {
    fn get_state(&self) -> Option<serde_json::Value> {
        let num_channels = self.channel_count().clamp(1, PORT_MAX_CHANNELS);
        let per_source = self.params.patterns.per_source();
        let num_sources = per_source.len();

        // Collect per-pattern active spans from all active voices
        let mut per_pattern_spans: Vec<Vec<(usize, usize)>> = vec![Vec::new(); num_sources];
        let mut any_active = false;

        for voice in self.state.voices.iter().take(num_channels) {
            if !voice.active {
                continue;
            }
            let Some(cached) = voice.cached_hap else {
                continue;
            };
            let Some(storage) = self.get_cycle_storage(cached.cached_cycle) else {
                continue;
            };
            let Some(hap) = storage.haps.get(cached.hap_index as usize) else {
                continue;
            };
            any_active = true;
            let start = hap.span_offset as usize;
            let end = start + hap.span_len as usize;
            for span in &storage.span_arena[start..end] {
                let idx = span.pattern_idx as usize;
                if idx < num_sources {
                    per_pattern_spans[idx].push((span.start as usize, span.end as usize));
                }
            }
        }

        if !any_active {
            None
        } else {
            // Deduplicate spans per pattern
            for spans in &mut per_pattern_spans {
                spans.sort();
                spans.dedup();
            }

            // Build param_spans map keyed by "patterns.0", "patterns.1", etc.
            // When the source is an array (Multiple), always use indexed keys
            // even for a single element, to match the argument span analyzer
            // which registers array elements as "patterns.0", "patterns.1", etc.
            let is_array = self.params.patterns.is_array_source();
            let mut param_spans = serde_json::Map::new();
            for (i, meta) in per_source.iter().enumerate() {
                let key = if !is_array && num_sources == 1 {
                    "patterns".to_string()
                } else {
                    format!("patterns.{}", i)
                };
                param_spans.insert(
                    key,
                    serde_json::json!({
                        "spans": per_pattern_spans.get(i).unwrap_or(&Vec::new()),
                        "source": meta.source,
                        "all_spans": meta.all_spans,
                    }),
                );
            }

            Some(serde_json::json!({
                "param_spans": param_spans,
                "num_channels": num_channels,
            }))
        }
    }
}

impl crate::types::PatchUpdateHandler for IntervalSeq {
    fn on_patch_update(&mut self) {
        self.invalidate_cache();
        self.rebuild_module_cache();
        self.update_scale_cache();
        // Combined pattern is already built at parse time inside IntervalPatternParam
    }
}

message_handlers!(impl IntervalSeq {});

#[cfg(test)]
impl Default for IntervalSeq {
    fn default() -> Self {
        Self {
            outputs: IntervalSeqOutputs::default(),
            state: IntervalSeqState::default(),
            params: IntervalSeqParams::default(),
            _channel_count: 4,
            _block_index: Default::default(),
        }
    }
}

#[cfg(test)]
impl Default for IntervalSeqParams {
    fn default() -> Self {
        Self {
            patterns: IntervalPatternParam::default(),
            scale: ScaleParam::parse("C(major)").unwrap(),
            playhead: None,
            channels: default_channels(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interval_value_from_atom() {
        use crate::pattern_system::mini::ast::AtomValue;
        use crate::pattern_system::mini::convert::FromMiniAtom;

        let v = IntervalValue::from_atom(&AtomValue::Number(5.0)).unwrap();
        assert!(matches!(v, IntervalValue::Degree(5)));

        // Non-integer numbers rejected.
        assert!(IntervalValue::from_atom(&AtomValue::Number(1.5)).is_err());

        // Hz / Note rejected — $iCycle only accepts integer scale degrees.
        assert!(IntervalValue::from_atom(&AtomValue::Hz(440.0)).is_err());
    }

    #[test]
    fn test_from_source_single_string() {
        let param =
            IntervalPatternParam::from_source(IntervalPatternSource::Single("0 1 2 3".into()))
                .unwrap();
        assert!(param.pattern().is_some());
        assert_eq!(param.num_sources(), 1);
        assert_eq!(param.per_source().len(), 1);
        assert_eq!(param.per_source()[0].source, "0 1 2 3");
    }

    #[test]
    fn test_from_source_empty_string() {
        let param =
            IntervalPatternParam::from_source(IntervalPatternSource::Single("".into())).unwrap();
        assert!(param.pattern().is_none());
        assert_eq!(param.num_sources(), 1);
    }

    #[test]
    fn test_from_source_multiple() {
        let param = IntervalPatternParam::from_source(IntervalPatternSource::Multiple(vec![
            "0 2 4".into(),
            "1".into(),
        ]))
        .unwrap();
        assert!(param.pattern().is_some());
        assert_eq!(param.num_sources(), 2);
        assert_eq!(param.per_source().len(), 2);
        assert_eq!(param.per_source()[0].source, "0 2 4");
        assert_eq!(param.per_source()[1].source, "1");

        // Combined: 0+1=1, 2+1=3, 4+1=5
        let combined = param.pattern().unwrap();
        let haps = combined.query_cycle_all(0);
        let onsets: Vec<_> = haps.iter().filter(|h| h.has_onset()).collect();
        assert_eq!(onsets.len(), 3);

        let mut degrees: Vec<i32> = onsets.iter().filter_map(|h| h.value.degree()).collect();
        degrees.sort();
        assert_eq!(degrees, vec![1, 3, 5]);
    }

    #[test]
    fn test_from_source_three_patterns() {
        let param = IntervalPatternParam::from_source(IntervalPatternSource::Multiple(vec![
            "0 2".into(),
            "1".into(),
            "10".into(),
        ]))
        .unwrap();

        let combined = param.pattern().unwrap();
        let haps = combined.query_cycle_all(0);
        let onsets: Vec<_> = haps.iter().filter(|h| h.has_onset()).collect();
        assert_eq!(onsets.len(), 2);

        let mut degrees: Vec<i32> = onsets.iter().filter_map(|h| h.value.degree()).collect();
        degrees.sort();
        // 0+1+10=11, 2+1+10=13
        assert_eq!(degrees, vec![11, 13]);
    }

    #[test]
    fn test_from_source_polyphony_via_stack() {
        // First pattern: 1 event per cycle
        // Second pattern: stack with 2 simultaneous events
        // app_left should produce 2 output events (polyphony)
        let param = IntervalPatternParam::from_source(IntervalPatternSource::Multiple(vec![
            "0".into(),
            "0, 4".into(),
        ]))
        .unwrap();

        let combined = param.pattern().unwrap();
        let haps = combined.query_cycle_all(0);
        let onsets: Vec<_> = haps.iter().filter(|h| h.has_onset()).collect();
        assert_eq!(onsets.len(), 2);

        let mut degrees: Vec<i32> = onsets.iter().filter_map(|h| h.value.degree()).collect();
        degrees.sort();
        assert_eq!(degrees, vec![0, 4]);
    }

    #[test]
    fn test_degree_to_voltage_major() {
        let mut seq = IntervalSeq::default();
        seq.state.scale_intervals = [0, 2, 4, 5, 7, 9, 11].iter().copied().collect(); // C major
        seq.state.base_midi = 60; // C4

        // Degree 0 = C4 = MIDI 60 = 0V
        let v0 = seq.degree_to_voltage(0);
        assert!((v0 - 0.0).abs() < 0.001);

        // Degree 1 = D4 = MIDI 62 = 2/12 V
        let v1 = seq.degree_to_voltage(1);
        assert!((v1 - (2.0 / 12.0)).abs() < 0.001);

        // Degree 7 = C5 = MIDI 72 = 1V
        let v7 = seq.degree_to_voltage(7);
        assert!((v7 - 1.0).abs() < 0.001);

        // Degree -1 = B3 = MIDI 59 = -1/12 V
        let v_neg1 = seq.degree_to_voltage(-1);
        assert!((v_neg1 - (-1.0 / 12.0)).abs() < 0.001);
    }

    #[test]
    fn test_degree_to_voltage_with_octave() {
        let mut seq = IntervalSeq::default();
        seq.state.scale_intervals = [0, 2, 4, 5, 7, 9, 11].iter().copied().collect(); // C major
        seq.state.base_midi = 48; // C3

        // Degree 0 = C3 = MIDI 48 = -1V
        let v0 = seq.degree_to_voltage(0);
        assert!((v0 - (-1.0)).abs() < 0.001);

        // Degree 7 = C4 = MIDI 60 = 0V
        let v7 = seq.degree_to_voltage(7);
        assert!((v7 - 0.0).abs() < 0.001);

        // D3 root
        seq.state.base_midi = 50; // D3
        // Degree 0 = D3 = MIDI 50 = -10/12 V
        let v0 = seq.degree_to_voltage(0);
        assert!((v0 - (-10.0 / 12.0)).abs() < 0.001);
    }

    #[test]
    fn test_degree_to_voltage_just() {
        use crate::dsp::utilities::scale::named_tuning;

        let mut seq = IntervalSeq::default();
        // 12-tone just scale rooted at C4.
        seq.state.scale_intervals = (0i8..12).collect();
        seq.state.base_midi = 60;
        seq.state.tuning = named_tuning("just").unwrap();

        // Degree 0 = root = 0V
        assert!(seq.degree_to_voltage(0).abs() < 1e-9);
        // Degree 4 = just major third = 5/4
        assert!((seq.degree_to_voltage(4) - 1.25_f64.log2()).abs() < 1e-9);
        // Degree 7 = just fifth = 3/2
        assert!((seq.degree_to_voltage(7) - 1.5_f64.log2()).abs() < 1e-9);
        // Degree 12 = octave up = exactly +1V
        assert!((seq.degree_to_voltage(12) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_scale_param_with_octave() {
        use crate::dsp::utilities::quantizer::ScaleParam;

        let scale = ScaleParam::parse("C3(major)").unwrap();
        assert_eq!(scale.base_midi(), 48);
        assert!(scale.snapper().is_some());

        let scale = ScaleParam::parse("Db3(min)").unwrap();
        assert_eq!(scale.base_midi(), 49);
        assert!(scale.snapper().is_some());

        // Without octave defaults to octave 4
        let scale = ScaleParam::parse("C(major)").unwrap();
        assert_eq!(scale.base_midi(), 60);

        let scale = ScaleParam::parse("D(major)").unwrap();
        assert_eq!(scale.base_midi(), 62);
    }

    #[test]
    fn test_add_interval_values() {
        let a = IntervalValue::Degree(3);
        let b = IntervalValue::Degree(4);
        let result = add_interval_values(&a, &b);
        assert!(matches!(result, IntervalValue::Degree(7)));

        let result = add_interval_values(&IntervalValue::Rest, &IntervalValue::Degree(1));
        assert!(result.is_rest());

        let result = add_interval_values(&IntervalValue::Degree(1), &IntervalValue::Rest);
        assert!(result.is_rest());

        let result = add_interval_values(&IntervalValue::Rest, &IntervalValue::Rest);
        assert!(result.is_rest());
    }

    #[test]
    fn test_derive_combined_polyphony_single() {
        let param =
            IntervalPatternParam::from_source(IntervalPatternSource::Single("0 2 4".into()))
                .unwrap();
        let count = derive_combined_polyphony(&param);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_derive_combined_polyphony_with_stack() {
        let param = IntervalPatternParam::from_source(IntervalPatternSource::Multiple(vec![
            "0".into(),
            "0, 4".into(),
        ]))
        .unwrap();
        let count = derive_combined_polyphony(&param);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_deserialize_patterns_from_payload() {
        let payload = serde_json::to_value(
            crate::dsp::seq::seq_value::ParsedPatternPayload::parse_for_test("0 2 4"),
        )
        .unwrap();
        let json = serde_json::json!({ "patterns": payload, "scale": "c(major)" });
        let params: IntervalSeqParams =
            deserr::deserialize::<IntervalSeqParams, _, crate::param_errors::ModuleParamErrors>(
                json,
            )
            .unwrap();
        assert!(params.patterns.pattern().is_some());
        assert_eq!(params.patterns.num_sources(), 1);
    }

    #[test]
    fn test_deserialize_patterns_from_array() {
        let p1 = serde_json::to_value(
            crate::dsp::seq::seq_value::ParsedPatternPayload::parse_for_test("0 2 4"),
        )
        .unwrap();
        let p2 = serde_json::to_value(
            crate::dsp::seq::seq_value::ParsedPatternPayload::parse_for_test("0 3"),
        )
        .unwrap();
        let json = serde_json::json!({ "patterns": [p1, p2], "scale": "c(major)" });
        let params: IntervalSeqParams =
            deserr::deserialize::<IntervalSeqParams, _, crate::param_errors::ModuleParamErrors>(
                json,
            )
            .unwrap();
        assert!(params.patterns.pattern().is_some());
        assert_eq!(params.patterns.num_sources(), 2);
    }

    #[test]
    fn test_extract_pattern_spans_includes_extras() {
        use crate::pattern_system::{HapContext, SourceSpan};

        // Single pattern with source_extra_spans (from *<4 6>)
        let mut ctx = HapContext::with_span(SourceSpan::new(0, 1));
        ctx.source_extra_spans.push(SourceSpan::new(5, 6));

        let mut arena: Vec<FlatSpan> = Vec::new();
        extract_pattern_spans_into(&ctx, 1, &mut arena);
        assert_eq!(arena.len(), 2);
        assert_eq!(arena[0].pattern_idx, 0);
        assert_eq!((arena[0].start, arena[0].end), (0, 1));
        assert_eq!(arena[1].pattern_idx, 0);
        assert_eq!((arena[1].start, arena[1].end), (5, 6));

        // Two patterns: pattern 0 has extras, pattern 1 has extras
        let mut ctx2 = HapContext::with_span(SourceSpan::new(0, 1));
        ctx2.source_extra_spans.push(SourceSpan::new(5, 6));
        ctx2.modifier_spans.push(SourceSpan::new(10, 11));
        ctx2.modifier_extra_spans
            .push(vec![SourceSpan::new(15, 16)]);

        let mut arena2: Vec<FlatSpan> = Vec::new();
        extract_pattern_spans_into(&ctx2, 2, &mut arena2);
        assert_eq!(arena2.len(), 4);
        // Pattern 0
        assert_eq!(arena2[0].pattern_idx, 0);
        assert_eq!((arena2[0].start, arena2[0].end), (0, 1));
        assert_eq!(arena2[1].pattern_idx, 0);
        assert_eq!((arena2[1].start, arena2[1].end), (5, 6));
        // Pattern 1
        assert_eq!(arena2[2].pattern_idx, 1);
        assert_eq!((arena2[2].start, arena2[2].end), (10, 11));
        assert_eq!(arena2[3].pattern_idx, 1);
        assert_eq!((arena2[3].start, arena2[3].end), (15, 16));
    }

    /// Multi-pattern highlighting must bucket flat spans by `pattern_idx`
    /// correctly. Verifies that span arena entries for a 2-pattern source
    /// contain both pattern_idx 0 (from pattern 0 source leaves) and
    /// pattern_idx 1 (from pattern 1's modifier_spans).
    #[test]
    fn test_multi_pattern_highlighting() {
        use crate::types::StatefulModule;

        // Source positions for "0 2 4" are 0..1, 2..3, 4..5.
        // Source positions for "1 3 5" are 0..1, 2..3, 4..5 (own string).
        let param = IntervalPatternParam::from_source(IntervalPatternSource::Multiple(vec![
            "0 2 4".into(),
            "1 3 5".into(),
        ]))
        .unwrap();

        // Cached cycle 0 storage must hold spans from both patterns,
        // tagged with the right pattern_idx.
        let storage = &param.cached_haps()[0];
        assert!(!storage.haps.is_empty(), "expected onset haps in cycle 0");

        let mut saw_p0 = false;
        let mut saw_p1 = false;
        for hap in &storage.haps {
            let start = hap.span_offset as usize;
            let end = start + hap.span_len as usize;
            for span in &storage.span_arena[start..end] {
                match span.pattern_idx {
                    0 => saw_p0 = true,
                    1 => saw_p1 = true,
                    other => panic!("unexpected pattern_idx {other}"),
                }
            }
        }
        assert!(saw_p0, "expected pattern_idx 0 span (pattern 0 source leaf)");
        assert!(saw_p1, "expected pattern_idx 1 span (pattern 1 modifier)");

        // End-to-end: simulate an active voice referencing the first onset
        // hap and check get_state buckets spans into "patterns.0" / "patterns.1".
        let mut seq = IntervalSeq::default();
        seq.params.patterns = param;
        seq.rebuild_module_cache();

        // Find first onset hap in cycle 0 storage.
        let storage = &seq.params.patterns.cached_haps()[0];
        let (onset_idx, onset_hap) = storage
            .haps
            .iter()
            .enumerate()
            .find(|(_, h)| h.has_onset && h.degree.is_some())
            .expect("at least one onset hap");
        let whole_begin = onset_hap.whole_begin;
        let whole_end = onset_hap.whole_end;

        seq.state.voices[0].active = true;
        seq.state.voices[0].cached_hap = Some(CachedIntervalHap {
            hap_index: onset_idx as u32,
            cached_cycle: 0,
            whole_begin,
            whole_end,
        });

        let json = seq.get_state().expect("expected state with active voice");
        let param_spans = json
            .get("param_spans")
            .and_then(|v| v.as_object())
            .expect("param_spans map");
        // Array source → indexed keys "patterns.0", "patterns.1".
        assert!(
            param_spans.contains_key("patterns.0"),
            "missing patterns.0 key: {param_spans:?}"
        );
        assert!(
            param_spans.contains_key("patterns.1"),
            "missing patterns.1 key: {param_spans:?}"
        );
        // Each entry must have a non-empty spans array (the active voice's
        // hap contributes leaves from both patterns).
        for key in ["patterns.0", "patterns.1"] {
            let spans = param_spans[key]
                .get("spans")
                .and_then(|v| v.as_array())
                .unwrap_or_else(|| panic!("{key} missing spans array"));
            assert!(!spans.is_empty(), "{key} spans empty");
        }
    }

    /// Parse-time cost: how long does `IntervalPatternParam::from_source`
    /// take for various patterns? Drives the choice of `PARAM_CACHE_CYCLES`.
    #[test]
    #[ignore]
    fn bench_parse_time() {
        use std::time::Instant;

        let cases: &[(&str, IntervalPatternSource)] = &[
            (
                "simple 4-note",
                IntervalPatternSource::Single("0 2 4 5".into()),
            ),
            (
                "euclidean",
                IntervalPatternSource::Single("0(5,8) 2(3,8) 4(7,16)".into()),
            ),
            (
                "3-pattern fold",
                IntervalPatternSource::Multiple(vec![
                    "0 2 4 5 7".into(),
                    "<0 3 5>".into(),
                    "0(3,8)".into(),
                ]),
            ),
            (
                "stack polyphony",
                IntervalPatternSource::Multiple(vec!["0 2 4".into(), "0,4,7".into()]),
            ),
        ];

        const RUNS: usize = 5;

        for (label, source) in cases {
            let mut total = std::time::Duration::ZERO;
            for _ in 0..RUNS {
                let start = Instant::now();
                let _ = std::hint::black_box(
                    IntervalPatternParam::from_source(source.clone()).unwrap(),
                );
                total += start.elapsed();
            }
            let avg = total / RUNS as u32;
            println!(
                "{:<20} avg_parse={:>8.2} ms  (PARAM_CACHE_CYCLES={})",
                label,
                avg.as_secs_f64() * 1000.0,
                PARAM_CACHE_CYCLES,
            );
        }
    }

    /// Arena-direct query measurement: just the pattern.query_cycle_all_into
    /// call timing in isolation. Excludes IntervalSeq's post-processing.
    #[test]
    #[ignore]
    fn bench_arena_query_only() {
        use std::time::Instant;

        let cases: &[(&str, IntervalPatternSource)] = &[
            ("simple 4-note", IntervalPatternSource::Single("0 2 4 5".into())),
            (
                "euclidean",
                IntervalPatternSource::Single("0(5,8) 2(3,8) 4(7,16)".into()),
            ),
            (
                "3-pattern fold",
                IntervalPatternSource::Multiple(vec![
                    "0 2 4 5 7".into(),
                    "<0 3 5>".into(),
                    "0(3,8)".into(),
                ]),
            ),
        ];

        const ITERS: usize = 2000;

        for (label, source) in cases {
            let param = IntervalPatternParam::from_source(source.clone()).unwrap();
            let pattern = param.pattern().unwrap();
            let mut bump = bumpalo::Bump::new();
            // Warm-up
            for _ in 0..16 {
                bump.reset();
                let mut buf: bumpalo::collections::Vec<
                    '_,
                    crate::pattern_system::ArenaHap<'_, IntervalValue>,
                > = bumpalo::collections::Vec::new_in(&bump);
                pattern.query_cycle_all_into(1, &bump, &mut buf);
            }
            let start = Instant::now();
            for i in 0..ITERS {
                bump.reset();
                let mut buf: bumpalo::collections::Vec<
                    '_,
                    crate::pattern_system::ArenaHap<'_, IntervalValue>,
                > = bumpalo::collections::Vec::new_in(&bump);
                pattern.query_cycle_all_into(1 + i as i64, &bump, &mut buf);
                std::hint::black_box(&buf);
            }
            let ns_per_call = start.elapsed().as_nanos() as f64 / ITERS as f64;
            println!("{:<20} arena_query={:>8.1} ns", label, ns_per_call);
        }
    }

    /// Breakdown bench: how much of `ensure_cycle_cached` is the pattern
    /// `query_cycle_all` call vs. our local post-processing.
    #[test]
    #[ignore]
    fn bench_miss_breakdown() {
        use std::time::Instant;

        let cases: &[(&str, IntervalPatternSource)] = &[
            ("simple 4-note", IntervalPatternSource::Single("0 2 4 5".into())),
            (
                "euclidean",
                IntervalPatternSource::Single("0(5,8) 2(3,8) 4(7,16)".into()),
            ),
            (
                "3-pattern fold",
                IntervalPatternSource::Multiple(vec![
                    "0 2 4 5 7".into(),
                    "<0 3 5>".into(),
                    "0(3,8)".into(),
                ]),
            ),
        ];

        const ITERS: usize = 2000;

        for (label, source) in cases {
            let param = IntervalPatternParam::from_source(source.clone()).unwrap();
            let pattern = param.pattern().unwrap();
            let num_patterns = param.num_sources();

            // A) query_cycle_all only.
            let start = Instant::now();
            for i in 0..ITERS {
                let _ = std::hint::black_box(pattern.query_cycle_all(1 + i as i64));
            }
            let query_ns = start.elapsed().as_nanos() as f64 / ITERS as f64;

            // B) query + extract/fill into a fresh storage (worst case).
            let start = Instant::now();
            for i in 0..ITERS {
                let haps = pattern.query_cycle_all(1 + i as i64);
                let mut storage = CycleStorage::with_capacity(
                    param.max_haps_per_cycle(),
                    param.max_spans_per_cycle(),
                );
                for hap in &haps {
                    let off = storage.span_arena.len() as u32;
                    extract_pattern_spans_into(&hap.context, num_patterns, &mut storage.span_arena);
                    let len = storage.span_arena.len() as u32 - off;
                    storage.haps.push(CombinedHap {
                        whole_begin: hap.whole_begin,
                        whole_end: hap.whole_end,
                        part_begin: hap.part_begin,
                        part_end: hap.part_end,
                        degree: hap.value.degree(),
                        has_onset: hap.has_onset(),
                        span_offset: off,
                        span_len: len,
                    });
                }
                std::hint::black_box(storage);
            }
            let full_ns = start.elapsed().as_nanos() as f64 / ITERS as f64;

            // C) query + fill into a *reused* pre-allocated storage (our path).
            let mut reuse_storage = CycleStorage::with_capacity(
                param.max_haps_per_cycle(),
                param.max_spans_per_cycle(),
            );
            let start = Instant::now();
            for i in 0..ITERS {
                let haps = pattern.query_cycle_all(1 + i as i64);
                reuse_storage.reset();
                for hap in &haps {
                    let off = reuse_storage.span_arena.len() as u32;
                    extract_pattern_spans_into(
                        &hap.context,
                        num_patterns,
                        &mut reuse_storage.span_arena,
                    );
                    let len = reuse_storage.span_arena.len() as u32 - off;
                    reuse_storage.haps.push(CombinedHap {
                        whole_begin: hap.whole_begin,
                        whole_end: hap.whole_end,
                        part_begin: hap.part_begin,
                        part_end: hap.part_end,
                        degree: hap.value.degree(),
                        has_onset: hap.has_onset(),
                        span_offset: off,
                        span_len: len,
                    });
                }
                std::hint::black_box(&reuse_storage);
            }
            let reuse_ns = start.elapsed().as_nanos() as f64 / ITERS as f64;

            println!(
                "{:<20} query={:>8.1} ns  full={:>8.1} ns  reuse={:>8.1} ns  post-only={:>7.1} ns",
                label,
                query_ns,
                full_ns,
                reuse_ns,
                reuse_ns - query_ns
            );
        }
    }

    /// Correctness check: dump every hap from every benched pattern across
    /// the first 16 cycles. Run on baseline + post-refactor and diff the
    /// outputs — must match exactly.
    /// Run: `cargo test -p modular_core --release dump_pattern_haps -- --ignored --nocapture > /tmp/haps.txt`
    #[test]
    #[ignore]
    fn dump_pattern_haps() {
        let cases: &[(&str, IntervalPatternSource)] = &[
            ("simple_4_note", IntervalPatternSource::Single("0 2 4 5".into())),
            ("euclidean", IntervalPatternSource::Single("0(5,8) 2(3,8) 4(7,16)".into())),
            ("3_pattern_fold", IntervalPatternSource::Multiple(vec!["0 2 4 5 7".into(), "<0 3 5>".into(), "0(3,8)".into()])),
            ("stack_polyphony", IntervalPatternSource::Multiple(vec!["0 2 4".into(), "0,4,7".into()])),
            ("stack_slowcat", IntervalPatternSource::Multiple(vec!["<0 2> 4".into(), "-7, <0!6 1>".into()])),
            ("stack_only", IntervalPatternSource::Single("0, < 4!2 5 >".into())),
            ("fast_stack", IntervalPatternSource::Multiple(vec!["<0 0 0 0 [3 2] 4 0 2>*8".into(), "0, <4 ~>*6".into()])),
            ("fast_stack_2", IntervalPatternSource::Multiple(vec!["<0 0 [3 2] 0 [4 0] 2>*8".into(), "0, <4 ~>*6".into()])),
            ("stack_euclid", IntervalPatternSource::Multiple(vec!["<0 0 [3 2] 0 [4 0] 2>*8".into(), "0, <4 ~>*6, <2(2,5) ~!2>*8".into()])),
            ("complex", IntervalPatternSource::Multiple(vec!["<[0 -1] 0 [3 2] 0 [4 0] 2>*8".into(), "-7, 0, <4 ~>*6, <2(2,5) ~!2>*8".into()])),
            ("bass2", IntervalPatternSource::Multiple(vec!["<[0,-7,4, <6 7 8>] ~>*16".into(), "[0@7 -2@4 -3@5]/2".into()])),
            ("nest_single1", IntervalPatternSource::Single("<0 <[4 2] 3> [1 3]>*4".into())),
            ("speed_subseq", IntervalPatternSource::Single("0*[8 <6 4>]".into())),
            ("dminor_duo", IntervalPatternSource::Multiple(vec!["<0 <-1 2>>*4".into(), "<0@4 2@2 5@2>".into()])),
            ("speed_seq", IntervalPatternSource::Single("0*[8 8 12 8]".into())),
            ("bass_long", IntervalPatternSource::Single(
                "<0 0 2 2 3 4 0 0 2 2 3 4 0 0 2 2 3 4 0 0 2 2 3 [4 -1]>".into(),
            )),
            ("wale_long", IntervalPatternSource::Single(
                "<7 7 [6@7 7] 4 [3@3 2] [3 4] 7 7 [6@7 7] 4 [3@3 2] [3 4] 7 7 [6@7 7] 4 [3@3 2] [3 4] 7 7 [6@7 7] 4 [3@3 2] [3 4]>".into(),
            )),
        ];

        for (label, source) in cases {
            let param = IntervalPatternParam::from_source(source.clone()).unwrap();
            let Some(pattern) = param.pattern() else {
                continue;
            };
            println!("=== {} ===", label);
            for cycle in 0..16 {
                let haps = pattern.query_cycle_all(cycle);
                for (i, hap) in haps.iter().enumerate() {
                    let mut spans = hap.context.get_all_span_tuples();
                    spans.sort();
                    println!(
                        "cycle={} hap={} wb={:.6} we={:.6} pb={:.6} pe={:.6} deg={:?} onset={} spans={:?}",
                        cycle,
                        i,
                        hap.whole_begin,
                        hap.whole_end,
                        hap.part_begin,
                        hap.part_end,
                        hap.value.degree(),
                        hap.has_onset(),
                        spans,
                    );
                }
            }
        }
    }

    /// Real-world bench: pulled directly from the user's live-coding patterns.
    /// Mix of patterned euclidean, stacks, replicates, slowcat alternations,
    /// and speed modifiers — exercises the chain end-to-end.
    #[test]
    #[ignore]
    fn bench_realworld_patterns() {
        use std::time::Instant;

        let cases: &[(&str, IntervalPatternSource)] = &[
            (
                "stack+slowcat",
                IntervalPatternSource::Multiple(vec![
                    "<0 2> 4".into(),
                    "-7, <0!6 1>".into(),
                ]),
            ),
            (
                "stack-only",
                IntervalPatternSource::Single("0, < 4!2 5 >".into()),
            ),
            (
                "fast-stack",
                IntervalPatternSource::Multiple(vec![
                    "<0 0 0 0 [3 2] 4 0 2>*8".into(),
                    "0, <4 ~>*6".into(),
                ]),
            ),
            (
                "fast-stack-2",
                IntervalPatternSource::Multiple(vec![
                    "<0 0 [3 2] 0 [4 0] 2>*8".into(),
                    "0, <4 ~>*6".into(),
                ]),
            ),
            (
                "stack-euclid",
                IntervalPatternSource::Multiple(vec![
                    "<0 0 [3 2] 0 [4 0] 2>*8".into(),
                    "0, <4 ~>*6, <2(2,5) ~!2>*8".into(),
                ]),
            ),
            (
                "complex",
                IntervalPatternSource::Multiple(vec![
                    "<[0 -1] 0 [3 2] 0 [4 0] 2>*8".into(),
                    "-7, 0, <4 ~>*6, <2(2,5) ~!2>*8".into(),
                ]),
            ),
            (
                "bass2",
                IntervalPatternSource::Multiple(vec![
                    "<[0,-7,4, <6 7 8>] ~>*16".into(),
                    "[0@7 -2@4 -3@5]/2".into(),
                ]),
            ),
            (
                "nest_single1",
                IntervalPatternSource::Single("<0 <[4 2] 3> [1 3]>*4".into()),
            ),
            (
                "speed_subseq",
                IntervalPatternSource::Single("0*[8 <6 4>]".into()),
            ),
            (
                "dminor_duo",
                IntervalPatternSource::Multiple(vec![
                    "<0 <-1 2>>*4".into(),
                    "<0@4 2@2 5@2>".into(),
                ]),
            ),
            (
                "speed_seq",
                IntervalPatternSource::Single("0*[8 8 12 8]".into()),
            ),
            (
                "bass_long",
                IntervalPatternSource::Single(
                    "<0 0 2 2 3 4 0 0 2 2 3 4 0 0 2 2 3 4 0 0 2 2 3 [4 -1]>".into(),
                ),
            ),
            (
                "wale_long",
                IntervalPatternSource::Single(
                    "<7 7 [6@7 7] 4 [3@3 2] [3 4] 7 7 [6@7 7] 4 [3@3 2] [3 4] 7 7 [6@7 7] 4 [3@3 2] [3 4] 7 7 [6@7 7] 4 [3@3 2] [3 4]>".into(),
                ),
            ),
        ];

        const ITERS: usize = 2000;

        for (label, source) in cases {
            let mut seq = IntervalSeq::default();
            seq.params.patterns = IntervalPatternParam::from_source(source.clone()).unwrap();
            seq.rebuild_module_cache();

            // Warm-up
            for _ in 0..32 {
                seq.ensure_cycle_cached(PARAM_CACHE_CYCLES as i64);
                seq.invalidate_cache();
            }

            let start = Instant::now();
            for i in 0..ITERS {
                let cycle = PARAM_CACHE_CYCLES as i64 + (i % MAX_MODULE_CYCLES) as i64;
                seq.ensure_cycle_cached(cycle);
                seq.invalidate_cache();
            }
            let per_call_ns = start.elapsed().as_nanos() as f64 / ITERS as f64;
            println!("{:<20} miss={:>9.1} ns", label, per_call_ns);
        }
    }

    /// Benchmark for `ensure_cycle_cached` on the audio-thread fall-through path.
    /// Run with: `cargo test -p modular_core --release bench_ensure_cycle_cached -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_ensure_cycle_cached() {
        use std::time::Instant;

        let cases: &[(&str, IntervalPatternSource)] = &[
            (
                "simple 4-note",
                IntervalPatternSource::Single("0 2 4 5".into()),
            ),
            (
                "euclidean",
                IntervalPatternSource::Single("0(5,8) 2(3,8) 4(7,16)".into()),
            ),
            (
                "3-pattern fold",
                IntervalPatternSource::Multiple(vec![
                    "0 2 4 5 7".into(),
                    "<0 3 5>".into(),
                    "0(3,8)".into(),
                ]),
            ),
            (
                "stack polyphony",
                IntervalPatternSource::Multiple(vec!["0 2 4".into(), "0,4,7".into()]),
            ),
        ];

        const ITERS: usize = 2000;

        for (label, source) in cases {
            // Fresh seq per case; rebuild cache so capacities are pre-allocated.
            let mut seq = IntervalSeq::default();
            seq.params.patterns = IntervalPatternParam::from_source(source.clone()).unwrap();
            seq.rebuild_module_cache();

            // Warm up — first call may include incidental setup
            seq.ensure_cycle_cached(PARAM_CACHE_CYCLES as i64);
            seq.invalidate_cache();

            let start = Instant::now();
            for i in 0..ITERS {
                // Every iter targets a fresh-invalidated slot, so the miss
                // path runs every time (worst case).
                let cycle = PARAM_CACHE_CYCLES as i64 + (i % MAX_MODULE_CYCLES) as i64;
                seq.ensure_cycle_cached(cycle);
                seq.invalidate_cache();
            }
            let elapsed = start.elapsed();
            let per_call_ns = elapsed.as_nanos() as f64 / ITERS as f64;

            // Steady-state hit
            let mut seq2 = IntervalSeq::default();
            seq2.params.patterns = IntervalPatternParam::from_source(source.clone()).unwrap();
            seq2.rebuild_module_cache();
            seq2.ensure_cycle_cached(PARAM_CACHE_CYCLES as i64); // prime
            let start_hit = Instant::now();
            for _ in 0..ITERS {
                seq2.ensure_cycle_cached(PARAM_CACHE_CYCLES as i64);
            }
            let elapsed_hit = start_hit.elapsed();
            let per_hit_ns = elapsed_hit.as_nanos() as f64 / ITERS as f64;

            println!(
                "{:<20} miss={:>8.1} ns  hit={:>6.1} ns",
                label, per_call_ns, per_hit_ns
            );
        }
    }
}
