//! SeqValue enum and SeqPatternParam for the new pattern-based sequencer.
//!
//! SeqValue represents the different value types that can appear in a sequence:
//! - Voltage values (V/Oct, pre-converted at parse time)
//! - Rests

use std::sync::Arc;

use deserr::{DeserializeError, ErrorKind, IntoValue, ValuePointerRef};
use schemars::JsonSchema;

use crate::{
    Patch,
    dsp::utils::midi_to_voct_f64,
    pattern_system::{
        DspHap, Pattern,
        mini::{
            FromMiniAtom,
            ast::AtomValue,
            convert::{ConvertError, HasRest},
        },
    },
    types::Connect,
};

/// A value in a sequence pattern.
///
/// Represents the different types of values that can be sequenced:
/// - Voltage (V/Oct, pre-converted from MIDI/note at parse time)
/// - Rests (silence/no output)
#[derive(Clone, Debug)]
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
                // Treat number as MIDI note, convert to voltage at parse time
                Ok(SeqValue::Voltage(midi_to_voct_f64(*n)))
            }
            AtomValue::Midi(m) => {
                // Convert MIDI to voltage at parse time
                Ok(SeqValue::Voltage(midi_to_voct_f64(*m as f64)))
            }
            AtomValue::Hz(hz) => {
                // Convert Hz to MIDI then to voltage
                let midi = 12.0 * (hz / 440.0).log2() + 69.0;
                Ok(SeqValue::Voltage(midi_to_voct_f64(midi)))
            }
            AtomValue::Volts(v) => {
                // Direct voltage value
                Ok(SeqValue::Voltage(*v))
            }
            AtomValue::Note {
                letter,
                accidental,
                octave,
            } => {
                // Convert note to voltage at parse time
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
            AtomValue::Identifier(s) => {
                // Otherwise treat as note without octave if single letter
                if s.len() == 1 {
                    let c = s.chars().next().unwrap().to_ascii_lowercase();
                    if ('a'..='g').contains(&c)
                        && let Some(midi) = note_to_midi(c, None, None)
                    {
                        return Ok(SeqValue::Voltage(midi_to_voct_f64(midi)));
                    }
                }
                Err(ConvertError::InvalidAtom(format!(
                    "Cannot convert '{}' to SeqValue",
                    s
                )))
            }
            AtomValue::String(s) => Err(ConvertError::InvalidAtom(format!(
                "Cannot convert string '{}' to SeqValue",
                s
            ))),
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

/// A pattern parameter that wraps a pattern string and its parsed components.
///
/// This struct is serialized as a simple string but contains the parsed pattern
/// and cached query results.
#[derive(Clone, Default, JsonSchema, Debug)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct SeqPatternParam {
    /// The source pattern string (used for serialization)
    #[allow(dead_code)]
    source: String,

    /// The parsed pattern (skipped in serialization)
    #[serde(skip, default)]
    #[schemars(skip)]
    pub(crate) pattern: Option<Pattern<SeqValue>>,

    /// All leaf spans in the pattern (character offsets within the pattern string).
    /// Computed once at parse time for creating Monaco tracked decorations.
    ///
    /// These differ from the "spans" returned in module state:
    /// - `all_spans`: All pattern leaves, used to create decorations that track edits
    /// - `spans` (in get_state): Currently active/playing spans, used for highlighting
    #[serde(skip, default)]
    #[schemars(skip)]
    pub(crate) all_spans: Vec<(usize, usize)>,

    /// Pre-computed haps for cycles 0..999, populated at parse time.
    /// Each element is an Arc-wrapped Vec of all haps intersecting that cycle.
    #[serde(skip, default)]
    #[schemars(skip)]
    pub(crate) cached_haps: Vec<Arc<Vec<DspHap<SeqValue>>>>,
}

impl SeqPatternParam {
    /// Parse a pattern string.
    fn parse(source: &str) -> Result<Self, String> {
        // Parse mini notation AST first (for span collection)
        let ast = crate::pattern_system::mini::parse_ast(source).map_err(|e| e.to_string())?;

        // Collect all leaf spans from AST
        let all_spans = crate::pattern_system::mini::collect_leaf_spans(&ast);

        // Convert AST to pattern
        let pattern =
            crate::pattern_system::mini::convert::<SeqValue>(&ast).map_err(|e| e.to_string())?;

        // Pre-compute and cache haps for cycles 0..999
        let cached_haps: Vec<Arc<Vec<DspHap<SeqValue>>>> = (0..1000)
            .map(|cycle| Arc::new(pattern.query_cycle_all(cycle)))
            .collect();

        Ok(Self {
            source: source.to_string(),
            pattern: Some(pattern),
            all_spans,
            cached_haps,
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

    /// Get the pre-computed cached haps for cycles 0..999.
    pub fn cached_haps(&self) -> &[Arc<Vec<DspHap<SeqValue>>>] {
        &self.cached_haps
    }
}

// deserr implementation for SeqPatternParam - transparent string wrapper that parses.
impl<E: DeserializeError> deserr::Deserr<E> for SeqPatternParam {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        let source = String::deserialize_from_value(value, location)?;
        if source.is_empty() {
            return Ok(Self::default());
        }
        Self::parse(&source).map_err(|e| {
            deserr::take_cf_content(E::error::<V>(
                None,
                ErrorKind::Unexpected { msg: e },
                location,
            ))
        })
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
        // Number is treated as MIDI and converted to voltage
        let n = SeqValue::from_atom(&AtomValue::Number(60.0)).unwrap();
        let expected_voltage = midi_to_voct_f64(60.0);
        assert!(matches!(n, SeqValue::Voltage(v) if (v - expected_voltage).abs() < 0.001));

        // Note is converted to voltage at parse time
        let note = SeqValue::from_atom(&AtomValue::Note {
            letter: 'a',
            accidental: None,
            octave: Some(4),
        })
        .unwrap();
        let expected_a4_voltage = midi_to_voct_f64(69.0); // A4 = MIDI 69
        assert!(matches!(note, SeqValue::Voltage(v) if (v - expected_a4_voltage).abs() < 0.001));

        assert!(
            SeqValue::from_atom(&AtomValue::Identifier("module(src:output:0)".to_string()))
                .is_err()
        );
        assert!(
            SeqValue::from_atom(&AtomValue::String("module(src:output:0)".to_string())).is_err()
        );
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
