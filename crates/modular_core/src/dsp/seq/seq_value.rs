//! SeqValue enum and SeqPatternParam for the new pattern-based sequencer.
//!
//! SeqValue represents the different value types that can appear in a sequence:
//! - Voltage values (V/Oct, pre-converted at parse time)
//! - Rests

use deserr::{Deserr, DeserializeError, ErrorKind, IntoValue, Map, Sequence, ValuePointerRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Patch,
    dsp::utils::midi_to_voct_f64,
    pattern_system::{
        Pattern,
        mini::{
            FromMiniAtom, MiniAST,
            ast::AtomValue,
            convert::{ConvertError, HasRest},
        },
    },
    types::Connect,
};

use super::cache::{
    CycleStorage, MIN_HAPS_CAP_HINT, MIN_SPANS_CAP_HINT, PARAM_CACHE_CYCLES, SPANS_RESERVE_PER_HAP,
    populate_cycle_storage as cache_populate,
};

/// Scalar cached hap data. No `Vec` or `Arc` — voices can hold these
/// by value without keeping cycle storage alive.
#[derive(Clone, Debug)]
pub(crate) struct SeqCycleHap {
    pub whole_begin: f64,
    pub whole_end: f64,
    pub part_begin: f64,
    pub part_end: f64,
    pub value: SeqValue,
    pub has_onset: bool,
    /// Range into the owning [`SeqCycleStorage::span_arena`].
    pub span_offset: u32,
    pub span_len: u32,
}

/// Per-cycle storage for Seq. Scalar haps + flat span arena. Each
/// `FlatSpan` carries the `pattern_idx` so a chained `$sp` payload's
/// multi-source highlights know which input string each leaf came from.
pub(crate) type SeqCycleStorage = CycleStorage<SeqCycleHap, super::cache::FlatSpan>;

/// Fill `storage` with the pattern's haps for `cycle`.
pub(crate) fn populate_cycle_storage(
    pattern: &Pattern<SeqValue>,
    cycle: i64,
    bump: &mut bumpalo::Bump,
    storage: &mut SeqCycleStorage,
) {
    cache_populate(pattern, cycle, bump, storage, |hap, haps, span_arena| {
        let span_offset = span_arena.len() as u32;
        hap.context.walk(&mut |pattern_idx, span| {
            span_arena.push(super::cache::FlatSpan {
                pattern_idx,
                start: span.start as u32,
                end: span.end as u32,
            });
        });
        let span_len = span_arena.len() as u32 - span_offset;
        haps.push(SeqCycleHap {
            whole_begin: hap.whole_begin_f64(),
            whole_end: hap.whole_end_f64(),
            part_begin: hap.part_begin_f64(),
            part_end: hap.part_end_f64(),
            value: hap.value,
            has_onset: hap.has_onset(),
            span_offset,
            span_len,
        });
    });
}

/// A value in a sequence pattern.
///
/// Represents the different types of values that can be sequenced:
/// - Voltage (V/Oct, pre-converted from MIDI/note at parse time)
/// - Rests (silence/no output)
#[derive(Clone, Copy, Debug)]
pub enum SeqValue {
    /// Pre-converted V/Oct voltage value.
    /// This replaces both Midi and Note variants - conversion happens at parse time.
    Voltage(f64),

    /// Rest - no value output, gate goes low
    Rest,
}

impl SeqValue {
    /// Get the voltage (V/Oct) value.
    /// Returns None for Rest.
    pub fn to_voltage(&self) -> Option<f64> {
        match self {
            SeqValue::Voltage(v) => Some(*v),
            SeqValue::Rest => None,
        }
    }

    /// Check if this is a rest value.
    pub fn is_rest(&self) -> bool {
        matches!(self, SeqValue::Rest)
    }
}

