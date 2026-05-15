//! Abstract Syntax Tree types for mini notation.
//!
//! These types represent the parsed structure of a mini notation pattern,
//! including source span information for editor highlighting.

use crate::pattern_system::SourceSpan;

/// Source location in the original pattern string.
#[derive(Clone, Debug, PartialEq)]
pub struct Located<T> {
    pub node: T,
    pub span: SourceSpan,
}

impl<T> Located<T> {
    pub fn new(node: T, start: usize, end: usize) -> Self {
        Self {
            node,
            span: SourceSpan::new(start, end),
        }
    }
}

/// AST for signed integer patterns (used for euclidean rotation).
#[derive(Clone, Debug, PartialEq)]
pub enum MiniASTI32 {
    /// A single value atom.
    Pure(Located<i32>),

    /// Rest/silence.
    Rest(SourceSpan),

    /// A list of patterns (from tail syntax).
    List(Located<Vec<MiniASTI32>>),

    /// Sequence of patterns (space-separated, played in order).
    Sequence(Vec<(MiniASTI32, Option<f64>)>), // (pattern, optional weight)

    /// Fast subsequence from [...] syntax (explicit fastcat).
    FastCat(Vec<(MiniASTI32, Option<f64>)>), // (pattern, optional weight)

    /// Slow subsequence (one item per cycle, with optional @ weight).
    SlowCat(Vec<(MiniASTI32, Option<f64>)>),

    /// Stack: comma-separated patterns play simultaneously.
    Stack(Vec<MiniASTI32>),

    /// Random choice between values.
    RandomChoice(Vec<MiniASTI32>, u64),

    /// Fast modifier: pattern * factor.
    Fast(Box<MiniASTI32>, Box<MiniASTF64>),

    /// Slow modifier: pattern / factor.
    Slow(Box<MiniASTI32>, Box<MiniASTF64>),

    /// Replicate: pattern ! n (repeat n times).
    Replicate(Box<MiniASTI32>, u32),

    /// Degrade: pattern ? prob (randomly drop with probability).
    Degrade(Box<MiniASTI32>, Option<f64>, u64),

    /// Euclidean rhythm: pattern(pulses, steps, rotation?).
    Euclidean {
        pattern: Box<MiniASTI32>,
        pulses: Box<MiniASTU32>,
        steps: Box<MiniASTU32>,
        rotation: Option<Box<MiniASTI32>>,
    },
}

/// AST for unsigned integer patterns (used for euclidean pulses/steps).
#[derive(Clone, Debug, PartialEq)]
pub enum MiniASTU32 {
    /// A single value atom.
    Pure(Located<u32>),

    /// Rest/silence.
    Rest(SourceSpan),

    /// A list of patterns (from tail syntax: c:e:g or c:[e f]).
    /// Elements can be atoms or subpatterns.
    List(Located<Vec<MiniASTU32>>),

    /// Sequence of patterns (space-separated, played in order).
    Sequence(Vec<(MiniASTU32, Option<f64>)>), // (pattern, optional weight)

    /// Fast subsequence from [...] syntax (explicit fastcat).
    FastCat(Vec<(MiniASTU32, Option<f64>)>), // (pattern, optional weight)

    /// Slow subsequence (one item per cycle, with optional @ weight).
    SlowCat(Vec<(MiniASTU32, Option<f64>)>),

    /// Stack: comma-separated patterns play simultaneously.
    Stack(Vec<MiniASTU32>),

    /// Random choice between values.
    RandomChoice(Vec<MiniASTU32>, u64),

    /// Fast modifier: pattern * factor.
    Fast(Box<MiniASTU32>, Box<MiniASTF64>),

    /// Slow modifier: pattern / factor.
    Slow(Box<MiniASTU32>, Box<MiniASTF64>),

    /// Replicate: pattern ! n (repeat n times).
    /// Count is a plain u32 since Strudel doesn't support patterned replicate counts.
    Replicate(Box<MiniASTU32>, u32),

