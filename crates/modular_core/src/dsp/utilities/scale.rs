//! Scale and chord specifications.
//!
//! A spec string names a set of pitches relative to a root:
//! - `root(…)` — a scale: a named scale, a tuning keyword, or custom intervals
//!   (optionally prefixed by a tuning keyword). Intervals reduce to pitch
//!   classes within one octave.
//! - `root[…]` — a chord: a chord shorthand or custom voicing intervals that
//!   keep their register, optionally prefixed by a tuning keyword and suffixed
//!   by an inversion (`invN`).
//! - `chromatic` — all 12 semitones from C.
//!
//! The root is a note letter, optional accidental and optional octave
//! (`C`, `Db3`, `f#5`); the octave defaults to 4, so `C` is C4 = 0 V.

use arrayvec::ArrayVec;
use deserr::{DeserializeError, ErrorKind, IntoValue, ValuePointerRef};

use crate::Patch;
use crate::poly::PORT_MAX_CHANNELS;
use crate::types::Connect;

use super::{chord_names, scale_names};

/// A fixed scale root (note letter + optional accidental + optional octave).
#[derive(Clone, Debug, PartialEq)]
pub struct FixedRoot {
    pub letter: char,
    pub accidental: Option<char>,
    pub octave: Option<i8>,
}

impl FixedRoot {
    /// Parse from a string like "c", "c#", "bb", "c3", "c#4", "db3".
    /// The optional octave number follows the note letter and accidental.
    pub fn parse(s: &str) -> Option<Self> {
        // Note names are always ASCII; index by byte position directly.
        let bytes = s.as_bytes();
        if bytes.is_empty() {
            return None;
        }

        let letter = (bytes[0] as char).to_ascii_lowercase();
        if !('a'..='g').contains(&letter) {
            return None;
        }

        let mut idx = 1;
        let accidental = if idx < bytes.len() {
            match bytes[idx] as char {
                '#' | 's' => {
                    idx += 1;
                    Some('#')
                }
                'b' | 'f' => {
                    idx += 1;
                    Some('b')
                }
                _ => None,
            }
        } else {
            None
        };

        let octave = if idx < bytes.len() {
            // idx has only advanced past single-byte ASCII chars, so s[idx..] is valid UTF-8.
            Some(s[idx..].parse::<i8>().ok()?)
        } else {
            None
        };

        Some(Self {
            letter,
            accidental,
            octave,
        })
    }

    /// Get the pitch class (0-11, C=0).
    pub fn pitch_class(&self) -> i8 {
        let base = match self.letter {
            'c' => 0,
            'd' => 2,
            'e' => 4,
            'f' => 5,
            'g' => 7,
            'a' => 9,
            'b' => 11,
            _ => 0,
        };

        let acc = match self.accidental {
            Some('#') => 1,
            Some('b') => -1,
            _ => 0,
        };

        ((base + acc) % 12 + 12) as i8 % 12
    }

    /// Get the base MIDI note number.
    ///
    /// If an octave is specified, returns the MIDI note for that root+octave
    /// (e.g. C3 = 48, D4 = 62). If no octave, defaults to octave 4 (C4 = 60).
    pub fn base_midi(&self) -> i32 {
        let pc = self.pitch_class() as i32;
        match self.octave {
            Some(oct) => (oct as i32 + 1) * 12 + pc,
            None => 60 + pc,
        }
    }
}

/// 5-limit just intonation, 12 tones, ratios relative to the root.
const JUST_RATIOS: [f64; 12] = [
    1.0,
    16.0 / 15.0,
    9.0 / 8.0,
    6.0 / 5.0,
    5.0 / 4.0,
    4.0 / 3.0,
    45.0 / 32.0,
    3.0 / 2.0,
    8.0 / 5.0,
    5.0 / 3.0,
    9.0 / 5.0,
    15.0 / 8.0,
];

/// Pythagorean tuning, 12 tones, ratios relative to the root.
const PYTHAGOREAN_RATIOS: [f64; 12] = [
    1.0,
    256.0 / 243.0,
    9.0 / 8.0,
    32.0 / 27.0,
    81.0 / 64.0,
    4.0 / 3.0,
    729.0 / 512.0,
    3.0 / 2.0,
    128.0 / 81.0,
    27.0 / 16.0,
    16.0 / 9.0,
    243.0 / 128.0,
];