/// Convert a note letter, accidental, and octave to MIDI note number.
fn note_to_midi(letter: char, accidental: Option<char>, octave: Option<i32>) -> Option<f64> {
    let base = match letter.to_ascii_lowercase() {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };

    let acc_offset = match accidental {
        Some('#') => 1,
        Some('b') => -1,
        _ => 0,
    };

    // Default octave to 4 if not specified
    let oct = octave.unwrap_or(4);
    Some(((oct + 1) * 12 + base + acc_offset) as f64)
}

impl FromMiniAtom for SeqValue {
    fn from_atom(atom: &AtomValue) -> Result<Self, ConvertError> {
        match atom {
            AtomValue::Number(n) => {
                // Bare numbers are voltages directly (1V/oct CV).
                Ok(SeqValue::Voltage(*n))
            }
            AtomValue::Hz(hz) => {
                // Convert Hz to MIDI then to voltage.
                let midi = 12.0 * (hz / 440.0).log2() + 69.0;
                Ok(SeqValue::Voltage(midi_to_voct_f64(midi)))
            }
            AtomValue::Note {
                letter,
                accidental,
                octave,
            } => {
                if let Some(midi) = note_to_midi(*letter, *accidental, *octave) {
                    Ok(SeqValue::Voltage(midi_to_voct_f64(midi)))
                } else {
                    Err(ConvertError::InvalidAtom(format!(
                        "Invalid note: {}{}{}",
                        letter,
                        accidental
                            .as_ref()
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                        octave.map(|o| o.to_string()).unwrap_or_default()
                    )))
                }
            }
        }
    }

    fn from_list(atoms: &[AtomValue]) -> Result<Self, ConvertError> {
        // Lists are handled by the scale operator, not here
        if atoms.len() == 1 {
            Self::from_atom(&atoms[0])
        } else {
            Err(ConvertError::ListNotSupported)
        }
    }

    fn combine_with_head(_head_atoms: &[AtomValue], _tail: &Self) -> Result<Self, ConvertError> {
        // SeqValue doesn't support head:tail combination directly.
        // Use operators like scale() for combining notes with scale patterns.
        Err(ConvertError::ListNotSupported)
    }

    fn rest_value() -> Option<Self> {
        Some(SeqValue::Rest)
    }

    fn supports_rest() -> bool {
        true
    }
}

impl HasRest for SeqValue {
    fn rest_value() -> Self {
        SeqValue::Rest
    }
}

/// JSON payload shape delivered in the patch graph for
/// `SeqPatternParam` / `IntervalPatternParam`. Produced client-side by
/// the TypeScript `$p(...)` helper in `src/main/dsl/miniNotation/`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct ParsedPatternPayload {
    /// The parsed AST.
    pub ast: MiniAST,
    /// The original mini-notation source string.
    pub source: String,
    /// Pre-computed leaf spans (used for Monaco tracked decorations).
    pub all_spans: Vec<(usize, usize)>,
}

impl ParsedPatternPayload {
    /// Build a payload by parsing a mini-notation string via the in-crate
    /// test parser. Integration tests in `tests/` need this (they're a
    /// separate crate, so `#[cfg(test)]` items in the lib aren't visible).
    /// The production path is the TypeScript `$p()` helper; this exists
    /// only so existing Rust fixtures don't need to hand-build ASTs.
    #[doc(hidden)]
    pub fn parse_for_test(source: &str) -> Self {
        if source.is_empty() {
            return Self {
                ast: MiniAST::Sequence(Vec::new()),
                source: String::new(),
                all_spans: Vec::new(),
            };
        }
        let ast = crate::pattern_system::mini::parse_ast(source)
            .expect("test_parser should parse the fixture source");
        let all_spans = crate::pattern_system::mini::collect_leaf_spans(&ast);
        ParsedPatternPayload {
            ast,
            source: source.to_string(),
            all_spans,
        }
    }
}

#[cfg(test)]
impl From<&str> for ParsedPatternPayload {
    fn from(source: &str) -> Self {
        Self::parse_for_test(source)
    }
}

