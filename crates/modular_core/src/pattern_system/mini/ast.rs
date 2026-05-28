//! Abstract Syntax Tree types for mini notation.
//!
//! These types represent the parsed structure of a mini notation pattern,
//! including source span information for editor highlighting.
//!
//! Parsing itself lives TypeScript-side (`src/main/dsl/miniNotation/`). The
//! DSL ships each `$cycle` / `$iCycle` pattern as a JSON `{ ast, source,
//! all_spans }` payload built by `$p(...)`; the Rust side deserializes it
//! here and forwards the `MiniAST` to `convert::convert` to build the
//! `Pattern<T>`.

use crate::pattern_system::SourceSpan;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Source location in the original pattern string.
///
/// Serialized as `{ "node": ..., "span": {...} }` to match the TS
/// `Located<T>` shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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

    /// Polymeter: `{a b, c d e}` or `{...}%n`. Each child sequence is scaled
    /// so that all children fit into `steps_per_cycle` steps per cycle, then
    /// stacked. Default `steps_per_cycle` is the step-count of the first
    /// child.
    Polymeter {
        children: Vec<MiniASTI32>,
        steps_per_cycle: Option<Box<MiniASTF64>>,
    },
}

/// AST for unsigned integer patterns (used for euclidean pulses/steps).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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

    /// Polymeter: `{...}` with optional `%n`. See `MiniAST::Polymeter`.
    Polymeter {
        children: Vec<MiniASTU32>,
        steps_per_cycle: Option<Box<MiniASTF64>>,
    },
}

/// AST for f64-valued patterns (used for fast/slow factors).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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

    /// Polymeter: `{...}` with optional `%n`. See `MiniAST::Polymeter`.
    Polymeter {
        children: Vec<MiniASTF64>,
        steps_per_cycle: Option<Box<MiniASTF64>>,
    },
}

/// The main AST node type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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

    /// Polymeter: `{a b, c d e}` with optional `%n` steps-per-cycle override.
    /// Each child pattern is scaled so its step count maps to
    /// `steps_per_cycle`; all scaled children are stacked. Default
    /// `steps_per_cycle` is the step-count of the first child sequence.
    /// Adapted from strudel's `polymeter` alignment in `packages/mini/mini.mjs`.
    Polymeter {
        children: Vec<MiniAST>,
        steps_per_cycle: Option<Box<MiniASTF64>>,
    },
}

/// Atomic value types produced by the `$p()` DSL parser.
///
/// The reduced set from strudel's krill grammar: `Number` covers bare
/// numeric atoms, `Hz` covers frequency-tagged numbers (`440hz`), and
/// `Note` covers pitched letter-octave atoms (`c4`, `d#3`, `eb5`).
/// Every other atom form (`m60` midi shorthand, sample-name identifiers,
/// `2v` voltage, module references, quoted strings) has been removed from
/// the grammar and is not representable here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum AtomValue {
    /// Numeric value. Module-level semantics decide the interpretation:
    /// `$cycle` maps a bare `Number` to volts (1V/oct); `$iCycle` maps it
    /// to an integer scale degree.
    Number(f64),

    /// Frequency in Hz.
    Hz(f64),

    /// Musical note (e.g., c4, a#3, bb5).
    Note {
        letter: char,
        /// Accidental: '#' for sharp, 'b' for flat.
        accidental: Option<char>,
        octave: Option<i32>,
    },
}

impl AtomValue {
    /// Convert to f64 (MIDI note number) if possible.
    ///
    /// Used by generic numeric converters; `SeqValue` and `IntervalValue`
    /// handle each variant directly in their own `from_atom` to avoid the
    /// loss of precision this method carries for `Hz`.
    pub fn to_f64(&self) -> Option<f64> {
        match self {
            AtomValue::Number(n) => Some(*n),
            AtomValue::Hz(h) => Some(*h),
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
        }
    }
}

#[cfg(test)]
pub(crate) mod test_builders {
    //! `#[cfg(test)]`-only helpers for constructing `MiniAST` nodes
    //! programmatically. These replace the Pest parser in existing Rust
    //! tests — see `pattern_system::mini::tests` / convert tests.

