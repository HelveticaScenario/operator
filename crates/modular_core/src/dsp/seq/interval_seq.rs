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
use std::sync::Arc;

use arrayvec::ArrayVec;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    dsp::{
        utilities::quantizer::ScaleParam,
        utils::{midi_to_voct_f64, min_gate_samples, TempGate, TempGateState},
    },
    pattern_system::Pattern,
    poly::{MonoSignalExt, PolyOutput, PORT_MAX_CHANNELS},
    types::Connect,
    MonoSignal, Patch,
};

/// Scale parameter for IntervalSeq that supports an optional octave in the root.
///
/// Wraps [`ScaleParam`] but deserializes via `parse_with_octave`, accepting
/// syntax like `"C3(major)"` or `"Db3(min)"` in addition to `"C(major)"`.
#[derive(Clone, Debug)]
struct IntervalScaleParam(ScaleParam);

impl Default for IntervalScaleParam {
    fn default() -> Self {
        Self(ScaleParam::parse_with_octave("C(major)").unwrap())
    }
}

impl<'de> serde::Deserialize<'de> for IntervalScaleParam {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let source = String::deserialize(deserializer)?;
        ScaleParam::parse_with_octave(&source)
            .map(Self)
            .ok_or_else(|| {
                serde::de::Error::custom(format!("Invalid scale specification: {}", source))
            })
    }
}

impl schemars::JsonSchema for IntervalScaleParam {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("IntervalScaleParam")
    }
    fn json_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(_gen)
    }
}

impl Connect for IntervalScaleParam {
    fn connect(&mut self, _patch: &Patch) {}
}

impl std::ops::Deref for IntervalScaleParam {
    type Target = ScaleParam;
    fn deref(&self) -> &ScaleParam {
        &self.0
    }
}

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
        match atom {
            crate::pattern_system::mini::ast::AtomValue::Number(n) => {
                Ok(IntervalValue::Degree(*n as i32))
            }
            crate::pattern_system::mini::ast::AtomValue::Midi(m) => {
                Ok(IntervalValue::Degree(*m as i32))
            }
            _ => Err(
                crate::pattern_system::mini::convert::ConvertError::InvalidAtom(
                    "IntervalValue only supports integers".to_string(),
                ),
            ),
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

/// Source representation for interval patterns: either a single pattern
/// string or an array of strings that get combined via `app_left` addition.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum IntervalPatternSource {
    Single(String),
    Multiple(Vec<String>),
}

impl Default for IntervalPatternSource {
    fn default() -> Self {
        Self::Single(String::new())
    }
}