#[cfg(test)]
impl From<String> for ParsedPatternPayload {
    fn from(source: String) -> Self {
        Self::parse_for_test(&source)
    }
}

/// Deserr bridge — round-trip via `serde_json::Value`. The payload is
/// structurally complex (recursive `MiniAST`), so a hand-rolled deserr
/// impl would duplicate the existing serde `Deserialize` impl. deserr's
/// `IntoValue` is trivially convertible to `serde_json::Value`, so we
/// lean on that.
impl<E: DeserializeError> Deserr<E> for ParsedPatternPayload {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        let json = value_to_json(value);
        serde_json::from_value::<ParsedPatternPayload>(json).map_err(|e| {
            deserr::take_cf_content(E::error::<V>(
                None,
                ErrorKind::Unexpected {
                    msg: format!("invalid parsed pattern payload: {e}"),
                },
                location,
            ))
        })
    }
}

fn value_to_json<V: IntoValue>(value: deserr::Value<V>) -> serde_json::Value {
    match value {
        deserr::Value::Null => serde_json::Value::Null,
        deserr::Value::Boolean(b) => serde_json::Value::Bool(b),
        deserr::Value::Integer(i) => serde_json::Value::Number(i.into()),
        deserr::Value::NegativeInteger(i) => serde_json::Value::Number(i.into()),
        deserr::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        deserr::Value::String(s) => serde_json::Value::String(s),
        deserr::Value::Sequence(seq) => serde_json::Value::Array(
            seq.into_iter()
                .map(|v: V| value_to_json::<V>(v.into_value()))
                .collect(),
        ),
        deserr::Value::Map(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map.into_iter() {
                out.insert(k, value_to_json::<V>(v.into_value()));
            }
            serde_json::Value::Object(out)
        }
    }
}

/// Default for `MiniAST` — empty sequence. Required so
/// `ParsedPatternPayload` can derive `Default`; deserialization always
/// overwrites the default.
impl Default for MiniAST {
    fn default() -> Self {
        MiniAST::Sequence(Vec::new())
    }
}

/// Arithmetic operation kind for chained `$sp` ops. Wire form uses
/// lowercase strings (`"add"`, `"sub"`) to match the TS builder.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SpOpKind {
    Add,
    Sub,
}

/// Strudel-style alignment mode for chained `$sp` ops. Wire form uses
/// lowercase strings (`"in"`, `"out"`, `"mix"`, `"squeeze"`,
/// `"squeezeout"`, `"reset"`, `"restart"`) to match the TS builder.
/// Mirrors [`crate::pattern_system::sp_combine::SpAlignmentMode`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SpAlignmentMode {
    In,
    Out,
    Mix,
    Squeeze,
    SqueezeOut,
    Reset,
    Restart,
}

impl SpAlignmentMode {
    fn to_rt(self) -> crate::pattern_system::sp_combine::SpAlignmentMode {
        use crate::pattern_system::sp_combine::SpAlignmentMode as Rt;
        match self {
            Self::In => Rt::In,
            Self::Out => Rt::Out,
            Self::Mix => Rt::Mix,
            Self::Squeeze => Rt::Squeeze,
            Self::SqueezeOut => Rt::SqueezeOut,
            Self::Reset => Rt::Reset,
            Self::Restart => Rt::Restart,
        }
    }
}

/// One chained op in an `$sp` payload: arithmetic op + alignment mode.
/// Paired positionally with `sources[i+1]` for `i = 0..ops.len()`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SpOp {
    pub op: SpOpKind,
    pub mode: SpAlignmentMode,
}

/// Source span used as an editor argument-span anchor.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ArgumentSpan {
    pub start: usize,
    pub end: usize,
}