    use super::{AtomValue, Located, MiniAST, MiniASTF64, MiniASTI32, MiniASTU32};
    use crate::pattern_system::SourceSpan;

    pub fn span(start: usize, end: usize) -> SourceSpan {
        SourceSpan::new(start, end)
    }

    pub fn pure_number(n: f64, start: usize, end: usize) -> MiniAST {
        MiniAST::Pure(Located::new(AtomValue::Number(n), start, end))
    }

    pub fn pure_note(letter: char, octave: i32, start: usize, end: usize) -> MiniAST {
        MiniAST::Pure(Located::new(
            AtomValue::Note {
                letter: letter.to_ascii_lowercase(),
                accidental: None,
                octave: Some(octave),
            },
            start,
            end,
        ))
    }

    pub fn rest(start: usize, end: usize) -> MiniAST {
        MiniAST::Rest(span(start, end))
    }

    pub fn seq(items: Vec<(MiniAST, Option<f64>)>) -> MiniAST {
        MiniAST::Sequence(items)
    }

    pub fn fastcat(items: Vec<(MiniAST, Option<f64>)>) -> MiniAST {
        MiniAST::FastCat(items)
    }

    pub fn slowcat(items: Vec<(MiniAST, Option<f64>)>) -> MiniAST {
        MiniAST::SlowCat(items)
    }

    pub fn fast_f64(pattern: MiniAST, factor: f64, start: usize, end: usize) -> MiniAST {
        MiniAST::Fast(
            Box::new(pattern),
            Box::new(MiniASTF64::Pure(Located::new(factor, start, end))),
        )
    }

    pub fn euclidean(pattern: MiniAST, pulses: u32, steps: u32) -> MiniAST {
        MiniAST::Euclidean {
            pattern: Box::new(pattern),
            pulses: Box::new(MiniASTU32::Pure(Located::new(pulses, 0, 0))),
            steps: Box::new(MiniASTU32::Pure(Located::new(steps, 0, 0))),
            rotation: None,
        }
    }

    #[allow(dead_code)]
    pub fn euclidean_with_rotation(
        pattern: MiniAST,
        pulses: u32,
        steps: u32,
        rotation: i32,
    ) -> MiniAST {
        MiniAST::Euclidean {
            pattern: Box::new(pattern),
            pulses: Box::new(MiniASTU32::Pure(Located::new(pulses, 0, 0))),
            steps: Box::new(MiniASTU32::Pure(Located::new(steps, 0, 0))),
            rotation: Some(Box::new(MiniASTI32::Pure(Located::new(rotation, 0, 0)))),
        }
    }

    pub fn replicate(pattern: MiniAST, count: u32) -> MiniAST {
        MiniAST::Replicate(Box::new(pattern), count)
    }