impl IntervalPatternSource {
    /// Get the individual source strings.
    fn sources(&self) -> Vec<&str> {
        match self {
            Self::Single(s) => vec![s.as_str()],
            Self::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

/// Per-source metadata retained for span tracking.
#[derive(Clone, Debug, Default)]
pub struct SourceMeta {
    source: String,
    all_spans: Vec<(usize, usize)>,
}

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

    /// Pre-computed combined haps for cycles 0..999, populated at parse time.
    /// Each element is an Arc-wrapped Vec of all CombinedHaps intersecting that cycle.
    cached_haps: Vec<Arc<Vec<CombinedHap>>>,
}

impl Default for IntervalPatternParam {
    fn default() -> Self {
        Self {
            source: IntervalPatternSource::default(),
            combined_pattern: None,
            per_source: Vec::new(),
            num_sources: 0,
            cached_haps: Vec::new(),
        }
    }
}

impl IntervalPatternParam {
    /// Parse a single pattern string into a `Pattern<IntervalValue>` and collect leaf spans.
    fn parse_one(source: &str) -> Result<(Pattern<IntervalValue>, Vec<(usize, usize)>), String> {
        let ast = crate::pattern_system::mini::parse_ast(source).map_err(|e| e.to_string())?;
        let all_spans = crate::pattern_system::mini::collect_leaf_spans(&ast);
        let pattern = crate::pattern_system::mini::convert::<IntervalValue>(&ast)
            .map_err(|e| e.to_string())?;
        Ok((pattern, all_spans))
    }

    /// Build from an `IntervalPatternSource`, parsing and combining patterns.
    fn from_source(source: IntervalPatternSource) -> Result<Self, String> {
        let sources = source.sources();

        // Filter out empty strings
        let non_empty: Vec<&str> = sources.iter().copied().filter(|s| !s.is_empty()).collect();

        if non_empty.is_empty() {
            return Ok(Self {
                per_source: sources
                    .iter()
                    .map(|s| SourceMeta {
                        source: s.to_string(),
                        all_spans: Vec::new(),
                    })
                    .collect(),
                num_sources: sources.len(),
                source,
                combined_pattern: None,
                cached_haps: Vec::new(),
            });
        }

        // Parse each source string
        let mut parsed: Vec<Pattern<IntervalValue>> = Vec::new();
        let mut per_source: Vec<SourceMeta> = Vec::new();

        for s in &sources {
            if s.is_empty() {
                per_source.push(SourceMeta {
                    source: s.to_string(),
                    all_spans: Vec::new(),
                });
            } else {
                let (pattern, all_spans) = Self::parse_one(s)?;
                per_source.push(SourceMeta {
                    source: s.to_string(),
                    all_spans,
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

        let num_sources = sources.len();

        // Pre-compute and cache combined haps for cycles 0..999
        let cached_haps: Vec<Arc<Vec<CombinedHap>>> = (0..1000)
            .map(|cycle| {
                let haps = combined.query_cycle_all(cycle);
                let combined_haps: Vec<CombinedHap> = haps
                    .iter()
                    .map(|hap| {
                        let pattern_spans = extract_pattern_spans(&hap.context, num_sources);
                        CombinedHap {
                            whole_begin: hap.whole_begin,
                            whole_end: hap.whole_end,
                            part_begin: hap.part_begin,
                            part_end: hap.part_end,
                            degree: hap.value.degree(),
                            has_onset: hap.has_onset(),
                            pattern_spans,
                        }
                    })
                    .collect();
                Arc::new(combined_haps)
            })
            .collect();

        Ok(Self {
            source,
            combined_pattern: Some(combined),
            per_source,
            num_sources,
            cached_haps,
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

    /// Get the pre-computed cached combined haps for cycles 0..999.
    pub(crate) fn cached_haps(&self) -> &[Arc<Vec<CombinedHap>>] {
        &self.cached_haps
    }
}

impl<'de> Deserialize<'de> for IntervalPatternParam {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let source = IntervalPatternSource::deserialize(deserializer)?;
        Self::from_source(source).map_err(serde::de::Error::custom)
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
}

/// Cached hap info for voice assignment.
/// Holds an Arc reference to the cycle's combined hap vector plus an index,
/// avoiding clone allocations on the audio thread.
#[derive(Clone, Debug)]
struct CachedIntervalHap {
    /// Arc reference to the full cycle's combined hap vector.
    cycle_haps: Arc<Vec<CombinedHap>>,
    /// Index of this hap within the cycle_haps vector.
    hap_index: usize,
    /// The cycle this hap was cached for.
    cached_cycle: i64,
}

impl CachedIntervalHap {
    /// Get a reference to the underlying CombinedHap.
    #[inline]
    fn hap(&self) -> &CombinedHap {
        &self.cycle_haps[self.hap_index]
    }

    fn contains(&self, playhead: f64) -> bool {
        let hap = self.hap();
        playhead >= hap.whole_begin && playhead < hap.whole_end
    }
}

/// Per-voice state for polyphonic interval sequencer.
#[derive(Clone, Debug, Default)]
struct IntervalVoiceState {
    /// Cached hap info for this voice
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

#[derive(Clone, Deserialize, ChannelCount, JsonSchema, Connect, Debug, SignalParams)]
#[serde(rename_all = "camelCase")]
pub struct IntervalSeqParams {
    /// patterns to combine (left-fold with appLeft addition); accepts a single
    /// pattern string or an array of pattern strings
    #[serde(default)]
    patterns: IntervalPatternParam,
    /// scale for quantizing degrees to pitches (supports optional octave, e.g. "c3(major)")
    #[serde(default)]
    scale: IntervalScaleParam,
    /// playhead position
    #[default_connection(module = RootClock, port = "playhead", channels = [0, 1])]
    #[signal(range = (0.0, 1.0))]
    playhead: Option<MonoSignal>,
    /// number of polyphonic voices (1–16)
    #[serde(default = "default_channels")]
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

    for cycle_haps in cached {
        for hap in cycle_haps.iter() {
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

/// Extract per-pattern source spans from a combined hap's context.
///
/// After a left-fold of N patterns via `app_left`, the merged `HapContext`
/// has:
/// - `source_span` = pattern 0's leaf span
/// - `modifier_spans[i]` = pattern (i+1)'s leaf span
fn extract_pattern_spans(
    context: &crate::pattern_system::HapContext,
    num_patterns: usize,
) -> Vec<Vec<(usize, usize)>> {
    let mut result = Vec::with_capacity(num_patterns);

    // Pattern 0's span = source_span
    if num_patterns > 0 {
        result.push(context.source_span.iter().map(|s| s.to_tuple()).collect());
    }

    // Patterns 1..N spans = modifier_spans in order
    let modifier_limit = context
        .modifier_spans
        .len()
        .min(num_patterns.saturating_sub(1));
    for i in 0..modifier_limit {
        result.push(vec![context.modifier_spans[i].to_tuple()]);
    }

    // Pad with empty vecs if needed
    while result.len() < num_patterns {
        result.push(Vec::new());
    }

    result
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
pub struct IntervalSeqState {
    /// Per-voice state array
    voices: [IntervalVoiceState; PORT_MAX_CHANNELS],
    /// Round-robin voice index for allocation
    next_voice: usize,
    /// Current cycle number
    current_cycle: Option<i64>,
    /// Arc-wrapped combined haps for the current cycle, updated on cycle boundaries
    current_cycle_haps: Option<Arc<Vec<CombinedHap>>>,
    /// Module-level cache for cycles >= 1000 (element 0 = cycle 1000)
    module_cache: Vec<Option<Arc<Vec<CombinedHap>>>>,
    /// Cached scale intervals for degree-to-semitone conversion (no audio-thread allocs)
    scale_intervals: ArrayVec<i8, CAP>,
    /// Base MIDI note for degree 0 (includes root pitch class + octave)
    base_midi: i32,
}

impl Default for IntervalSeqState {
    fn default() -> Self {
        Self {
            voices: std::array::from_fn(|_| IntervalVoiceState::default()),
            next_voice: 0,
            current_cycle: None,
            current_cycle_haps: None,
            module_cache: Vec::new(),
            scale_intervals: [0, 2, 4, 5, 7, 9, 11].into_iter().collect(), // Default major scale
            base_midi: 60,                                                 // C4
        }
    }
}

/// A combined hap from the folded pattern, ready for voice allocation.
#[derive(Clone, Debug)]
pub(crate) struct CombinedHap {
    whole_begin: f64,
    whole_end: f64,
    part_begin: f64,
    part_end: f64,
    /// Combined degree, None if rest
    degree: Option<i32>,
    has_onset: bool,
    /// Source spans per input pattern (index 0 = first pattern, etc.)
    pattern_spans: Vec<Vec<(usize, usize)>>,
}

impl IntervalSeq {
    /// Invalidate the cycle cache.
    fn invalidate_cache(&mut self) {
        self.state.current_cycle = None;
        self.state.current_cycle_haps = None;
        self.state.module_cache.clear();
        // Do NOT clear voices — they hold their own Arc
    }

    /// Ensure that the given cycle's haps are available in either param cache or module cache.
    fn ensure_cycle_cached(&mut self, cycle: i64) {
        if cycle < 1000 {
            return; // Already in param cache
        }

        let module_idx = (cycle - 1000) as usize;

        if module_idx >= self.state.module_cache.capacity() {
            self.state.module_cache.resize(module_idx + 1, None);
        }

        if self.state.module_cache[module_idx].is_some() {
            return;
        }

        // Query pattern, convert to CombinedHaps, store in module cache
        if let Some(pattern) = self.params.patterns.pattern() {
            let haps = pattern.query_cycle_all(cycle);
            let num_patterns = self.params.patterns.num_sources();
            let combined_haps: Vec<CombinedHap> = haps
                .iter()
                .map(|hap| {
                    let pattern_spans = extract_pattern_spans(&hap.context, num_patterns);
                    CombinedHap {
                        whole_begin: hap.whole_begin,
                        whole_end: hap.whole_end,
                        part_begin: hap.part_begin,
                        part_end: hap.part_end,
                        degree: hap.value.degree(),
                        has_onset: hap.has_onset(),
                        pattern_spans,
                    }
                })
                .collect();
            self.state.module_cache[module_idx] = Some(Arc::new(combined_haps));
        }
    }

    /// Get a reference to the Arc-wrapped haps for the given cycle.
    fn get_cycle_haps(&self, cycle: i64) -> Option<&Arc<Vec<CombinedHap>>> {
        if cycle < 1000 {
            self.params.patterns.cached_haps().get(cycle as usize)
        } else {
            let module_idx = (cycle - 1000) as usize;
            self.state
                .module_cache
                .get(module_idx)
                .and_then(|opt| opt.as_ref())
        }
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

        // Total MIDI note: base_midi (root + octave) + degree_octave*12 + semitone_in_scale
        let midi = self.state.base_midi + (octave * 12) + semitone_in_scale;

        midi_to_voct_f64(midi as f64)
    }

    /// Update cached scale info from params.
    fn update_scale_cache(&mut self) {
        let scale: &ScaleParam = &self.params.scale;
        self.state.base_midi = scale.base_midi();
        if let Some(snapper) = scale.snapper() {
            self.state.scale_intervals = snapper.scale_intervals().clone();
        } else {
            // Chromatic - all 12 semitones
            self.state.scale_intervals = (0i8..CAP as i8).into_iter().collect();
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
            self.state.current_cycle_haps = self.get_cycle_haps(current_cycle).cloned();
            self.state.current_cycle = Some(current_cycle);
        }

        // Collect events to process (avoids borrow conflicts)
        // Store (hap_index, degree) tuples — we'll create CachedIntervalHap from the Arc later
        let cycle_haps_arc = self.state.current_cycle_haps.clone();
        let events_to_process: Vec<(usize, i32)> = if let Some(ref cycle_haps) = cycle_haps_arc {
            cycle_haps
                .iter()
                .enumerate()
                .filter_map(|(hap_index, combined)| {
                    if !combined.has_onset {
                        return None;
                    }

                    if playhead < combined.part_begin || playhead >= combined.part_end {
                        return None;
                    }

                    let degree = combined.degree?;

                    // Check if already assigned
                    let already_assigned = (0..num_channels).any(|i| {
                        if let Some(ref existing) = self.state.voices[i].cached_hap {
                            existing.hap_index == hap_index
                                && existing.cached_cycle == current_cycle
                        } else {
                            false
                        }
                    });

                    if already_assigned {
                        return None;
                    }

                    Some((hap_index, degree))
                })
                .collect()
        } else {
            Vec::new()
        };

        // Process collected events
        if let Some(cycle_haps) = cycle_haps_arc {
            for (hap_index, degree) in events_to_process {
                let Some(voice_idx) = self.allocate_voice(playhead, num_channels) else {
                    continue; // No free voice — skip this event
                };
                let voltage = self.degree_to_voltage(degree);

                let voice = &mut self.state.voices[voice_idx];
                voice.cached_hap = Some(CachedIntervalHap {
                    cycle_haps: cycle_haps.clone(),
                    hap_index,
                    cached_cycle: current_cycle,
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
        }

        // Output all voices
        for ch in 0..num_channels {
            let voice = &mut self.state.voices[ch];

            if voice.active {
                self.outputs.cv.set(ch, voice.cached_voltage as f32);
            }

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
            if let Some(ref cached) = self.state.voices[i].cached_hap {
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
            if voice.active {
                if let Some(ref cached) = voice.cached_hap {
                    any_active = true;
                    for (i, spans) in cached.hap().pattern_spans.iter().enumerate() {
                        if i < num_sources {
                            per_pattern_spans[i].extend(spans.iter().cloned());
                        }
                    }
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
        self.update_scale_cache();
        // Combined pattern is already built at parse time inside IntervalPatternParam
    }
}

message_handlers!(impl IntervalSeq {});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interval_value_from_atom() {
        use crate::pattern_system::mini::ast::AtomValue;
        use crate::pattern_system::mini::convert::FromMiniAtom;

        let v = IntervalValue::from_atom(&AtomValue::Number(5.0)).unwrap();
        assert!(matches!(v, IntervalValue::Degree(5)));

        let v = IntervalValue::from_atom(&AtomValue::Midi(3)).unwrap();
        assert!(matches!(v, IntervalValue::Degree(3)));
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
    fn test_scale_param_with_octave() {
        use crate::dsp::utilities::quantizer::ScaleParam;

        let scale = ScaleParam::parse_with_octave("C3(major)").unwrap();
        assert_eq!(scale.base_midi(), 48);
        assert!(scale.snapper().is_some());

        let scale = ScaleParam::parse_with_octave("Db3(min)").unwrap();
        assert_eq!(scale.base_midi(), 49);
        assert!(scale.snapper().is_some());

        // Without octave defaults to octave 4
        let scale = ScaleParam::parse_with_octave("C(major)").unwrap();
        assert_eq!(scale.base_midi(), 60);

        let scale = ScaleParam::parse_with_octave("D(major)").unwrap();
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
    fn test_deserialize_patterns_from_string() {
        let json = serde_json::json!({ "patterns": "0 2 4" });
        let params: IntervalSeqParams = serde_json::from_value(json).unwrap();
        assert!(params.patterns.pattern().is_some());
        assert_eq!(params.patterns.num_sources(), 1);
    }

    #[test]
    fn test_deserialize_patterns_from_array() {
        let json = serde_json::json!({ "patterns": ["0 2 4", "0 3"] });
        let params: IntervalSeqParams = serde_json::from_value(json).unwrap();
        assert!(params.patterns.pattern().is_some());
        assert_eq!(params.patterns.num_sources(), 2);
    }
}