/// JSON payload shape delivered for chained `$sp(...).add(...)` patterns.
/// First entry of `sources` is the left pattern; subsequent entries are
/// chained RHS patterns combined via the parallel `ops` slot.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct SpPatternPayload {
    /// Discriminator — TS-side `$sp` builds objects with this set so the
    /// `SeqPatternSource` untagged enum picks the `Sp` variant.
    #[serde(rename = "__kind")]
    pub kind: SpKindTag,
    pub sources: Vec<ParsedPatternPayload>,
    pub scale: String,
    pub ops: Vec<SpOp>,
    pub argument_spans: Vec<ArgumentSpan>,
}

/// Phantom-style discriminator that matches the literal `"SpPattern"`
/// string in JSON.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "PascalCase")]
pub enum SpKindTag {
    #[default]
    SpPattern,
}

/// Wire-level dispatch shape: either a single `ParsedPatternPayload`
/// (legacy single-source path used by `$cycle($p(...))`) or a chained
/// `$sp` payload with multi-source highlighting.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SeqPatternSource {
    Sp(SpPatternPayload),
    Single(ParsedPatternPayload),
}

impl Default for SeqPatternSource {
    fn default() -> Self {
        Self::Single(ParsedPatternPayload::default())
    }
}

/// Per-source metadata retained for span tracking. Mirrors
/// `IntervalPatternParam::SourceMeta` (`interval_seq.rs:130-138`).
#[derive(Clone, Debug, Default)]
pub struct SeqSourceMeta {
    pub source: String,
    pub all_spans: Vec<(usize, usize)>,
}

/// A pattern parameter that wraps a parsed pattern payload and its
/// derived runtime state.
///
/// Wire shape is [`SeqPatternSource`] — either a single
/// [`ParsedPatternPayload`] (the existing single-source path for
/// `$cycle($p(...))`) or an [`SpPatternPayload`] carrying multiple
/// degree patterns + scale + chain ops for `$cycle($sp(...).add(...))`.
/// Either way the resolved runtime is a [`Pattern<SeqValue>`] plus
/// per-cycle hap storage.
#[derive(Clone, Default, Debug)]
pub struct SeqPatternParam {
    /// First source string (echoed for legacy callers that ask for `.source()`).
    #[allow(dead_code)]
    source: String,

    /// Per-source metadata (always at least one entry once parsed).
    /// Chained `$sp` payloads push one entry per chained RHS.
    pub(crate) per_source: Vec<SeqSourceMeta>,

    /// True when the original payload was an `Sp` (chained), so
    /// `get_state` emits `pattern.0`, `pattern.1`, ... rather than the
    /// single `pattern` key.
    pub(crate) is_multi_source: bool,

    /// The parsed pattern.
    pub(crate) pattern: Option<Pattern<SeqValue>>,

    /// Leaf spans of the first source (`per_source[0]`) — kept as a
    /// flat `(start, end)` list for back-compat with callers that don't
    /// know about multi-source.
    pub(crate) all_spans: Vec<(usize, usize)>,

    /// Pre-computed haps for cycles 0..PARAM_CACHE_CYCLES.
    pub(crate) cached_haps: Vec<SeqCycleStorage>,

    /// Largest `haps.len()` seen across the cached cycles.
    pub(crate) max_haps_per_cycle: usize,

    /// Largest `span_arena.len()` seen across the cached cycles.
    pub(crate) max_spans_per_cycle: usize,
}

impl JsonSchema for SeqPatternParam {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        SeqPatternSource::schema_name()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        SeqPatternSource::json_schema(generator)
    }
}

impl SeqPatternParam {
    fn from_payload(payload: ParsedPatternPayload) -> Result<Self, String> {
        if payload.source.is_empty() {
            return Ok(Self::default());
        }
        let pattern = crate::pattern_system::mini::convert::<SeqValue>(&payload.ast)
            .map_err(|e| e.to_string())?;

        let per_source = vec![SeqSourceMeta {
            source: payload.source.clone(),
            all_spans: payload.all_spans.clone(),
        }];

        let (cached_haps, max_haps_per_cycle, max_spans_per_cycle) =
            bake_cycles(&pattern);

        Ok(Self {
            source: payload.source,
            per_source,
            is_multi_source: false,
            pattern: Some(pattern),
            all_spans: payload.all_spans,
            cached_haps,
            max_haps_per_cycle,
            max_spans_per_cycle,
        })
    }