    pub fn degrade(pattern: MiniAST, prob: Option<f64>, seed: u64) -> MiniAST {
        MiniAST::Degrade(Box::new(pattern), prob, seed)
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
        MiniAST::Polymeter {
            children,
            steps_per_cycle,
        } => {
            for child in children {
                collect_leaf_spans_recursive(child, spans);
            }
            if let Some(spc) = steps_per_cycle {
                collect_f64_spans(spc, spans);
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
        MiniASTF64::Polymeter {
            children,
            steps_per_cycle,
        } => {
            for child in children {
                collect_f64_spans(child, spans);
            }
            if let Some(spc) = steps_per_cycle {
                collect_f64_spans(spc, spans);
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
        MiniASTU32::Polymeter {
            children,
            steps_per_cycle,
        } => {
            for child in children {
                collect_u32_spans(child, spans);
            }
            if let Some(spc) = steps_per_cycle {
                collect_f64_spans(spc, spans);
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
        MiniASTI32::Polymeter {
            children,
            steps_per_cycle,
        } => {
            for child in children {
                collect_i32_spans(child, spans);
            }
            if let Some(spc) = steps_per_cycle {
                collect_f64_spans(spc, spans);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_builders::*;
    use super::*;

    #[test]
    fn test_collect_leaf_spans_basic() {
        // Sequence of three Pure atoms: "0 1 2" -> spans (0,1), (2,3), (4,5).
        let ast = seq(vec![
            (pure_number(0.0, 0, 1), None),
            (pure_number(1.0, 2, 3), None),
            (pure_number(2.0, 4, 5), None),
        ]);
        let spans = collect_leaf_spans(&ast);
        assert_eq!(spans, vec![(0, 1), (2, 3), (4, 5)]);
    }

    #[test]
    fn test_collect_leaf_spans_includes_modifier_factors() {
        // "c*[1 2]" — collector should include c, 1, 2 spans.
        let ast = MiniAST::Fast(
            Box::new(pure_note('c', 4, 0, 1)),
            Box::new(MiniASTF64::FastCat(vec![
                (MiniASTF64::Pure(Located::new(1.0, 3, 4)), None),
                (MiniASTF64::Pure(Located::new(2.0, 5, 6)), None),
            ])),
        );
        let spans = collect_leaf_spans(&ast);
        assert_eq!(spans.len(), 3);
        assert!(spans.contains(&(0, 1)));
        assert!(spans.contains(&(3, 4)));
        assert!(spans.contains(&(5, 6)));
    }

    #[test]
    fn test_atom_value_to_f64() {
        assert_eq!(AtomValue::Number(42.0).to_f64(), Some(42.0));
        assert_eq!(AtomValue::Hz(440.0).to_f64(), Some(440.0));

        let c4 = AtomValue::Note {
            letter: 'c',
            accidental: None,
            octave: Some(4),
        };
        assert_eq!(c4.to_f64(), Some(60.0));

        let a4 = AtomValue::Note {
            letter: 'a',
            accidental: None,
            octave: Some(4),
        };
        assert_eq!(a4.to_f64(), Some(69.0));
    }

    #[test]
    fn test_roundtrip_serde_number() {
        let ast = pure_number(1.5, 0, 3);
        let json = serde_json::to_value(&ast).unwrap();
        let decoded: MiniAST = serde_json::from_value(json).unwrap();
        assert_eq!(ast, decoded);
    }

    #[test]
    fn test_roundtrip_serde_rest() {
        let ast = rest(5, 6);
        let json = serde_json::to_value(&ast).unwrap();
        let decoded: MiniAST = serde_json::from_value(json).unwrap();
        assert_eq!(ast, decoded);
    }

    #[test]
    fn test_roundtrip_serde_sequence_with_weight() {
        let ast = seq(vec![
            (pure_number(0.0, 0, 1), Some(3.0)),
            (pure_number(1.0, 3, 4), None),
        ]);
        let json = serde_json::to_value(&ast).unwrap();
        let decoded: MiniAST = serde_json::from_value(json).unwrap();
        assert_eq!(ast, decoded);
    }

    #[test]
    fn test_roundtrip_serde_euclidean() {
        let ast = euclidean(pure_number(1.0, 0, 1), 3, 8);
        let json = serde_json::to_value(&ast).unwrap();
        let decoded: MiniAST = serde_json::from_value(json).unwrap();
        assert_eq!(ast, decoded);
    }
}

/// Assign deterministic seeds to all `RandomChoice` and `Degrade` nodes.
///
/// Called after parsing to ensure that the same pattern string always produces
/// the same seed assignments, regardless of construction order or concurrent
/// parses. The counter is incremented depth-first, matching the left-to-right
/// source order of `?` and `|` operators (like Strudel's `var seed = 0`).
///
/// Now that parsing lives TypeScript-side, seeds are assigned there and this
/// function is retained only for any test-only AST fixtures that need it.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn assign_seeds(ast: &mut MiniAST, counter: &mut u64) {
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
        MiniAST::Fast(pattern, _factor) => {
            assign_seeds(pattern, counter);
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
        MiniAST::Euclidean { pattern, .. } => {
            assign_seeds(pattern, counter);
        }
        MiniAST::Polymeter { children, .. } => {
            for child in children {
                assign_seeds(child, counter);
            }
        }
    }
}