    /// Degrade: pattern ? prob (randomly drop with probability).
    Degrade(Box<MiniASTU32>, Option<f64>, u64),

    /// Euclidean rhythm: pattern(pulses, steps, rotation?).
    Euclidean {
        pattern: Box<MiniASTU32>,
        pulses: Box<MiniASTU32>,
        steps: Box<MiniASTU32>,
        rotation: Option<Box<MiniASTI32>>,
    },
}
/// The main AST node type.
#[derive(Clone, Debug, PartialEq)]
pub enum MiniASTF64 {
    /// A single value atom.
    Pure(Located<f64>),

    /// Rest/silence.
    Rest(SourceSpan),

    /// A list of patterns (from tail syntax: c:e:g or c:[e f]).
    /// Elements can be atoms or subpatterns.
    List(Located<Vec<MiniASTF64>>),

    /// Sequence of patterns (space-separated, played in order).
    Sequence(Vec<(MiniASTF64, Option<f64>)>), // (pattern, optional weight)

    /// Fast subsequence from [...] syntax (explicit fastcat).
    FastCat(Vec<(MiniASTF64, Option<f64>)>), // (pattern, optional weight)

    /// Slow subsequence (one item per cycle, with optional @ weight).
    SlowCat(Vec<(MiniASTF64, Option<f64>)>),

    /// Stack: comma-separated patterns play simultaneously.
    Stack(Vec<MiniASTF64>),

    /// Random choice between values.
    RandomChoice(Vec<MiniASTF64>, u64),

    /// Fast modifier: pattern * factor.
    Fast(Box<MiniASTF64>, Box<MiniASTF64>),

    /// Slow modifier: pattern / factor.
    Slow(Box<MiniASTF64>, Box<MiniASTF64>),

    /// Replicate: pattern ! n (repeat n times).
    /// Count is a plain u32 since Strudel doesn't support patterned replicate counts.
    Replicate(Box<MiniASTF64>, u32),

    /// Degrade: pattern ? prob (randomly drop with probability).
    Degrade(Box<MiniASTF64>, Option<f64>, u64),

    /// Euclidean rhythm: pattern(pulses, steps, rotation?).
    Euclidean {
        pattern: Box<MiniASTF64>,
        pulses: Box<MiniASTU32>,
        steps: Box<MiniASTU32>,
        rotation: Option<Box<MiniASTI32>>,
    },
}
/// The main AST node type.
#[derive(Clone, Debug, PartialEq)]
pub enum MiniAST {
    /// A single value atom.
    Pure(Located<AtomValue>),

    /// Rest/silence.
    Rest(SourceSpan),

    /// A list of patterns (from tail syntax: c:e:g or c:[e f]).
    /// Elements can be atoms or subpatterns.
    List(Located<Vec<MiniAST>>),

    /// Sequence of patterns (space-separated, played in order).
    Sequence(Vec<(MiniAST, Option<f64>)>), // (pattern, optional weight)

    /// Fast subsequence from [...] syntax (explicit fastcat).
    /// Unlike Sequence which is the implicit result of space-separated elements,
    /// FastCat preserves that this came from explicit [...] grouping.
    /// This distinction matters for nesting: `<[c e]>` should be slowcat of one fastcat,
    /// not slowcat of two elements.
    FastCat(Vec<(MiniAST, Option<f64>)>), // (pattern, optional weight)

    /// Slow subsequence (one item per cycle, with optional @ weight).
    SlowCat(Vec<(MiniAST, Option<f64>)>),

    /// Stack: comma-separated patterns play simultaneously.
    Stack(Vec<MiniAST>),

    /// Random choice between values.
    RandomChoice(Vec<MiniAST>, u64),

    /// Fast modifier: pattern * factor.
    Fast(Box<MiniAST>, Box<MiniASTF64>),

    /// Slow modifier: pattern / factor.
    Slow(Box<MiniAST>, Box<MiniAST>),

