//! Named scale vocabulary for `root(name)` scale specs.

const MAJOR: &[i8] = &[0, 2, 4, 5, 7, 9, 11];
const MINOR: &[i8] = &[0, 2, 3, 5, 7, 8, 10];
const HARMONIC_MINOR: &[i8] = &[0, 2, 3, 5, 7, 8, 11];
const MELODIC_MINOR: &[i8] = &[0, 2, 3, 5, 7, 9, 11];
const PENTATONIC_MAJOR: &[i8] = &[0, 2, 4, 7, 9];
const PENTATONIC_MINOR: &[i8] = &[0, 3, 5, 7, 10];

/// Names are matched ASCII case-insensitively after collapsing whitespace runs
/// to a single space.
const SCALES: &[(&str, &[i8])] = &[
    ("major", MAJOR),
    ("maj", MAJOR),
    ("ionian", MAJOR),
    ("minor", MINOR),
    ("min", MINOR),
    ("aeolian", MINOR),
    ("dorian", &[0, 2, 3, 5, 7, 9, 10]),
    ("phrygian", &[0, 1, 3, 5, 7, 8, 10]),
    ("lydian", &[0, 2, 4, 6, 7, 9, 11]),
    ("mixolydian", &[0, 2, 4, 5, 7, 9, 10]),
    ("locrian", &[0, 1, 3, 5, 6, 8, 10]),
    ("harmonic minor", HARMONIC_MINOR),
    ("harmonicminor", HARMONIC_MINOR),
    ("har minor", HARMONIC_MINOR),
    ("melodic minor", MELODIC_MINOR),
    ("melodicminor", MELODIC_MINOR),
    ("mel minor", MELODIC_MINOR),
    ("pentatonic major", PENTATONIC_MAJOR),
    ("pentatonic maj", PENTATONIC_MAJOR),
    ("pent major", PENTATONIC_MAJOR),
    ("pent maj", PENTATONIC_MAJOR),
    ("pentatonic minor", PENTATONIC_MINOR),
    ("pentatonic min", PENTATONIC_MINOR),
    ("pent minor", PENTATONIC_MINOR),
    ("pent min", PENTATONIC_MINOR),
    ("blues", &[0, 3, 5, 6, 7, 10]),
    ("whole tone", &[0, 2, 4, 6, 8, 10]),
    ("wholetone", &[0, 2, 4, 6, 8, 10]),
    ("chromatic", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
];

/// Look up a scale's semitone intervals (ascending, within one octave) by name.
///
/// `M` and `m` are case-sensitive abbreviations for major and minor; every
/// other name is case-insensitive.
pub fn lookup(name: &str) -> Option<&'static [i8]> {
    let name = name.trim();
    match name {
        "M" => return Some(MAJOR),
        "m" => return Some(MINOR),
        _ => {}
    }
    SCALES
        .iter()
        .find(|(candidate, _)| words_eq_ignore_case(name, candidate))
        .map(|(_, intervals)| *intervals)
}

/// Compare whitespace-separated words, ASCII case-insensitively, without
/// allocating.
fn words_eq_ignore_case(a: &str, b: &str) -> bool {
    let mut a = a.split_whitespace();
    let mut b = b.split_whitespace();
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if x.eq_ignore_ascii_case(y) => {}
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_alias_resolves() {
        for (name, intervals) in SCALES {
            assert_eq!(lookup(name), Some(*intervals), "{name}");
        }
    }

    #[test]
    fn case_and_whitespace_insensitive() {
        assert_eq!(lookup("Minor"), Some(MINOR));
        assert_eq!(lookup("HARMONIC   minor"), Some(HARMONIC_MINOR));
        assert_eq!(lookup(" pent maj "), Some(PENTATONIC_MAJOR));
    }

    #[test]
    fn single_letter_abbreviations_are_case_sensitive() {
        assert_eq!(lookup("M"), Some(MAJOR));
        assert_eq!(lookup("m"), Some(MINOR));
    }

    #[test]
    fn names_must_match_exactly() {
        assert_eq!(lookup("maj7"), None);
        assert_eq!(lookup("major foo"), None);
        assert_eq!(lookup("M anything"), None);
        assert_eq!(lookup("xblues"), None);
        assert_eq!(lookup(""), None);
    }

    #[test]
    fn intervals_are_ascending_within_an_octave() {
        for (name, intervals) in SCALES {
            assert_eq!(intervals[0], 0, "{name}");
            assert!(intervals.windows(2).all(|w| w[0] < w[1]), "{name}");
            assert!(*intervals.last().unwrap() < 12, "{name}");
        }
    }
}