/// 12-tone equal temperament tuning: each step is an exact 1/12 V.
fn et_tuning() -> [f64; 12] {
    std::array::from_fn(|i| i as f64 / 12.0)
}

/// Convert a table of frequency ratios into V/Oct offsets (`log2` of each ratio).
fn tuning_from_ratios(ratios: &[f64; 12]) -> [f64; 12] {
    std::array::from_fn(|i| ratios[i].log2())
}

/// Look up a tuning table by keyword. Returns `None` for unrecognized names.
///
/// Recognized: `chromatic` (12-TET), `just` (5-limit just intonation),
/// `pythagorean` / `pythag` (Pythagorean tuning).
fn named_tuning(name: &str) -> Option<[f64; 12]> {
    // ASCII case-insensitive; does not allocate.
    if name.eq_ignore_ascii_case("chromatic") {
        Some(et_tuning())
    } else if name.eq_ignore_ascii_case("just") {
        Some(tuning_from_ratios(&JUST_RATIOS))
    } else if name.eq_ignore_ascii_case("pythagorean") || name.eq_ignore_ascii_case("pythag") {
        Some(tuning_from_ratios(&PYTHAGOREAN_RATIOS))
    } else {
        None
    }
}

const CHROMATIC: [i8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

/// A parsed scale or chord spec: a root, a tuning, and an ascending voicing.
///
/// Parsed on the main thread; holds no heap data, so it is cheap to clone into
/// pattern closures.
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleSpec {
    /// MIDI note of the root (degree 0 at interval 0).
    base_midi: i32,
    /// V/Oct offset of each chromatic step above the root (index 0-11).
    tuning: [f64; 12],
    /// Semitone offsets from the root, strictly ascending. Scales lie within
    /// `0..12`; chords may span several octaves or omit the root.
    voicing: ArrayVec<i8, PORT_MAX_CHANNELS>,
    /// Semitones between repeats of the voicing when indexing degrees: the
    /// voicing's span rounded up to whole octaves.
    period_semitones: i32,
    /// V/Oct voltage of each voicing note, ascending.
    voltages: ArrayVec<f32, PORT_MAX_CHANNELS>,
}

impl ScaleSpec {
    /// Parse a spec string (see the module docs for the grammar).
    pub fn parse(source: &str) -> Result<Self, String> {
        let source = source.trim();

        if source.is_empty() {
            return Err("scale spec is empty".to_string());
        }

        if source.eq_ignore_ascii_case("chromatic") {
            return Self::new(60, et_tuning(), &CHROMATIC);
        }

        let (open, close, is_chord) = match source.find(['(', '[']) {
            Some(i) if source.as_bytes()[i] == b'(' => (i, ')', false),
            Some(i) => (i, ']', true),
            None => {
                return Err(
                    "expected root(scale), root[chord] or \"chromatic\" (e.g. \"C(major)\", \"C3[maj7]\")"
                        .to_string(),
                );
            }
        };
        if !source.ends_with(close) {
            return Err(format!("missing closing '{close}'"));
        }

        let root_str = &source[..open];
        let root = FixedRoot::parse(root_str)
            .ok_or_else(|| format!("invalid root note \"{root_str}\""))?;
        let body = &source[open + 1..source.len() - close.len_utf8()];

        if is_chord {
            Self::parse_chord(&root, body)
        } else {
            Self::parse_scale(&root, body)
        }
    }

    /// `root(…)`: named scale, tuning keyword, or `[tuning] intervals…`.
    fn parse_scale(root: &FixedRoot, body: &str) -> Result<Self, String> {
        let body = body.trim();
        let base_midi = root.base_midi();

        if let Some(tuning) = named_tuning(body) {
            return Self::new(base_midi, tuning, &CHROMATIC);
        }
        if let Some(intervals) = scale_names::lookup(body) {
            return Self::new(base_midi, et_tuning(), intervals);
        }

        let mut tokens = body.split_whitespace().peekable();
        let tuning = match tokens.peek().and_then(|t| named_tuning(t)) {
            Some(tuning) => {
                tokens.next();
                tuning
            }
            None => et_tuning(),
        };

        // Custom intervals reduce to pitch classes; the root is always a degree.
        let mut pitch_classes = ArrayVec::<i8, 12>::new();
        pitch_classes.push(0);
        let mut any = false;
        for token in tokens {
            let interval = token.parse::<i8>().map_err(|_| {
                format!("unknown scale \"{body}\" (\"{token}\" is not a scale name or interval)")
            })?;
            any = true;
            let pc = interval.rem_euclid(12);
            if !pitch_classes.contains(&pc) {
                pitch_classes.push(pc);
            }
        }
        if !any {
            return Err(format!("scale \"{body}\" has no intervals"));
        }
        pitch_classes.sort_unstable();
        Self::new(base_midi, tuning, &pitch_classes)
    }

    /// `root[…]`: `[tuning] (chord-name | intervals…) [invN]`.
    fn parse_chord(root: &FixedRoot, body: &str) -> Result<Self, String> {
        let mut tokens: ArrayVec<&str, { PORT_MAX_CHANNELS + 2 }> = ArrayVec::new();
        for token in body.split_whitespace() {
            tokens
                .try_push(token)
                .map_err(|_| format!("chord has more than {PORT_MAX_CHANNELS} notes"))?;
        }

        let mut tuning = et_tuning();
        if let Some(t) = tokens.first().and_then(|t| named_tuning(t)) {
            tuning = t;
            tokens.remove(0);
        }

        let mut inversion = 0usize;
        if let Some(n) = tokens.last().and_then(|t| t.strip_prefix("inv")) {
            inversion = n
                .parse()
                .map_err(|_| format!("invalid inversion \"inv{n}\" (expected inv1, inv2, …)"))?;
            tokens.pop();
        }

        let mut voicing = ArrayVec::<i8, PORT_MAX_CHANNELS>::new();
        match tokens.as_slice() {
            [] => return Err("chord is empty".to_string()),
            [name] if chord_names::lookup(name).is_some() => {
                voicing.extend(chord_names::lookup(name).unwrap().iter().copied());
            }
            _ => {
                for token in &tokens {
                    let interval = token.parse::<i8>().map_err(|_| {
                        format!(
                            "unknown chord \"{body}\" (\"{token}\" is not a chord name or interval)"
                        )
                    })?;
                    if !voicing.contains(&interval) {
                        voicing.try_push(interval).map_err(|_| {
                            format!("chord has more than {PORT_MAX_CHANNELS} notes")
                        })?;
                    }
                }
                voicing.sort_unstable();
            }
        }

        if inversion >= voicing.len() {
            return Err(format!(
                "inversion inv{inversion} needs more than {} chord notes",
                voicing.len()
            ));
        }
        for interval in &mut voicing[..inversion] {
            *interval = interval
                .checked_add(12)
                .ok_or_else(|| "inverted interval out of range".to_string())?;
        }
        voicing.sort_unstable();
        voicing.dedup_sorted();

        Self::new(root.base_midi(), tuning, &voicing)
    }

    fn new(base_midi: i32, tuning: [f64; 12], voicing: &[i8]) -> Result<Self, String> {
        let (Some(&min), Some(&max)) = (voicing.first(), voicing.last()) else {
            return Err("scale spec has no notes".to_string());
        };
        let span = (max as i32 - min as i32) + 1;
        let mut spec = Self {
            base_midi,
            tuning,
            voicing: voicing.iter().copied().collect(),
            period_semitones: (span + 11) / 12 * 12,
            voltages: ArrayVec::new(),
        };
        spec.voltages = voicing
            .iter()
            .map(|&n| spec.interval_voltage(n as i32) as f32)
            .collect();
        Ok(spec)
    }

    /// V/Oct voltage of a semitone offset from the root, applying the tuning.
    fn interval_voltage(&self, semitones: i32) -> f64 {
        let root_v = (self.base_midi - 60) as f64 / 12.0;
        root_v + semitones.div_euclid(12) as f64 + self.tuning[semitones.rem_euclid(12) as usize]
    }

    /// V/Oct voltage of each voicing note, ascending.
    pub fn voltages(&self) -> &[f32] {
        &self.voltages
    }

    /// V/Oct voltage of a signed degree. Degrees index the voicing and wrap by
    /// `period_semitones` (one octave for scales) in both directions.
    pub fn degree_voltage(&self, degree: i32) -> f64 {
        let len = self.voicing.len() as i32;
        let period = degree.div_euclid(len);
        let note = self.voicing[degree.rem_euclid(len) as usize] as i32;
        self.interval_voltage(note) + (period * self.period_semitones / 12) as f64
    }
}

trait DedupSorted {
    fn dedup_sorted(&mut self);
}

impl<const N: usize> DedupSorted for ArrayVec<i8, N> {
    fn dedup_sorted(&mut self) {
        let mut write = 0usize;
        for read in 0..self.len() {
            if write == 0 || self[read] != self[write - 1] {
                self[write] = self[read];
                write += 1;
            }
        }
        self.truncate(write);
    }
}

impl Connect for ScaleSpec {
    fn apply_default_connections(&mut self) {}
    fn connect(&mut self, _patch: &Patch) {}
    fn collect_cables(&self, _sink: &mut Vec<String>) {}
    fn inject_index_ptr(&mut self, _ptr: *const std::cell::Cell<usize>) {}
}

impl schemars::JsonSchema for ScaleSpec {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ScaleSpec")
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }
}