    fn from_sp_payload(payload: SpPatternPayload) -> Result<Self, String> {
        use crate::dsp::seq::interval_seq::{
            IntervalValue, add_interval_values, sub_interval_values,
        };
        use crate::dsp::utilities::quantizer::{ScaleParam, degree_to_voltage};
        use crate::pattern_system::sp_combine::combine_sp;

        if payload.sources.is_empty() {
            return Ok(Self::default());
        }
        if payload.ops.len() + 1 != payload.sources.len() {
            return Err(format!(
                "$sp payload mismatch: {} sources but {} ops (expected ops.len() == sources.len() - 1)",
                payload.sources.len(),
                payload.ops.len()
            ));
        }

        // Empty leftmost source short-circuits to silence — matches the
        // single-source convention above.
        if payload.sources[0].source.is_empty() {
            return Ok(Self::default());
        }

        // Parse scale up front so a bad scale string fails before we do
        // any pattern work.
        let scale = ScaleParam::parse(&payload.scale)
            .ok_or_else(|| format!("invalid scale: {}", payload.scale))?;
        let base_midi = scale.base_midi();
        let (intervals, tuning): (Vec<i8>, [f64; 12]) = match scale.snapper() {
            Some(s) => (s.scale_intervals().iter().copied().collect(), *s.tuning()),
            None => ((0i8..12).collect(), std::array::from_fn(|i| i as f64 / 12.0)),
        };

        // Lower each source AST into a Pattern<IntervalValue>. Strip
        // modifier spans before combining so each input's span tree
        // becomes its own primary chain — the walk emitter then assigns
        // pattern_idx = 0 to source[0], 1 to source[1], etc.
        let mut patterns: Vec<crate::pattern_system::Pattern<IntervalValue>> =
            Vec::with_capacity(payload.sources.len());
        for src in &payload.sources {
            let p = crate::pattern_system::mini::convert::<IntervalValue>(&src.ast)
                .map_err(|e| e.to_string())?;
            patterns.push(p.strip_modifier_spans());
        }

        // Fold left.
        let mut combined = patterns[0].clone();
        for (i, op) in payload.ops.iter().enumerate() {
            let rhs = &patterns[i + 1];
            let f: fn(&IntervalValue, &IntervalValue) -> IntervalValue = match op.op {
                SpOpKind::Add => add_interval_values,
                SpOpKind::Sub => sub_interval_values,
            };
            combined = combine_sp(&combined, rhs, op.mode.to_rt(), f);
        }

        // Resolve degrees -> SeqValue voltages, then cache cycles.
        let resolver = move |v: &IntervalValue| match v {
            IntervalValue::Degree(d) => SeqValue::Voltage(degree_to_voltage(
                *d,
                base_midi,
                &intervals,
                &tuning,
            )),
            IntervalValue::Rest => SeqValue::Rest,
        };
        let voltage_pattern = combined.fmap(resolver);

        let per_source: Vec<SeqSourceMeta> = payload
            .sources
            .iter()
            .map(|s| SeqSourceMeta {
                source: s.source.clone(),
                all_spans: s.all_spans.clone(),
            })
            .collect();

        let (cached_haps, max_haps_per_cycle, max_spans_per_cycle) =
            bake_cycles(&voltage_pattern);

        Ok(Self {
            source: payload.sources[0].source.clone(),
            per_source,
            is_multi_source: true,
            pattern: Some(voltage_pattern),
            all_spans: payload.sources[0].all_spans.clone(),
            cached_haps,
            max_haps_per_cycle,
            max_spans_per_cycle,
        })
    }

    /// Get the parsed pattern.
    pub fn pattern(&self) -> Option<&Pattern<SeqValue>> {
        self.pattern.as_ref()
    }