    /// Replicate: pattern ! n (repeat n times).
    /// Count is a plain u32 since Strudel doesn't support patterned replicate counts.
    Replicate(Box<MiniAST>, u32),

    /// Degrade: pattern ? prob (randomly drop with probability).
    Degrade(Box<MiniAST>, Option<f64>, u64),

    /// Euclidean rhythm: pattern(pulses, steps, rotation?).
    Euclidean {
        pattern: Box<MiniAST>,
        pulses: Box<MiniASTU32>,
        steps: Box<MiniASTU32>,
        rotation: Option<Box<MiniASTI32>>,
    },
}

/// Atomic value types.
#[derive(Clone, Debug, PartialEq)]
pub enum AtomValue {
    /// Numeric value.
    Number(f64),

    /// MIDI note number.
    Midi(i32),

    /// Frequency in Hz.
    Hz(f64),

    /// Voltage (for modular synth).
    Volts(f64),

    /// Musical note (e.g., c4, a#3, bb5).
    Note {
        letter: char,
        /// Accidental: '#' for sharp, 'b' for flat
        accidental: Option<char>,
        octave: Option<i32>,
    },

    /// String identifier (for scale names, sample names, etc.).
    Identifier(String),

    /// Quoted string.
    String(String),
}

impl AtomValue {
    /// Convert to f64 if possible.
    pub fn to_f64(&self) -> Option<f64> {
        match self {
            AtomValue::Number(n) => Some(*n),
            AtomValue::Midi(m) => Some(*m as f64),
            AtomValue::Hz(h) => Some(*h),
            AtomValue::Volts(v) => Some(*v),
            AtomValue::Note {
                letter,
                accidental,
                octave,
            } => {
                // Convert note to MIDI number
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
                    Some('#') | Some('s') => 1,
                    Some('b') | Some('f') => -1,
                    _ => 0,
                };

                let oct = octave.unwrap_or(4);
                Some(((oct + 1) * 12 + base + acc_offset) as f64)
            }
            AtomValue::Identifier(_) => None,
            AtomValue::String(_) => None,
        }
    }

    /// Try to parse a string as an AtomValue.
    pub fn parse(s: &str) -> AtomValue {
        // Try parsing as number first
        if let Ok(n) = s.parse::<f64>() {
            return AtomValue::Number(n);
        }

        // Check for Hz suffix
        if (s.ends_with("hz") || s.ends_with("Hz"))
            && let Ok(n) = s[..s.len() - 2].parse::<f64>()
        {
            return AtomValue::Hz(n);
        }

        // Check for voltage suffix
        if (s.ends_with('v') || s.ends_with('V'))
            && let Ok(n) = s[..s.len() - 1].parse::<f64>()
        {
            return AtomValue::Volts(n);
        }

        // Check for MIDI prefix
        if (s.starts_with('m') || s.starts_with('M'))
            && let Ok(n) = s[1..].parse::<i32>()
        {
            return AtomValue::Midi(n);
        }

        // Try parsing as note
        if let Some(note) = parse_note(s) {
            return note;
        }

        // Default to identifier
        AtomValue::Identifier(s.to_string())
    }
}

