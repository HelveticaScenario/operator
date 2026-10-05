//! Chord shorthand vocabulary for `root[name]` chord specs.

const MAJOR: &[i8] = &[0, 4, 7];
const MINOR: &[i8] = &[0, 3, 7];
const DIMINISHED: &[i8] = &[0, 3, 6];
const AUGMENTED: &[i8] = &[0, 4, 8];
const SUS4: &[i8] = &[0, 5, 7];
const DOMINANT_7: &[i8] = &[0, 4, 7, 10];
const MAJOR_7: &[i8] = &[0, 4, 7, 11];
const MINOR_7: &[i8] = &[0, 3, 7, 10];
const MINOR_MAJOR_7: &[i8] = &[0, 3, 7, 11];
const DIMINISHED_7: &[i8] = &[0, 3, 6, 9];
const HALF_DIMINISHED_7: &[i8] = &[0, 3, 6, 10];
const AUGMENTED_7: &[i8] = &[0, 4, 8, 10];
const ADD_9: &[i8] = &[0, 4, 7, 14];
const ADD_11: &[i8] = &[0, 4, 7, 17];

/// Semitone offsets from the root, ascending. Extensions above the octave
/// keep their register (a 9th is 14, not 2). Names are case-sensitive because
/// `M` and `m` name different chords.
const CHORDS: &[(&str, &[i8])] = &[
    ("maj", MAJOR),
    ("M", MAJOR),
    ("major", MAJOR),
    ("min", MINOR),
    ("m", MINOR),
    ("minor", MINOR),
    ("dim", DIMINISHED),
    ("°", DIMINISHED),
    ("aug", AUGMENTED),
    ("+", AUGMENTED),
    ("sus2", &[0, 2, 7]),
    ("sus4", SUS4),
    ("sus", SUS4),
    ("5", &[0, 7]),
    ("6", &[0, 4, 7, 9]),
    ("m6", &[0, 3, 7, 9]),
    ("69", &[0, 4, 7, 9, 14]),
    ("7", DOMINANT_7),
    ("dom7", DOMINANT_7),
    ("maj7", MAJOR_7),
    ("M7", MAJOR_7),
    ("Δ", MAJOR_7),
    ("Δ7", MAJOR_7),
    ("m7", MINOR_7),
    ("min7", MINOR_7),
    ("mMaj7", MINOR_MAJOR_7),
    ("mM7", MINOR_MAJOR_7),
    ("dim7", DIMINISHED_7),
    ("°7", DIMINISHED_7),
    ("m7b5", HALF_DIMINISHED_7),
    ("ø", HALF_DIMINISHED_7),
    ("ø7", HALF_DIMINISHED_7),
    ("aug7", AUGMENTED_7),
    ("7#5", AUGMENTED_7),
    ("+7", AUGMENTED_7),
    ("maj7#5", &[0, 4, 8, 11]),
    ("7sus4", &[0, 5, 7, 10]),
    ("7sus2", &[0, 2, 7, 10]),
    ("7b5", &[0, 4, 6, 10]),
    ("9", &[0, 4, 7, 10, 14]),
    ("maj9", &[0, 4, 7, 11, 14]),
    ("m9", &[0, 3, 7, 10, 14]),
    ("11", &[0, 4, 7, 10, 14, 17]),
    ("maj11", &[0, 4, 7, 11, 14, 17]),
    ("m11", &[0, 3, 7, 10, 14, 17]),
    ("13", &[0, 4, 7, 10, 14, 17, 21]),
    ("maj13", &[0, 4, 7, 11, 14, 17, 21]),
    ("m13", &[0, 3, 7, 10, 14, 17, 21]),
    ("7b9", &[0, 4, 7, 10, 13]),
    ("7#9", &[0, 4, 7, 10, 15]),
    ("7#11", &[0, 4, 7, 10, 18]),
    ("add9", ADD_9),
    ("add2", ADD_9),
    ("madd9", &[0, 3, 7, 14]),
    ("add11", ADD_11),
    ("add4", ADD_11),
];

/// Look up a chord's voicing (semitones from the root, ascending) by name.
pub fn lookup(name: &str) -> Option<&'static [i8]> {
    CHORDS
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, intervals)| *intervals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_alias_resolves() {
        for (name, intervals) in CHORDS {
            assert_eq!(lookup(name), Some(*intervals), "{name}");
        }
    }

    #[test]
    fn names_are_unique() {
        for (i, (a, _)) in CHORDS.iter().enumerate() {
            assert!(
                CHORDS[i + 1..].iter().all(|(b, _)| a != b),
                "duplicate chord name {a}"
            );
        }
    }

    #[test]
    fn voicings_are_rooted_and_ascending() {
        for (name, intervals) in CHORDS {
            assert_eq!(intervals[0], 0, "{name}");
            assert!(intervals.windows(2).all(|w| w[0] < w[1]), "{name}");
        }
    }

    #[test]
    fn major_and_minor_letters_are_case_sensitive() {
        assert_eq!(lookup("M"), Some(MAJOR));
        assert_eq!(lookup("m"), Some(MINOR));
        assert_eq!(lookup("M7"), Some(MAJOR_7));
        assert_eq!(lookup("m7"), Some(MINOR_7));
        assert_eq!(lookup("MAJ7"), None);
    }
}