    /// Get the source pattern string (the evaluated pattern passed to the parser).
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Get all leaf spans in the pattern (for frontend tracked decorations).
    pub fn all_spans(&self) -> &[(usize, usize)] {
        &self.all_spans
    }

    /// Per-source metadata. Single-source legacy payloads return a
    /// one-element slice; chained `$sp` payloads return one entry per
    /// source string in the chain.
    pub(crate) fn per_source(&self) -> &[SeqSourceMeta] {
        &self.per_source
    }

    /// `true` when the payload was an `Sp` chain (multi-source param
    /// span output keyed by `pattern.0`, `pattern.1`, ...).
    pub(crate) fn is_multi_source(&self) -> bool {
        self.is_multi_source
    }

    /// Get the pre-computed cached cycle storages for cycles 0..PARAM_CACHE_CYCLES.
    pub(crate) fn cached_haps(&self) -> &[SeqCycleStorage] {
        &self.cached_haps
    }

    /// Capacity hint for sizing audio-thread cycle storages.
    pub(crate) fn max_haps_per_cycle(&self) -> usize {
        self.max_haps_per_cycle
    }

    /// Capacity hint for sizing audio-thread span arenas.
    pub(crate) fn max_spans_per_cycle(&self) -> usize {
        self.max_spans_per_cycle
    }
}

/// Pre-compute cycles 0..PARAM_CACHE_CYCLES of a `Pattern<SeqValue>`
/// into `SeqCycleStorage` slots and derive audio-thread capacity hints.
fn bake_cycles(pattern: &Pattern<SeqValue>) -> (Vec<SeqCycleStorage>, usize, usize) {
    let mut bump = bumpalo::Bump::new();
    let mut cached_haps: Vec<SeqCycleStorage> = Vec::with_capacity(PARAM_CACHE_CYCLES);
    for cycle in 0..PARAM_CACHE_CYCLES as i64 {
        let mut storage = SeqCycleStorage::with_capacity(
            MIN_HAPS_CAP_HINT,
            MIN_HAPS_CAP_HINT * SPANS_RESERVE_PER_HAP,
        );
        populate_cycle_storage(pattern, cycle, &mut bump, &mut storage);
        cached_haps.push(storage);
    }
    let max_haps = cached_haps.iter().map(|c| c.haps.len()).max().unwrap_or(0);
    let max_spans = cached_haps
        .iter()
        .map(|c| c.span_arena.len())
        .max()
        .unwrap_or(0);
    let max_haps_per_cycle = (max_haps.max(MIN_HAPS_CAP_HINT) * 3) / 2;
    let max_spans_per_cycle = (max_spans.max(MIN_SPANS_CAP_HINT) * 3) / 2;
    (cached_haps, max_haps_per_cycle, max_spans_per_cycle)
}

// deserr implementation: reads either a plain `ParsedPatternPayload`
// (`{ ast, source, all_spans }`) or the chained `SpPatternPayload`
// (`{ __kind: 'SpPattern', sources, scale, ops, argument_spans }`).
impl<E: DeserializeError> Deserr<E> for SeqPatternParam {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        let json = value_to_json(value);
        let parsed: SeqPatternSource = serde_json::from_value(json).map_err(|e| {
            deserr::take_cf_content(E::error::<V>(
                None,
                ErrorKind::Unexpected {
                    msg: format!("invalid seq pattern payload: {e}"),
                },
                location,
            ))
        })?;
        match parsed {
            SeqPatternSource::Single(p) => Self::from_payload(p).map_err(|e| {
                deserr::take_cf_content(E::error::<V>(
                    None,
                    ErrorKind::Unexpected { msg: e },
                    location,
                ))
            }),
            SeqPatternSource::Sp(p) => Self::from_sp_payload(p).map_err(|e| {
                deserr::take_cf_content(E::error::<V>(
                    None,
                    ErrorKind::Unexpected { msg: e },
                    location,
                ))
            }),
        }
    }
}

