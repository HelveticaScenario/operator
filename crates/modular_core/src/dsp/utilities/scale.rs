//! Scale snapping infrastructure for quantizers.
//!
//! This module provides:
//! - `ScaleSnapper`: A lookup table for snapping MIDI notes to a scale
//! - Scale type validation and known scale types

use std::fmt::Write as FmtWrite;

use arrayvec::{ArrayString, ArrayVec};
use rust_music_theory::note::{Note, Notes, Pitch};
use rust_music_theory::scale::Scale;

/// A fixed scale root (note letter + optional accidental + optional octave).
#[derive(Clone, Debug, PartialEq)]
pub struct FixedRoot {
    pub letter: char,
    pub accidental: Option<char>,
    pub octave: Option<i8>,
}

impl FixedRoot {
    /// Create a new fixed root.
    pub fn new(letter: char, accidental: Option<char>) -> Self {
        Self {
            letter,
            accidental,
            octave: None,
        }
    }

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

    /// Convert to rust_music_theory Pitch.
    pub fn to_pitch(&self) -> Option<Pitch> {
        // At most 2 chars: letter + optional accidental.
        let mut pitch_str = ArrayString::<2>::new();
        pitch_str.push(self.letter.to_ascii_uppercase());
        if let Some(acc) = self.accidental {
            pitch_str.push(acc);
        }
        Pitch::from_str(pitch_str.as_str())
    }
}