impl<E: DeserializeError> deserr::Deserr<E> for ScaleSpec {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        let source = String::deserialize_from_value(value, location)?;
        Self::parse(&source).map_err(|reason| {
            deserr::take_cf_content(E::error::<V>(
                None,
                ErrorKind::Unexpected {
                    msg: format!("Invalid scale specification \"{source}\": {reason}"),
                },
                location,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voltages(spec: &str) -> Vec<f64> {
        ScaleSpec::parse(spec)
            .unwrap()
            .voltages()
            .iter()
            .map(|&v| v as f64)
            .collect()
    }

    fn semitones(spec: &str) -> Vec<f64> {
        voltages(spec).iter().map(|v| (v * 12.0).round()).collect()
    }

    #[test]
    fn test_fixed_root_parse() {
        let c = FixedRoot::parse("c").unwrap();
        assert_eq!(c.letter, 'c');
        assert_eq!(c.accidental, None);
        assert_eq!(c.octave, None);

        let cs = FixedRoot::parse("c#").unwrap();
        assert_eq!(cs.accidental, Some('#'));

        let bb = FixedRoot::parse("bb").unwrap();
        assert_eq!(bb.letter, 'b');
        assert_eq!(bb.accidental, Some('b'));
    }

    #[test]
    fn test_fixed_root_parse_with_octave() {
        assert_eq!(FixedRoot::parse("c3").unwrap().base_midi(), 48);
        assert_eq!(FixedRoot::parse("c#4").unwrap().base_midi(), 61);
        assert_eq!(FixedRoot::parse("db3").unwrap().base_midi(), 49);
        assert_eq!(FixedRoot::parse("b5").unwrap().base_midi(), 83);
    }

    #[test]
    fn test_fixed_root_pitch_class() {
        assert_eq!(FixedRoot::parse("c").unwrap().pitch_class(), 0);
        assert_eq!(FixedRoot::parse("c#").unwrap().pitch_class(), 1);
        assert_eq!(FixedRoot::parse("d").unwrap().pitch_class(), 2);
        assert_eq!(FixedRoot::parse("a").unwrap().pitch_class(), 9);
        assert_eq!(FixedRoot::parse("b").unwrap().pitch_class(), 11);
    }

    #[test]
    fn named_scales() {
        assert_eq!(semitones("C(major)"), [0.0, 2.0, 4.0, 5.0, 7.0, 9.0, 11.0]);
        assert_eq!(semitones("A3(min)"), [-3.0, -1.0, 0.0, 2.0, 4.0, 5.0, 7.0]);
        assert_eq!(semitones("d(M)"), [2.0, 4.0, 6.0, 7.0, 9.0, 11.0, 13.0]);
        assert_eq!(semitones("C(Harmonic Minor)").len(), 7);
        assert_eq!(semitones("chromatic").len(), 12);
        assert_eq!(semitones("C(chromatic)").len(), 12);
    }

    #[test]
    fn custom_scale_intervals_reduce_to_pitch_classes() {
        assert_eq!(semitones("D(0 2 4 5 7 9 11)").len(), 7);
        // The root is always included; intervals wrap into one octave.
        assert_eq!(semitones("C(4 7 14)"), [0.0, 2.0, 4.0, 7.0]);
    }

    #[test]
    fn just_and_pythagorean_tunings() {
        let just = voltages("C(just)");
        assert!((just[4] - 1.25_f64.log2()).abs() < 1e-6);
        assert!((just[7] - 1.5_f64.log2()).abs() < 1e-6);
        let pyth = voltages("C(pythag)");
        assert!((pyth[4] - (81.0_f64 / 64.0).log2()).abs() < 1e-6);
        // Tuning prefix on custom intervals.
        let tuned = voltages("C(just 0 3 4 8)");
        assert!((tuned[2] - 1.25_f64.log2()).abs() < 1e-6);
        // Root offset: the just fifth above D.
        let d = voltages("D(just)");
        assert!((d[7] - (2.0 / 12.0 + 1.5_f64.log2())).abs() < 1e-6);
    }

    #[test]
    fn chord_names_and_octaves() {
        assert_eq!(semitones("C[maj]"), [0.0, 4.0, 7.0]);
        assert_eq!(semitones("C3[maj7]"), [-12.0, -8.0, -5.0, -1.0]);
        assert_eq!(semitones("A[m7]"), [9.0, 12.0, 16.0, 19.0]);
        assert_eq!(semitones("C[9]"), [0.0, 4.0, 7.0, 10.0, 14.0]);
    }

    #[test]
    fn chord_voicing_intervals_keep_register() {
        assert_eq!(semitones("C[0 7 16]"), [0.0, 7.0, 16.0]);
        assert_eq!(semitones("C[16 0 7]"), [0.0, 7.0, 16.0]);
        assert_eq!(semitones("C[-12 0 4 7]"), [-12.0, 0.0, 4.0, 7.0]);
    }

    #[test]
    fn chord_tuning_prefix_and_inversion() {
        let just = voltages("C[just maj]");
        assert!((just[1] - 1.25_f64.log2()).abs() < 1e-6);
        assert_eq!(semitones("C[maj inv1]"), [4.0, 7.0, 12.0]);
        assert_eq!(semitones("C[maj7 inv2]"), [7.0, 11.0, 12.0, 16.0]);
        let pyth = voltages("C[pythag 0 4 7 inv1]");
        assert!((pyth[0] - (81.0_f64 / 64.0).log2()).abs() < 1e-6);
        assert!((pyth[2] - 1.0).abs() < 1e-6);
        // A single numeric token is a chord name, not an interval.
        assert_eq!(semitones("C[7]"), [0.0, 4.0, 7.0, 10.0]);
    }

    #[test]
    fn invalid_specs_are_rejected() {
        for bad in [
            "",
            "major",
            "C(maj7)",
            "C(major foo)",
            "C(M anything)",
            "C[maj",
            "C(major",
            "C[maj8]",
            "C[]",
            "C[maj inv3]",
            "C[maj invx]",
            "H(major)",
            "C()",
        ] {
            assert!(ScaleSpec::parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn degree_voltage_wraps_by_period() {
        let major = ScaleSpec::parse("C(major)").unwrap();
        assert!((major.degree_voltage(7) - 1.0).abs() < 1e-9);
        assert!((major.degree_voltage(-1) - (-1.0 / 12.0)).abs() < 1e-9);
        assert!((major.degree_voltage(-7) - (-1.0)).abs() < 1e-9);

        // A 9th chord spans more than an octave, so it repeats every two.
        let ninth = ScaleSpec::parse("C[9]").unwrap();
        assert!((ninth.degree_voltage(4) - 14.0 / 12.0).abs() < 1e-9);
        assert!((ninth.degree_voltage(5) - 2.0).abs() < 1e-9);
        assert!((ninth.degree_voltage(-1) - (14.0 / 12.0 - 2.0)).abs() < 1e-9);

        // An inverted triad repeats every octave from its lowest note.
        let inv = ScaleSpec::parse("C[maj inv1]").unwrap();
        assert!((inv.degree_voltage(3) - 16.0 / 12.0).abs() < 1e-9);
    }
}