fn parse_note(s: &str) -> Option<AtomValue> {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let letter = chars[0].to_ascii_lowercase();
    if !('a'..='g').contains(&letter) {
        return None;
    }

    // A single letter like "c" might be a note, but multi-char without
    // accidental or octave is likely an identifier
    if chars.len() == 1 {
        // Single letter could be a note without octave
        return Some(AtomValue::Note {
            letter,
            accidental: None,
            octave: None,
        });
    }

    let mut idx = 1;
    let mut accidental = None;

    // Check for single accidental
    if idx < chars.len() {
        match chars[idx] {
            '#' | 's' => {
                accidental = Some('#');
                idx += 1;
            }
            'b' | 'f' => {
                // 'b' or 'f' as accidental only if followed by digit, or at end
                // and not part of a word like "bd" (bass drum)
                if idx + 1 >= chars.len() {
                    // At end - this is ambiguous, treat as accidental for single letter note
                    accidental = Some('b');
                    idx += 1;
                } else if chars[idx + 1].is_ascii_digit() || chars[idx + 1] == '-' {
                    accidental = Some('b');
                    idx += 1;
                } else {
                    // Followed by non-digit like "bd" - this is an identifier
                    return None;
                }
            }
            c if c.is_ascii_digit() || c == '-' => {
                // Octave follows directly
            }
            _ => {
                // Invalid note format (e.g., "bd", "sn")
                return None;
            }
        }
    }

    // Parse octave
    let octave = if idx < chars.len() {
        let octave_str: String = chars[idx..].iter().collect();
        octave_str.parse::<i32>().ok()
    } else {
        None
    };

    Some(AtomValue::Note {
        letter,
        accidental,
        octave,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atom_parse_number() {
        assert_eq!(AtomValue::parse("42"), AtomValue::Number(42.0));
        assert_eq!(AtomValue::parse("3.14"), AtomValue::Number(3.14));
        assert_eq!(AtomValue::parse("-1.5"), AtomValue::Number(-1.5));
    }

    #[test]
    fn test_atom_parse_hz() {
        assert_eq!(AtomValue::parse("440hz"), AtomValue::Hz(440.0));
        assert_eq!(AtomValue::parse("880Hz"), AtomValue::Hz(880.0));
    }

    #[test]
    fn test_atom_parse_volts() {
        assert_eq!(AtomValue::parse("5v"), AtomValue::Volts(5.0));
        assert_eq!(AtomValue::parse("1.5V"), AtomValue::Volts(1.5));
    }

    #[test]
    fn test_atom_parse_midi() {
        assert_eq!(AtomValue::parse("m60"), AtomValue::Midi(60));
        assert_eq!(AtomValue::parse("M127"), AtomValue::Midi(127));
    }

    #[test]
    fn test_atom_parse_note() {
        assert_eq!(
            AtomValue::parse("c4"),
            AtomValue::Note {
                letter: 'c',
                accidental: None,
                octave: Some(4)
            }
        );
        assert_eq!(
            AtomValue::parse("a#3"),
            AtomValue::Note {
                letter: 'a',
                accidental: Some('#'),
                octave: Some(3)
            }
        );
        assert_eq!(
            AtomValue::parse("bb5"),
            AtomValue::Note {
                letter: 'b',
                accidental: Some('b'),
                octave: Some(5)
            }
        );
    }

    #[test]
    fn test_note_to_f64() {
        let c4 = AtomValue::Note {
            letter: 'c',
            accidental: None,
            octave: Some(4),
        };
        assert_eq!(c4.to_f64(), Some(60.0)); // Middle C

        let a4 = AtomValue::Note {
            letter: 'a',
            accidental: None,
            octave: Some(4),
        };
        assert_eq!(a4.to_f64(), Some(69.0)); // A440
    }

    #[test]
    fn test_atom_parse_identifier() {
        assert_eq!(
            AtomValue::parse("maj"),
            AtomValue::Identifier("maj".to_string())
        );
        assert_eq!(
            AtomValue::parse("bd"),
            AtomValue::Identifier("bd".to_string())
        );
    }

    #[test]
    fn test_collect_leaf_spans_includes_modifiers() {
        use super::super::parser::parse;

        // Pattern: "c*[1 2]" - both 'c' and '1', '2' should have spans
        // "c*[1 2]" positions:
        //  c at 0-1
        //  * at 1
        //  [ at 2
        //  1 at 3-4
        //  space at 4
        //  2 at 5-6
        //  ] at 6
        let ast = parse("c*[1 2]").unwrap();
        let spans = collect_leaf_spans(&ast);

        // Should have 3 spans: c, 1, and 2
        assert_eq!(
            spans.len(),
            3,
            "Expected 3 spans (c, 1, 2), got {:?}",
            spans
        );
        assert!(
            spans.contains(&(0, 1)),
            "Missing span for 'c' at 0-1: {:?}",
            spans
        );
        assert!(
            spans.contains(&(3, 4)),
            "Missing span for '1' at 3-4: {:?}",
            spans
        );
        assert!(
            spans.contains(&(5, 6)),
            "Missing span for '2' at 5-6: {:?}",
            spans
        );
    }
}

/// Collect all leaf source spans from a MiniAST.
/// This traverses the entire AST and collects spans from Pure nodes and Rest nodes.
pub fn collect_leaf_spans(ast: &MiniAST) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    collect_leaf_spans_recursive(ast, &mut spans);
    spans
}

fn collect_leaf_spans_recursive(ast: &MiniAST, spans: &mut Vec<(usize, usize)>) {
    match ast {
        MiniAST::Pure(located) => {
            spans.push(located.span.to_tuple());
        }
        MiniAST::Rest(span) => {
            spans.push(span.to_tuple());
        }
        MiniAST::List(located) => {
            for child in &located.node {
                collect_leaf_spans_recursive(child, spans);
            }
        }
        MiniAST::Sequence(items) | MiniAST::FastCat(items) => {
            for (child, _weight) in items {
                collect_leaf_spans_recursive(child, spans);
            }
        }
        MiniAST::SlowCat(items) => {
            for (child, _weight) in items {
                collect_leaf_spans_recursive(child, spans);
            }
        }
        MiniAST::RandomChoice(items, _) | MiniAST::Stack(items) => {
            for child in items {
                collect_leaf_spans_recursive(child, spans);
            }
        }
        MiniAST::Fast(pattern, factor) => {
            collect_leaf_spans_recursive(pattern, spans);
            collect_f64_spans(factor, spans);
        }
        MiniAST::Slow(pattern, factor) => {
            collect_leaf_spans_recursive(pattern, spans);
            // Slow's factor is MiniAST, not MiniASTF64
            collect_leaf_spans_recursive(factor, spans);
        }
        MiniAST::Replicate(pattern, _count) => {
            collect_leaf_spans_recursive(pattern, spans);
        }
        MiniAST::Degrade(pattern, _prob, _) => {
            collect_leaf_spans_recursive(pattern, spans);
        }
        MiniAST::Euclidean {
            pattern,
            pulses,
            steps,
            rotation,
        } => {
            collect_leaf_spans_recursive(pattern, spans);
            collect_u32_spans(pulses, spans);
            collect_u32_spans(steps, spans);
            if let Some(rot) = rotation {
                collect_i32_spans(rot, spans);
            }
        }
    }
}

/// Collect spans from MiniASTF64 (used for fast/slow factors).
fn collect_f64_spans(ast: &MiniASTF64, spans: &mut Vec<(usize, usize)>) {
    match ast {
        MiniASTF64::Pure(located) => {
            spans.push(located.span.to_tuple());
        }
        MiniASTF64::Rest(span) => {
            spans.push(span.to_tuple());
        }
        MiniASTF64::List(located) => {
            for child in &located.node {
                collect_f64_spans(child, spans);
            }
        }
        MiniASTF64::Sequence(items) | MiniASTF64::FastCat(items) => {
            for (child, _weight) in items {
                collect_f64_spans(child, spans);
            }
        }
        MiniASTF64::SlowCat(items) => {
            for (child, _weight) in items {
                collect_f64_spans(child, spans);
            }
        }
        MiniASTF64::RandomChoice(items, _) | MiniASTF64::Stack(items) => {
            for child in items {
                collect_f64_spans(child, spans);
            }
        }
        MiniASTF64::Fast(pattern, factor) => {
            collect_f64_spans(pattern, spans);
            collect_f64_spans(factor, spans);
        }
        MiniASTF64::Slow(pattern, factor) => {
            collect_f64_spans(pattern, spans);
            collect_f64_spans(factor, spans);
        }
        MiniASTF64::Replicate(pattern, _count) => {
            collect_f64_spans(pattern, spans);
        }
        MiniASTF64::Degrade(pattern, _prob, _) => {
            collect_f64_spans(pattern, spans);
        }
        MiniASTF64::Euclidean {
            pattern,
            pulses,
            steps,
            rotation,
        } => {
            collect_f64_spans(pattern, spans);
            collect_u32_spans(pulses, spans);
            collect_u32_spans(steps, spans);
            if let Some(rot) = rotation {
                collect_i32_spans(rot, spans);
            }
        }
    }
}

/// Collect spans from MiniASTU32 (used for euclidean pulses/steps).
fn collect_u32_spans(ast: &MiniASTU32, spans: &mut Vec<(usize, usize)>) {
    match ast {
        MiniASTU32::Pure(located) => {
            spans.push(located.span.to_tuple());
        }
        MiniASTU32::Rest(span) => {
            spans.push(span.to_tuple());
        }
        MiniASTU32::List(located) => {
            for child in &located.node {
                collect_u32_spans(child, spans);
            }
        }
        MiniASTU32::Sequence(items) | MiniASTU32::FastCat(items) => {
            for (child, _weight) in items {
                collect_u32_spans(child, spans);
            }
        }
        MiniASTU32::SlowCat(items) => {
            for (child, _weight) in items {
                collect_u32_spans(child, spans);
            }
        }
        MiniASTU32::RandomChoice(items, _) | MiniASTU32::Stack(items) => {
            for child in items {
                collect_u32_spans(child, spans);
            }
        }
        MiniASTU32::Fast(pattern, factor) => {
            collect_u32_spans(pattern, spans);
            collect_f64_spans(factor, spans);
        }
        MiniASTU32::Slow(pattern, factor) => {
            collect_u32_spans(pattern, spans);
            collect_f64_spans(factor, spans);
        }
        MiniASTU32::Replicate(pattern, _count) => {
            collect_u32_spans(pattern, spans);
        }
        MiniASTU32::Degrade(pattern, _prob, _) => {
            collect_u32_spans(pattern, spans);
        }
        MiniASTU32::Euclidean {
            pattern,
            pulses,
            steps,
            rotation,
        } => {
            collect_u32_spans(pattern, spans);
            collect_u32_spans(pulses, spans);
            collect_u32_spans(steps, spans);
            if let Some(rot) = rotation {
                collect_i32_spans(rot, spans);
            }
        }
    }
}

/// Collect spans from MiniASTI32 (used for euclidean rotation).
fn collect_i32_spans(ast: &MiniASTI32, spans: &mut Vec<(usize, usize)>) {
    match ast {
        MiniASTI32::Pure(located) => {
            spans.push(located.span.to_tuple());
        }
        MiniASTI32::Rest(span) => {
            spans.push(span.to_tuple());
        }
        MiniASTI32::List(located) => {
            for child in &located.node {
                collect_i32_spans(child, spans);
            }
        }
        MiniASTI32::Sequence(items) | MiniASTI32::FastCat(items) => {
            for (child, _weight) in items {
                collect_i32_spans(child, spans);
            }
        }
        MiniASTI32::SlowCat(items) => {
            for (child, _weight) in items {
                collect_i32_spans(child, spans);
            }
        }
        MiniASTI32::RandomChoice(items, _) | MiniASTI32::Stack(items) => {
            for child in items {
                collect_i32_spans(child, spans);
            }
        }
        MiniASTI32::Fast(pattern, factor) => {
            collect_i32_spans(pattern, spans);
            collect_f64_spans(factor, spans);
        }
        MiniASTI32::Slow(pattern, factor) => {
            collect_i32_spans(pattern, spans);
            collect_f64_spans(factor, spans);
        }
        MiniASTI32::Replicate(pattern, _count) => {
            collect_i32_spans(pattern, spans);
        }
        MiniASTI32::Degrade(pattern, _prob, _) => {
            collect_i32_spans(pattern, spans);
        }
        MiniASTI32::Euclidean {
            pattern,
            pulses,
            steps,
            rotation,
        } => {
            collect_i32_spans(pattern, spans);
            collect_u32_spans(pulses, spans);
            collect_u32_spans(steps, spans);
            if let Some(rot) = rotation {
                collect_i32_spans(rot, spans);
            }
        }
    }
}

/// Assign deterministic seeds to all `RandomChoice` and `Degrade` nodes.
///
/// Called after parsing to ensure that the same pattern string always produces
/// the same seed assignments, regardless of construction order or concurrent
/// parses. The counter is incremented depth-first, matching the left-to-right
/// source order of `?` and `|` operators (like Strudel's `var seed = 0`).
pub fn assign_seeds(ast: &mut MiniAST, counter: &mut u64) {
    match ast {
        MiniAST::Pure(_) | MiniAST::Rest(_) => {}
        MiniAST::List(located) => {
            for child in &mut located.node {
                assign_seeds(child, counter);
            }
        }
        MiniAST::Sequence(items) | MiniAST::FastCat(items) | MiniAST::SlowCat(items) => {
            for (child, _) in items {
                assign_seeds(child, counter);
            }
        }
        MiniAST::Stack(items) => {
            for child in items {
                assign_seeds(child, counter);
            }
        }
        MiniAST::RandomChoice(items, seed) => {
            *seed = *counter;
            *counter += 1;
            for child in items {
                assign_seeds(child, counter);
            }
        }
        MiniAST::Fast(pattern, factor) => {
            assign_seeds(pattern, counter);
            assign_seeds_f64(factor, counter);
        }
        MiniAST::Slow(pattern, factor) => {
            assign_seeds(pattern, counter);
            assign_seeds(factor, counter);
        }
        MiniAST::Replicate(pattern, _) => {
            assign_seeds(pattern, counter);
        }
        MiniAST::Degrade(pattern, _, seed) => {
            *seed = *counter;
            *counter += 1;
            assign_seeds(pattern, counter);
        }
        MiniAST::Euclidean {
            pattern,
            pulses,
            steps,
            rotation,
        } => {
            assign_seeds(pattern, counter);
            assign_seeds_u32(pulses, counter);
            assign_seeds_u32(steps, counter);
            if let Some(rot) = rotation {
                assign_seeds_i32(rot, counter);
            }
        }
    }
}

/// Assign deterministic seeds to `RandomChoice`/`Degrade` nodes in an f64 AST.
pub fn assign_seeds_f64(ast: &mut MiniASTF64, counter: &mut u64) {
    match ast {
        MiniASTF64::Pure(_) | MiniASTF64::Rest(_) => {}
        MiniASTF64::List(located) => {
            for child in &mut located.node {
                assign_seeds_f64(child, counter);
            }
        }
        MiniASTF64::Sequence(items) | MiniASTF64::FastCat(items) | MiniASTF64::SlowCat(items) => {
            for (child, _) in items {
                assign_seeds_f64(child, counter);
            }
        }
        MiniASTF64::Stack(items) => {
            for child in items {
                assign_seeds_f64(child, counter);
            }
        }
        MiniASTF64::RandomChoice(items, seed) => {
            *seed = *counter;
            *counter += 1;
            for child in items {
                assign_seeds_f64(child, counter);
            }
        }
        MiniASTF64::Fast(pattern, factor) | MiniASTF64::Slow(pattern, factor) => {
            assign_seeds_f64(pattern, counter);
            assign_seeds_f64(factor, counter);
        }
        MiniASTF64::Replicate(pattern, _) => {
            assign_seeds_f64(pattern, counter);
        }
        MiniASTF64::Degrade(pattern, _, seed) => {
            *seed = *counter;
            *counter += 1;
            assign_seeds_f64(pattern, counter);
        }
        MiniASTF64::Euclidean {
            pattern,
            pulses,
            steps,
            rotation,
        } => {
            assign_seeds_f64(pattern, counter);
            assign_seeds_u32(pulses, counter);
            assign_seeds_u32(steps, counter);
            if let Some(rot) = rotation {
                assign_seeds_i32(rot, counter);
            }
        }
    }
}

/// Assign deterministic seeds to `RandomChoice`/`Degrade` nodes in a u32 AST.
pub fn assign_seeds_u32(ast: &mut MiniASTU32, counter: &mut u64) {
    match ast {
        MiniASTU32::Pure(_) | MiniASTU32::Rest(_) => {}
        MiniASTU32::List(located) => {
            for child in &mut located.node {
                assign_seeds_u32(child, counter);
            }
        }
        MiniASTU32::Sequence(items) | MiniASTU32::FastCat(items) | MiniASTU32::SlowCat(items) => {
            for (child, _) in items {
                assign_seeds_u32(child, counter);
            }
        }
        MiniASTU32::Stack(items) => {
            for child in items {
                assign_seeds_u32(child, counter);
            }
        }
        MiniASTU32::RandomChoice(items, seed) => {
            *seed = *counter;
            *counter += 1;
            for child in items {
                assign_seeds_u32(child, counter);
            }
        }
        MiniASTU32::Fast(pattern, factor) | MiniASTU32::Slow(pattern, factor) => {
            assign_seeds_u32(pattern, counter);
            assign_seeds_f64(factor, counter);
        }
        MiniASTU32::Replicate(pattern, _) => {
            assign_seeds_u32(pattern, counter);
        }
        MiniASTU32::Degrade(pattern, _, seed) => {
            *seed = *counter;
            *counter += 1;
            assign_seeds_u32(pattern, counter);
        }
        MiniASTU32::Euclidean {
            pattern,
            pulses,
            steps,
            rotation,
        } => {
            assign_seeds_u32(pattern, counter);
            assign_seeds_u32(pulses, counter);
            assign_seeds_u32(steps, counter);
            if let Some(rot) = rotation {
                assign_seeds_i32(rot, counter);
            }
        }
    }
}

/// Assign deterministic seeds to `RandomChoice`/`Degrade` nodes in an i32 AST.
pub fn assign_seeds_i32(ast: &mut MiniASTI32, counter: &mut u64) {
    match ast {
        MiniASTI32::Pure(_) | MiniASTI32::Rest(_) => {}
        MiniASTI32::List(located) => {
            for child in &mut located.node {
                assign_seeds_i32(child, counter);
            }
        }
        MiniASTI32::Sequence(items) | MiniASTI32::FastCat(items) | MiniASTI32::SlowCat(items) => {
            for (child, _) in items {
                assign_seeds_i32(child, counter);
            }
        }
        MiniASTI32::Stack(items) => {
            for child in items {
                assign_seeds_i32(child, counter);
            }
        }
        MiniASTI32::RandomChoice(items, seed) => {
            *seed = *counter;
            *counter += 1;
            for child in items {
                assign_seeds_i32(child, counter);
            }
        }
        MiniASTI32::Fast(pattern, factor) | MiniASTI32::Slow(pattern, factor) => {
            assign_seeds_i32(pattern, counter);
            assign_seeds_f64(factor, counter);
        }
        MiniASTI32::Replicate(pattern, _) => {
            assign_seeds_i32(pattern, counter);
        }
        MiniASTI32::Degrade(pattern, _, seed) => {
            *seed = *counter;
            *counter += 1;
            assign_seeds_i32(pattern, counter);
        }
        MiniASTI32::Euclidean {
            pattern,
            pulses,
            steps,
            rotation,
        } => {
            assign_seeds_i32(pattern, counter);
            assign_seeds_u32(pulses, counter);
            assign_seeds_u32(steps, counter);
            if let Some(rot) = rotation {
                assign_seeds_i32(rot, counter);
            }
        }
    }
}