/// Remove consecutive duplicates from a sorted `ArrayVec<i8, 12>` in-place.
///
/// Equivalent to `[T]::dedup()` but works around the auto-deref resolution
/// issue with unsized `[T]` receivers in the 2024 edition.
fn dedup_sorted(v: &mut ArrayVec<i8, 12>) {
    if v.len() <= 1 {
        return;
    }
    let mut write = 1usize;
    for read in 1..v.len() {
        if v[read] != v[write - 1] {
            v[write] = v[read];
            write += 1;
        }
    }
    v.truncate(write);
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
pub fn et_tuning() -> [f64; 12] {
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
pub fn named_tuning(name: &str) -> Option<[f64; 12]> {
    // ASCII case-insensitive compares — no allocation, unlike `to_lowercase`.
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

/// A scale snapper with precomputed lookup table for fast MIDI→scale snapping.
///
/// The `snap_table` contains 13 entries (0-12 inclusive, where 12 wraps to next octave):
/// - Index 0 = offset for pitch class at root
/// - Index 1 = offset for pitch class 1 semitone above root
/// - ...up to index 12 = octave boundary handling
///
/// Each table entry is the signed offset to the nearest scale degree.
/// When equidistant, prefers the lower pitch.
#[derive(Clone, Debug)]
pub struct ScaleSnapper {
    /// Snap offsets for each chromatic pitch relative to root (0-12).
    /// Value is the signed semitone offset to snap to the nearest scale tone.
    snap_table: [i8; 13],

    /// Root offset in semitones (C=0, C#=1, ..., B=11).
    root_offset: i8,

    /// The scale type name (for reference).
    scale_name: ArrayString<64>,

    /// Scale intervals (semitones from root for each scale degree).
    scale_intervals: ArrayVec<i8, 12>,

    /// V/Oct offset of each chromatic step above the root (index 0-11).
    /// 12-TET by default; non-equal for just / Pythagorean tunings.
    tuning: [f64; 12],
}

impl ScaleSnapper {
    /// Build a ScaleSnapper from a scale type name and root.
    ///
    /// # Arguments
    /// * `root` - The root note of the scale
    /// * `scale_name` - The scale type (e.g., "major", "minor", "dorian")
    ///
    /// # Returns
    /// `Some(ScaleSnapper)` if the scale is valid, `None` otherwise.
    pub fn new(root: &FixedRoot, scale_name: &str) -> Option<Self> {
        // "chromatic", "just", "pythagorean" / "pythag" all keep every chromatic
        // step; they differ only in the per-step tuning table.
        if let Some(tuning) = named_tuning(scale_name) {
            let mut name = ArrayString::<64>::new();
            name.push_str(scale_name);
            let mut intervals = ArrayVec::<i8, 12>::new();
            for i in 0i8..12 {
                intervals.push(i);
            }
            return Some(Self {
                snap_table: [0; 13],
                root_offset: root.pitch_class(),
                scale_name: name,
                scale_intervals: intervals,
                tuning,
            });
        }

        let pitch = root.to_pitch()?;
        let root_note = Note::new(pitch, 4); // Octave doesn't matter for interval calculation

        // Build scale definition string — at most ~20 chars (pitch 1-2, space, name ≤15).
        let mut scale_def = ArrayString::<64>::new();
        write!(scale_def, "{} {}", root_note.pitch, scale_name).ok()?;
        let scale = Scale::from_regex(scale_def.as_str()).ok()?;

        let notes = scale.notes();
        if notes.is_empty() {
            return None;
        }

        // Build set of scale degrees (pitch classes relative to root).
        // A scale has at most 12 distinct degrees.
        let root_pc = root.pitch_class();
        let mut scale_degrees = ArrayVec::<i8, 12>::new();
        for n in notes.iter().take(12) {
            let pc = n.pitch.into_u8() as i8;
            scale_degrees.push(((pc - root_pc) % 12 + 12) % 12);
        }

        // Remove duplicates and sort.
        scale_degrees.sort();
        dedup_sorted(&mut scale_degrees);

        // degrees_with_octave = scale_degrees + boundary 12.
        let mut degrees_with_octave = ArrayVec::<i8, 13>::new();
        degrees_with_octave.extend(scale_degrees.iter().copied());
        degrees_with_octave.push(12);

        // degrees_extended = degrees_with_octave + (scale_degrees shifted down an octave).
        let mut degrees_extended = ArrayVec::<i8, 25>::new();
        degrees_extended.extend(degrees_with_octave.iter().copied());
        for &d in &scale_degrees {
            degrees_extended.push(d - 12);
        }
        degrees_extended.sort();

        // Build snap table: for each chromatic pitch (0-12), find nearest scale degree.
        let mut snap_table = [0i8; 13];
        for chromatic in 0..=12 {
            let mut best_offset = 0i8;
            let mut best_dist = i8::MAX;

            for &degree in &degrees_extended {
                let offset = degree - chromatic;
                let dist = offset.abs();

                if dist < best_dist || (dist == best_dist && offset < 0) {
                    best_dist = dist;
                    best_offset = offset;
                }
            }

            snap_table[chromatic as usize] = best_offset;
        }

        let root_offset = root.pitch_class();
        let mut name = ArrayString::<64>::new();
        name.push_str(scale_name);

        Some(Self {
            snap_table,
            root_offset,
            scale_name: name,
            scale_intervals: scale_degrees,
            tuning: et_tuning(),
        })
    }

    /// Build a ScaleSnapper from custom intervals (0-11) with a given tuning.
    ///
    /// # Arguments
    /// * `root` - The root note of the scale
    /// * `intervals` - Slice of semitone offsets from root (0 = root, 2 = major second, etc.)
    /// * `tuning` - V/Oct offset of each chromatic step (use [`et_tuning`] for 12-TET)
    ///
    /// # Returns
    /// A ScaleSnapper configured for the custom scale.
    pub fn from_intervals(root: &FixedRoot, intervals: &[i8], tuning: [f64; 12]) -> Self {
        let root_pc = root.pitch_class();

        // Normalize intervals to 0-11 range, take at most 12, then deduplicate.
        let mut scale_degrees = ArrayVec::<i8, 12>::new();
        for &i in intervals.iter().take(12) {
            scale_degrees.push(((i % 12) + 12) % 12);
        }
        scale_degrees.sort();
        dedup_sorted(&mut scale_degrees);

        // Ensure root is included.
        if !scale_degrees.contains(&0) {
            scale_degrees.insert(0, 0);
        }

        // degrees_with_octave = scale_degrees + boundary 12.
        let mut degrees_with_octave = ArrayVec::<i8, 13>::new();
        degrees_with_octave.extend(scale_degrees.iter().copied());
        degrees_with_octave.push(12);

        // degrees_extended = degrees_with_octave + (scale_degrees shifted down an octave).
        let mut degrees_extended = ArrayVec::<i8, 25>::new();
        degrees_extended.extend(degrees_with_octave.iter().copied());
        for &d in &scale_degrees {
            degrees_extended.push(d - 12);
        }
        degrees_extended.sort();

        // Build snap table.
        let mut snap_table = [0i8; 13];
        for chromatic in 0..=12 {
            let mut best_offset = 0i8;
            let mut best_dist = i8::MAX;

            for &degree in &degrees_extended {
                let offset = degree - chromatic;
                let dist = offset.abs();

                if dist < best_dist || (dist == best_dist && offset < 0) {
                    best_dist = dist;
                    best_offset = offset;
                }
            }

            snap_table[chromatic as usize] = best_offset;
        }

        let mut name = ArrayString::<64>::new();
        name.push_str("custom");

        Self {
            snap_table,
            root_offset: root_pc,
            scale_name: name,
            scale_intervals: scale_degrees,
            tuning,
        }
    }

    /// Snap a MIDI note to the nearest scale degree.
    ///
    /// # Arguments
    /// * `midi` - The MIDI note number (can be fractional)
    ///
    /// # Returns
    /// The snapped MIDI note number (always an exact integer — no fractional cents).
    pub fn snap_midi(&self, midi: f64) -> f64 {
        // Round to nearest semitone so e.g. 59.9 resolves as C4 (60) not B3 (59)
        let midi_int = midi.round() as i32;

        // Convert MIDI to pitch class (C=0, C#=1, ..., B=11)
        // MIDI 60 = C4, so midi % 12 gives pitch class with C=0
        let midi_pc = ((midi_int % 12) + 12) % 12;

        // Convert to position relative to scale root
        let pc_in_scale = ((midi_pc - self.root_offset as i32) % 12 + 12) % 12;

        // Look up snap offset
        let snap_offset = self.snap_table[pc_in_scale as usize];

        // Apply snap — return clean integer MIDI note (no cents)
        (midi_int + snap_offset as i32) as f64
    }

    /// Snap a V/Oct voltage to the nearest scale degree.
    ///
    /// # Arguments
    /// * `voct` - V/Oct voltage (C4 = 0V)
    ///
    /// # Returns
    /// The snapped V/Oct voltage.
    pub fn snap_voct(&self, voct: f64) -> f64 {
        // Convert V/Oct to MIDI
        let midi = voct * 12.0 + 60.0;
        // Snap in MIDI domain (12-TET nearest semitone)
        let snapped_midi = self.snap_midi(midi);
        // Convert back to V/Oct, applying the tuning table
        self.tuned_voct(snapped_midi as i32)
    }

    /// Convert an integer 12-TET MIDI note to V/Oct using this scale's tuning.
    ///
    /// For 12-TET this is identity with `(midi - 60) / 12`; for just /
    /// Pythagorean tunings each chromatic step is offset by its ratio.
    fn tuned_voct(&self, midi_int: i32) -> f64 {
        let root = self.root_offset as i32;
        let pc = ((midi_int - root) % 12 + 12) % 12;
        let octave = (midi_int - 60 - root - pc) / 12;
        root as f64 / 12.0 + octave as f64 + self.tuning[pc as usize]
    }

    /// Check if a MIDI note is in the scale.
    pub fn is_in_scale(&self, midi: f64) -> bool {
        let midi_int = midi.round() as i32;
        let midi_pc = ((midi_int % 12) + 12) % 12;
        let pc_in_scale = ((midi_pc - self.root_offset as i32) % 12 + 12) % 12;
        self.snap_table[pc_in_scale as usize] == 0
    }

    /// Get the scale type name.
    pub fn scale_name(&self) -> &str {
        self.scale_name.as_str()
    }

    /// Get the scale intervals (semitone offsets from root for each degree).
    pub fn scale_intervals(&self) -> &ArrayVec<i8, 12> {
        &self.scale_intervals
    }

    /// Get the tuning table: V/Oct offset of each chromatic step above the root.
    pub fn tuning(&self) -> &[f64; 12] {
        &self.tuning
    }

    /// Get the root offset in semitones (C=0, C#=1, ..., B=11).
    pub fn root_offset(&self) -> i8 {
        self.root_offset
    }
}

/// Validate that a scale type name is recognized by rust_music_theory.
///
/// This uses `Scale::from_regex` to validate the scale name. Supported scales include:
/// - Diatonic modes: major/ionian, minor/aeolian, dorian, phrygian, lydian, mixolydian, locrian
/// - Other scales: harmonic minor, melodic minor, pentatonic major/minor, blues, chromatic, whole tone
/// - Abbreviations: maj, min, pent maj, pent min, har minor, mel minor, wholetone, etc.
pub fn validate_scale_type(name: &str) -> bool {
    // Tuning keywords (chromatic / just / pythagorean / pythag) bypass
    // Scale::from_regex in ScaleSnapper::new.
    if named_tuning(name).is_some() {
        return true;
    }

    // Try to parse with a C root - if it works, the scale type is valid
    Scale::from_regex(&format!("C {}", name)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_root_parse() {
        let c = FixedRoot::parse("c").unwrap();
        assert_eq!(c.letter, 'c');
        assert_eq!(c.accidental, None);
        assert_eq!(c.octave, None);

        let cs = FixedRoot::parse("c#").unwrap();
        assert_eq!(cs.letter, 'c');
        assert_eq!(cs.accidental, Some('#'));
        assert_eq!(cs.octave, None);

        let bb = FixedRoot::parse("bb").unwrap();
        assert_eq!(bb.letter, 'b');
        assert_eq!(bb.accidental, Some('b'));
        assert_eq!(bb.octave, None);
    }

    #[test]
    fn test_fixed_root_parse_with_octave() {
        let c3 = FixedRoot::parse("c3").unwrap();
        assert_eq!(c3.letter, 'c');
        assert_eq!(c3.accidental, None);
        assert_eq!(c3.octave, Some(3));
        assert_eq!(c3.base_midi(), 48); // C3

        let cs4 = FixedRoot::parse("c#4").unwrap();
        assert_eq!(cs4.letter, 'c');
        assert_eq!(cs4.accidental, Some('#'));
        assert_eq!(cs4.octave, Some(4));
        assert_eq!(cs4.base_midi(), 61); // C#4

        let db3 = FixedRoot::parse("db3").unwrap();
        assert_eq!(db3.letter, 'd');
        assert_eq!(db3.accidental, Some('b'));
        assert_eq!(db3.octave, Some(3));
        assert_eq!(db3.base_midi(), 49); // Db3

        let b5 = FixedRoot::parse("b5").unwrap();
        assert_eq!(b5.letter, 'b');
        assert_eq!(b5.accidental, None);
        assert_eq!(b5.octave, Some(5));
        assert_eq!(b5.base_midi(), 83); // B5
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
    fn test_scale_snapper_c_major() {
        let root = FixedRoot::parse("c").unwrap();
        let snapper = ScaleSnapper::new(&root, "major").unwrap();

        // C major: C D E F G A B
        // C (60) should stay C
        assert_eq!(snapper.snap_midi(60.0), 60.0);

        // D (62) should stay D
        assert_eq!(snapper.snap_midi(62.0), 62.0);

        // C# (61) should snap to C (60) - prefer lower when equidistant
        assert_eq!(snapper.snap_midi(61.0), 60.0);

        // F# (66) should snap to F (65) or G (67)
        // F# is equidistant, should prefer lower (F)
        let snapped = snapper.snap_midi(66.0);
        assert!(snapped == 65.0 || snapped == 67.0);
    }

    #[test]
    fn test_scale_snapper_chromatic() {
        let root = FixedRoot::parse("c").unwrap();
        let snapper = ScaleSnapper::new(&root, "chromatic").unwrap();

        // Chromatic should pass through all notes unchanged
        for midi in 0..128 {
            assert_eq!(snapper.snap_midi(midi as f64), midi as f64);
        }
    }

    #[test]
    fn test_scale_snapper_discards_cents() {
        let root = FixedRoot::parse("c").unwrap();
        let snapper = ScaleSnapper::new(&root, "major").unwrap();

        // 60.3 (C + 30 cents) should snap to exactly 60.0 (C)
        assert_eq!(snapper.snap_midi(60.3), 60.0);

        // 60.6 (closer to 61 = C#, which snaps to C in C major) → 60.0
        assert_eq!(snapper.snap_midi(60.6), 60.0);

        // 61.4 (C# + 40 cents, rounds to 61, snaps to C) → 60.0
        assert_eq!(snapper.snap_midi(61.4), 60.0);

        // 61.6 (closer to 62 = D, which is in C major) → 62.0
        assert_eq!(snapper.snap_midi(61.6), 62.0);
    }

    #[test]
    fn test_scale_snapper_stable_within_semitone() {
        let root = FixedRoot::parse("c").unwrap();
        let snapper = ScaleSnapper::new(&root, "major").unwrap();

        // Sweeping from 60.0 to 60.49 should always produce 60.0 (C)
        // since all round to MIDI 60
        for i in 0..50 {
            let midi = 60.0 + i as f64 * 0.01;
            assert_eq!(
                snapper.snap_midi(midi),
                60.0,
                "MIDI {midi} should snap to 60.0"
            );
        }
    }

    #[test]
    fn test_scale_snapper_from_intervals() {
        let root = FixedRoot::parse("c").unwrap();
        // Major scale intervals: 0, 2, 4, 5, 7, 9, 11
        let snapper = ScaleSnapper::from_intervals(&root, &[0, 2, 4, 5, 7, 9, 11], et_tuning());

        // C (60) should stay C
        assert_eq!(snapper.snap_midi(60.0), 60.0);
        // D (62) should stay D
        assert_eq!(snapper.snap_midi(62.0), 62.0);
        // C# (61) should snap to C (60)
        assert_eq!(snapper.snap_midi(61.0), 60.0);
    }

    #[test]
    fn test_scale_snapper_voct() {
        let root = FixedRoot::parse("c").unwrap();
        let snapper = ScaleSnapper::new(&root, "major").unwrap();

        // C4 = MIDI 60 = V/Oct 0.0
        let c4_voct = (60.0 - 60.0) / 12.0;
        let snapped = snapper.snap_voct(c4_voct);
        assert!((snapped - c4_voct).abs() < 0.001);
    }

    #[test]
    fn test_validate_scale_type() {
        // Standard scale names
        assert!(validate_scale_type("major"));
        assert!(validate_scale_type("Minor"));
        assert!(validate_scale_type("dorian"));
        assert!(validate_scale_type("harmonic minor"));
        assert!(validate_scale_type("harmonicminor"));
        assert!(validate_scale_type("chromatic"));

        // Diatonic modes
        assert!(validate_scale_type("ionian"));
        assert!(validate_scale_type("phrygian"));
        assert!(validate_scale_type("lydian"));
        assert!(validate_scale_type("mixolydian"));
        assert!(validate_scale_type("aeolian"));
        assert!(validate_scale_type("locrian"));

        // Additional scale types
        assert!(validate_scale_type("melodic minor"));
        assert!(validate_scale_type("pentatonic major"));
        assert!(validate_scale_type("pentatonic minor"));
        assert!(validate_scale_type("blues"));
        assert!(validate_scale_type("whole tone"));

        // Abbreviations supported by rust_music_theory
        assert!(validate_scale_type("maj"));
        assert!(validate_scale_type("min"));
        assert!(validate_scale_type("pent maj"));
        assert!(validate_scale_type("pent min"));
        assert!(validate_scale_type("har minor"));
        assert!(validate_scale_type("mel minor"));
        assert!(validate_scale_type("wholetone"));

        // Invalid scale types should fail
        assert!(!validate_scale_type("unknown_scale"));
        assert!(!validate_scale_type("fake_mode"));
        assert!(!validate_scale_type(""));

        // Just / Pythagorean tunings are recognized
        assert!(validate_scale_type("just"));
        assert!(validate_scale_type("Just"));
        assert!(validate_scale_type("pythagorean"));
        assert!(validate_scale_type("Pythagorean"));
        assert!(validate_scale_type("pythag"));
        assert!(validate_scale_type("Pythag"));
    }

    #[test]
    fn test_from_intervals_just_tuning() {
        let root = FixedRoot::parse("c").unwrap();
        // Custom subset {0,3,4,8} tuned with just intonation.
        let just =
            ScaleSnapper::from_intervals(&root, &[0, 3, 4, 8], named_tuning("just").unwrap());
        assert_eq!(just.scale_intervals().as_slice(), &[0, 3, 4, 8]);
        // The major third (4 semitones) carries the just 5/4 ratio.
        assert!((just.tuned_voct(64) - 1.25_f64.log2()).abs() < 1e-9);

        // The same intervals at 12-TET keep the third at 4/12 V.
        let et = ScaleSnapper::from_intervals(&root, &[0, 3, 4, 8], et_tuning());
        assert_eq!(et.scale_intervals().as_slice(), &[0, 3, 4, 8]);
        assert!((et.tuned_voct(64) - 4.0 / 12.0).abs() < 1e-9);

        // Pythagorean alias resolves to the same table as the full name.
        assert_eq!(named_tuning("pythag"), named_tuning("pythagorean"));
    }

    #[test]
    fn test_just_intonation_tuning() {
        let root = FixedRoot::parse("c").unwrap();
        let snapper = ScaleSnapper::new(&root, "just").unwrap();

        // Root unchanged.
        assert!((snapper.tuned_voct(60) - 0.0).abs() < 1e-9);
        // Perfect fifth = 3/2.
        assert!((snapper.tuned_voct(67) - 1.5_f64.log2()).abs() < 1e-9);
        // Major third = 5/4.
        assert!((snapper.tuned_voct(64) - 1.25_f64.log2()).abs() < 1e-9);
        // Octave = exactly +1 V.
        assert!((snapper.tuned_voct(72) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_pythagorean_tuning() {
        let root = FixedRoot::parse("c").unwrap();
        let snapper = ScaleSnapper::new(&root, "pythagorean").unwrap();

        // Perfect fifth = 3/2 (same as just).
        assert!((snapper.tuned_voct(67) - 1.5_f64.log2()).abs() < 1e-9);
        // Major third = 81/64 (wider than the just third).
        assert!((snapper.tuned_voct(64) - (81.0_f64 / 64.0).log2()).abs() < 1e-9);
        assert!(snapper.tuned_voct(64) > 1.25_f64.log2());
    }

    #[test]
    fn test_just_tuning_root_offset() {
        // Root D: the fifth above D is A, MIDI 69.
        let root = FixedRoot::parse("d").unwrap();
        let snapper = ScaleSnapper::new(&root, "just").unwrap();

        // D4 (MIDI 62) sits at 2/12 V.
        assert!((snapper.tuned_voct(62) - 2.0 / 12.0).abs() < 1e-9);
        // A above D is a just fifth higher.
        assert!((snapper.tuned_voct(69) - (2.0 / 12.0 + 1.5_f64.log2())).abs() < 1e-9);
    }

    #[test]
    fn test_just_snap_voct() {
        let root = FixedRoot::parse("c").unwrap();
        let snapper = ScaleSnapper::new(&root, "just").unwrap();

        // An equal-tempered major third input snaps to the just major third.
        let et_third = 4.0 / 12.0;
        let snapped = snapper.snap_voct(et_third);
        assert!((snapped - 1.25_f64.log2()).abs() < 1e-9);

        // A 12-TET snapper leaves the same input unchanged.
        let et_snapper = ScaleSnapper::new(&root, "chromatic").unwrap();
        assert!((et_snapper.snap_voct(et_third) - et_third).abs() < 1e-9);
    }
}