impl Connect for SeqPatternParam {
    fn connect(&mut self, _patch: &Patch) {}
    fn collect_cables(&self, _sink: &mut Vec<String>) {}
    fn inject_index_ptr(&mut self, _ptr: *const std::cell::Cell<usize>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seq_value_to_voltage() {
        // C4 = MIDI 60 -> voltage = (60 - 33) / 12 = 2.25
        let c4_voltage = midi_to_voct_f64(60.0);
        assert!((SeqValue::Voltage(c4_voltage).to_voltage().unwrap() - c4_voltage).abs() < 0.001);

        // C4 + 50 cents = MIDI 60.5 -> voltage = (60.5 - 33) / 12 = 2.2917
        let c4_50_cents_voltage = midi_to_voct_f64(60.5);
        assert!(
            (SeqValue::Voltage(c4_50_cents_voltage).to_voltage().unwrap() - c4_50_cents_voltage)
                .abs()
                < 0.001
        );

        assert_eq!(SeqValue::Rest.to_voltage(), None);
    }

    #[test]
    fn test_note_to_midi_helper() {
        // C4 = MIDI 60
        assert_eq!(note_to_midi('c', None, Some(4)), Some(60.0));
        // C (default octave 4) = MIDI 60
        assert_eq!(note_to_midi('c', None, None), Some(60.0));
        // C#4 = MIDI 61
        assert_eq!(note_to_midi('c', Some('#'), Some(4)), Some(61.0));
        // A0 = MIDI 21
        assert_eq!(note_to_midi('a', None, Some(0)), Some(21.0));
        // A1 = MIDI 33 (our 0V reference)
        assert_eq!(note_to_midi('a', None, Some(1)), Some(33.0));
    }

    #[test]
    fn test_from_atom() {
        // Bare numbers are voltages directly (1V/oct).
        let n = SeqValue::from_atom(&AtomValue::Number(2.0)).unwrap();
        assert!(matches!(n, SeqValue::Voltage(v) if (v - 2.0).abs() < 0.001));

        // Note is converted to voltage at parse time
        let note = SeqValue::from_atom(&AtomValue::Note {
            letter: 'a',
            accidental: None,
            octave: Some(4),
        })
        .unwrap();
        let expected_a4_voltage = midi_to_voct_f64(69.0); // A4 = MIDI 69
        assert!(matches!(note, SeqValue::Voltage(v) if (v - expected_a4_voltage).abs() < 0.001));
    }

    #[test]
    fn test_note_octaves_different() {
        use crate::pattern_system::Fraction;
        use crate::pattern_system::mini::parse;

        // Parse "a1 a2 a3 a4" and check each note has different voltage
        let pattern: crate::pattern_system::Pattern<SeqValue> =
            parse("a1 a2 a3 a4").expect("Should parse");

        let haps = pattern.query_arc(Fraction::from(0), Fraction::from(1));

        assert_eq!(haps.len(), 4, "Should have 4 haps");

        let voltages: Vec<f64> = haps.iter().filter_map(|h| h.value.to_voltage()).collect();

        // a1 = MIDI 33 = 0V, a2 = MIDI 45 = 1V, a3 = MIDI 57 = 2V, a4 = MIDI 69 = 3V
        let expected = [
            midi_to_voct_f64(33.0), // a1
            midi_to_voct_f64(45.0), // a2
            midi_to_voct_f64(57.0), // a3
            midi_to_voct_f64(69.0), // a4
        ];

        for (i, (actual, expected)) in voltages.iter().zip(expected.iter()).enumerate() {
            assert!(
                (actual - expected).abs() < 0.001,
                "a{} voltage mismatch",
                i + 1
            );
        }
    }

    #[test]
    fn test_seq_value_supports_rest() {
        use crate::pattern_system::mini::convert::FromMiniAtom;
        // SeqValue should support rests
        assert!(SeqValue::supports_rest());
        assert!(<SeqValue as FromMiniAtom>::rest_value().is_some());
        assert!(matches!(
            <SeqValue as FromMiniAtom>::rest_value(),
            Some(SeqValue::Rest)
        ));
    }

    #[test]
    fn test_seq_value_has_rest_trait() {
        // Test HasRest trait implementation
        use crate::pattern_system::HasRest;
        let rest = <SeqValue as HasRest>::rest_value();
        assert!(matches!(rest, SeqValue::Rest));
    }

    #[test]
    fn test_seq_value_euclidean() {
        use crate::pattern_system::Fraction;
        use crate::pattern_system::mini::parse;

        // Test that euclidean patterns work with SeqValue
        // c(2,4) means 2 pulses in 4 steps, so we should get:
        // [c, ~, c, ~] = c at 0-0.25 and 0.5-0.75, rest at 0.25-0.5 and 0.75-1.0
        let pattern: crate::pattern_system::Pattern<SeqValue> =
            parse("c(2,4)").expect("Should parse euclidean pattern");

        let haps = pattern.query_arc(Fraction::from(0), Fraction::from(1));

        println!("Euclidean c(2,4) haps:");
        for hap in &haps {
            println!(
                "  {:?} at {:?}-{:?}",
                hap.value,
                hap.whole.as_ref().map(|w| w.begin.to_string()),
                hap.whole.as_ref().map(|w| w.end.to_string())
            );
        }

        assert_eq!(haps.len(), 4, "Should have 4 haps (2 notes, 2 rests)");

        // Count notes and rests
        let notes: Vec<_> = haps.iter().filter(|h| !h.value.is_rest()).collect();
        let rests: Vec<_> = haps.iter().filter(|h| h.value.is_rest()).collect();

        assert_eq!(notes.len(), 2, "Should have 2 note haps");
        assert_eq!(rests.len(), 2, "Should have 2 rest haps");
    }

    #[test]
    fn test_seq_value_rest_in_pattern() {
        use crate::pattern_system::Fraction;
        use crate::pattern_system::mini::parse;

        // SeqValue should allow rest (~) in patterns
        let pattern: crate::pattern_system::Pattern<SeqValue> =
            parse("c4 ~ e4").expect("Should parse with rest");

        let haps = pattern.query_arc(Fraction::from(0), Fraction::from(1));
        assert_eq!(haps.len(), 3, "Should have 3 haps including rest");

        // Second hap should be a rest
        assert!(haps[1].value.is_rest(), "Second hap should be a rest");
    }

    #[test]
    fn test_seq_value_degrade_in_pattern() {
        use crate::pattern_system::Fraction;
        use crate::pattern_system::mini::parse;

        // SeqValue should allow degrade (?) in patterns
        let pattern: crate::pattern_system::Pattern<SeqValue> =
            parse("c4?").expect("Should parse with degrade");

        // Query multiple times - should always get a hap (either note or rest)
        for i in 0..10 {
            let haps = pattern.query_arc(Fraction::from(i), Fraction::from(i + 1));
            assert_eq!(
                haps.len(),
                1,
                "Should always have exactly 1 hap at cycle {}",
                i
            );
        }
    }

    #[test]
    fn test_seq_value_euclidean_in_pattern() {
        use crate::pattern_system::Fraction;
        use crate::pattern_system::mini::parse;

        // SeqValue should allow euclidean in patterns
        let pattern: crate::pattern_system::Pattern<SeqValue> =
            parse("c4(3,8)").expect("Should parse with euclidean");

        let haps = pattern.query_arc(Fraction::from(0), Fraction::from(1));

        // Should have 8 haps (3 notes + 5 rests)
        assert_eq!(haps.len(), 8, "Should have 8 haps (euclidean 3,8)");

        // Count pulses (non-rests)
        let pulse_count = haps.iter().filter(|h| !h.value.is_rest()).count();
        assert_eq!(pulse_count, 3, "Should have 3 pulses");
    }
}
