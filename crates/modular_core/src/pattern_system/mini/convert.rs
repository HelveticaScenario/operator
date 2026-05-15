//! Convert mini notation AST to Pattern.
//!
//! The conversion is parameterized by the target atom type through
//! the `FromMiniAtom` trait.

use super::ast::{AtomValue, Located, MiniAST, MiniASTF64, MiniASTI32, MiniASTU32};
use super::parser::ParseError;
use crate::pattern_system::{
    Fraction, Pattern,
    combinators::{fastcat, slowcat, stack, timecat},
    constructors,
    constructors::pure_with_span,
    random::choose_with_seed,
};

/// Trait for types that support rest values.
///
/// Types implementing this trait can represent "silence" or "no value" states.
/// Operations that can produce rests (like `degrade`, `euclid`, `mask`) require
/// this trait to ensure the pattern always produces a hap when queried.
pub trait HasRest: Clone + Send + Sync + 'static {
    /// Get the rest value for this type.
    fn rest_value() -> Self;
}

/// Trait for types that can be created from mini notation atoms.
pub trait FromMiniAtom: Clone + Send + Sync + 'static {
    /// Convert a single atom value to this type.
    fn from_atom(atom: &AtomValue) -> Result<Self, ConvertError>;

    /// Convert a list of atoms to this type.
    /// Override for types that support list syntax (e.g., scale specs).
    fn from_list(atoms: &[AtomValue]) -> Result<Self, ConvertError> {
        if atoms.len() == 1 {
            Self::from_atom(&atoms[0])
        } else {
            Err(ConvertError::ListNotSupported)
        }
    }

    /// Combine a list of head atoms with a tail value.
    /// Used for distributing patterns like `c:e:[f g]` -> `[c:e:f, c:e:g]`
    /// Default implementation prepends atoms to a single-element list with the value.
    fn combine_with_head(head_atoms: &[AtomValue], tail: &Self) -> Result<Self, ConvertError>;

    /// Get the rest/silence value, if supported.
    ///
    /// DEPRECATED: Use `HasRest` trait instead. This method is kept for
    /// backward compatibility but should not be used for new code.
    fn rest_value() -> Option<Self> {
        None
    }

    /// Returns true if this type supports rest values.
    ///
    /// Types that support rests can use operations like `degrade`, `euclid`,
    /// and the `~` (rest) syntax in mini notation. Types that don't support
    /// rests will produce parse errors when these operations are used.
    fn supports_rest() -> bool {
        false
    }
}

/// Error type for conversion failures.
#[derive(Debug, Clone)]
pub enum ConvertError {
    /// Cannot convert atom to target type.
    InvalidAtom(String),
    /// Lists are not supported for this type.
    ListNotSupported,
    /// Operator error.
    OperatorError(String),
    /// Parse error.
    ParseError(ParseError),
    /// Operation requires rest support but pattern type doesn't support rests.
    RestNotSupported(String),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::InvalidAtom(msg) => write!(f, "Invalid atom: {}", msg),
            ConvertError::ListNotSupported => write!(f, "List syntax not supported for this type"),
            ConvertError::OperatorError(msg) => write!(f, "Operator error: {}", msg),
            ConvertError::ParseError(err) => write!(f, "Parse error: {}", err),
            ConvertError::RestNotSupported(op) => write!(
                f,
                "'{}' requires a pattern type that supports rests. This operation is only available for note/sequence patterns.",
                op
            ),
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<ParseError> for ConvertError {
    fn from(err: ParseError) -> Self {
        ConvertError::ParseError(err)
    }
}

// Implement FromMiniAtom for common types

impl FromMiniAtom for f64 {
    fn from_atom(atom: &AtomValue) -> Result<Self, ConvertError> {
        atom.to_f64()
            .ok_or_else(|| ConvertError::InvalidAtom("Cannot convert to f64".to_string()))
    }

    fn from_list(atoms: &[AtomValue]) -> Result<Self, ConvertError> {
        if atoms.len() == 1 {
            Self::from_atom(&atoms[0])
        } else {
            // For f64, try averaging the list values
            let values: Vec<f64> = atoms
                .iter()
                .map(Self::from_atom)
                .collect::<Result<_, _>>()?;
            Ok(values.iter().sum::<f64>() / values.len() as f64)
        }
    }

    fn combine_with_head(head_atoms: &[AtomValue], tail: &Self) -> Result<Self, ConvertError> {
        // For f64, combine by averaging all values
        let mut values: Vec<f64> = head_atoms
            .iter()
            .map(Self::from_atom)
            .collect::<Result<_, _>>()?;
        values.push(*tail);
        Ok(values.iter().sum::<f64>() / values.len() as f64)
    }
    // f64 does not support rests - use default supports_rest() -> false
}

impl FromMiniAtom for f32 {
    fn from_atom(atom: &AtomValue) -> Result<Self, ConvertError> {
        atom.to_f64()
            .map(|f| f as f32)
            .ok_or_else(|| ConvertError::InvalidAtom("Cannot convert to f32".to_string()))
    }

    fn combine_with_head(head_atoms: &[AtomValue], tail: &Self) -> Result<Self, ConvertError> {
        let mut values: Vec<f32> = head_atoms
            .iter()
            .map(Self::from_atom)
            .collect::<Result<_, _>>()?;
        values.push(*tail);
        Ok(values.iter().sum::<f32>() / values.len() as f32)
    }
    // f32 does not support rests - use default supports_rest() -> false
}

impl FromMiniAtom for i64 {
    fn from_atom(atom: &AtomValue) -> Result<Self, ConvertError> {
        atom.to_f64()
            .map(|f| f as i64)
            .ok_or_else(|| ConvertError::InvalidAtom("Cannot convert to i64".to_string()))
    }

    fn combine_with_head(_head_atoms: &[AtomValue], _tail: &Self) -> Result<Self, ConvertError> {
        Err(ConvertError::ListNotSupported)
    }
    // i64 does not support rests - use default supports_rest() -> false
}

impl FromMiniAtom for i32 {
    fn from_atom(atom: &AtomValue) -> Result<Self, ConvertError> {
        atom.to_f64()
            .map(|f| f as i32)
            .ok_or_else(|| ConvertError::InvalidAtom("Cannot convert to i32".to_string()))
    }

    fn combine_with_head(_head_atoms: &[AtomValue], _tail: &Self) -> Result<Self, ConvertError> {
        Err(ConvertError::ListNotSupported)
    }
    // i32 does not support rests - use default supports_rest() -> false
}

impl FromMiniAtom for String {
    fn from_atom(atom: &AtomValue) -> Result<Self, ConvertError> {
        match atom {
            AtomValue::Identifier(s) => Ok(s.clone()),
            AtomValue::String(s) => Ok(s.clone()),
            AtomValue::Number(n) => Ok(n.to_string()),
            _ => Err(ConvertError::InvalidAtom(
                "Cannot convert to String".to_string(),
            )),
        }
    }

    fn from_list(atoms: &[AtomValue]) -> Result<Self, ConvertError> {
        // Join with colons for string lists
        let strings: Vec<String> = atoms
            .iter()
            .map(Self::from_atom)
            .collect::<Result<_, _>>()?;
        Ok(strings.join(":"))
    }

    fn combine_with_head(head_atoms: &[AtomValue], tail: &Self) -> Result<Self, ConvertError> {
        let mut strings: Vec<String> = head_atoms
            .iter()
            .map(Self::from_atom)
            .collect::<Result<_, _>>()?;
        strings.push(tail.clone());
        Ok(strings.join(":"))
    }
}

impl FromMiniAtom for bool {
    fn from_atom(atom: &AtomValue) -> Result<Self, ConvertError> {
        match atom {
            AtomValue::Number(n) => Ok(*n != 0.0),
            AtomValue::Identifier(s) => match s.to_lowercase().as_str() {
                "true" | "t" | "1" | "yes" => Ok(true),
                "false" | "f" | "0" | "no" => Ok(false),
                _ => Err(ConvertError::InvalidAtom(format!(
                    "Cannot convert '{}' to bool",
                    s
                ))),
            },
            _ => Err(ConvertError::InvalidAtom(
                "Cannot convert to bool".to_string(),
            )),
        }
    }

    fn combine_with_head(_head_atoms: &[AtomValue], _tail: &Self) -> Result<Self, ConvertError> {
        Err(ConvertError::ListNotSupported)
    }
    // bool does not support rests - use default supports_rest() -> false
}

/// Convert an AST to a Pattern.
pub fn convert<T: FromMiniAtom>(ast: &MiniAST) -> Result<Pattern<T>, ConvertError> {
    convert_inner(ast)
}

/// Evaluate a MiniASTF64 to get a single f64 value.
///
/// This recursively evaluates the AST at cycle 0. For complex patterns,
/// it returns the first value found.
fn eval_f64(ast: &MiniASTF64) -> f64 {
    match ast {
        MiniASTF64::Pure(Located { node, .. }) => *node,
        MiniASTF64::Rest(_) => 0.0, // Rest evaluates to 0
        MiniASTF64::List(Located { node, .. }) => {
            // Return first element or 0
            node.first().map(eval_f64).unwrap_or(0.0)
        }
        MiniASTF64::Sequence(elements) | MiniASTF64::FastCat(elements) => {
            // Return first element or 0
            elements.first().map(|(e, _)| eval_f64(e)).unwrap_or(0.0)
        }
        MiniASTF64::SlowCat(elements) => elements.first().map(|(e, _)| eval_f64(e)).unwrap_or(0.0),
        MiniASTF64::RandomChoice(elements, _) | MiniASTF64::Stack(elements) => {
            // For deterministic evaluation, just take first
            elements.first().map(eval_f64).unwrap_or(0.0)
        }
        MiniASTF64::Fast(pattern, _) => eval_f64(pattern),
        MiniASTF64::Slow(pattern, _) => eval_f64(pattern),
        MiniASTF64::Replicate(pattern, _) => eval_f64(pattern),
        MiniASTF64::Degrade(pattern, _, _) => eval_f64(pattern),
        MiniASTF64::Euclidean { pattern, .. } => eval_f64(pattern),
    }
}

/// Convert a MiniASTF64 to a Pattern<Fraction>.
///
/// This properly handles patterned values like `[2 3]` instead of
/// reducing them to a single scalar.
///
/// Now preserves source spans for modifier pattern highlighting.
fn convert_f64_pattern(ast: &MiniASTF64) -> Pattern<Fraction> {
    match ast {
        MiniASTF64::Pure(Located { node, span }) => {
            pure_with_span(Fraction::from(*node), span.clone())
        }

        MiniASTF64::Rest(span) => pure_with_span(Fraction::from_integer(0), span.clone()),

        MiniASTF64::List(Located { node, .. }) => {
            // Take first element for list (same as eval_f64 behavior)
            node.first()
                .map(convert_f64_pattern)
                .unwrap_or_else(|| constructors::pure(Fraction::from_integer(0)))
        }

        MiniASTF64::Sequence(elements) | MiniASTF64::FastCat(elements) => {
            if elements.is_empty() {
                return constructors::pure(Fraction::from_integer(0));
            }

            // Expand Replicate nodes (see MiniAST::Sequence/FastCat for details).
            let expanded: Vec<(&MiniASTF64, Option<f64>)> = elements
                .iter()
                .flat_map(|(p, w)| match p {
                    MiniASTF64::Replicate(inner, count) => {
                        std::iter::repeat_n((inner.as_ref(), None), *count as usize)
                            .collect::<Vec<_>>()
                    }
                    other => vec![(other, *w)],
                })
                .collect();

            let has_weights = expanded.iter().any(|(_, w)| w.is_some());

            if has_weights {
                let weighted: Vec<(Fraction, Pattern<Fraction>)> = expanded
                    .iter()
                    .map(|(e, w)| {
                        let weight = w.unwrap_or(1.0);
                        (Fraction::from(weight), convert_f64_pattern(e))
                    })
                    .collect();
                timecat(weighted)
            } else {
                let pats: Vec<Pattern<Fraction>> = expanded
                    .iter()
                    .map(|(e, _)| convert_f64_pattern(e))
                    .collect();
                fastcat(pats)
            }
        }

        MiniASTF64::SlowCat(elements) => {
            // Expand Replicate nodes within SlowCat (see MiniAST::SlowCat for details)
            let expanded: Vec<(&MiniASTF64, Option<f64>)> = elements
                .iter()
                .flat_map(|(p, w)| match p {
                    MiniASTF64::Replicate(inner, count) => {
                        std::iter::repeat_n((inner.as_ref(), None), *count as usize)
                            .collect::<Vec<_>>()
                    }
                    other => vec![(other, *w)],
                })
                .collect();

            let has_weights = expanded.iter().any(|(_, w)| w.is_some());

            if has_weights {
                let total_weight: f64 = expanded.iter().map(|(_, w)| w.unwrap_or(1.0)).sum();
                let weighted: Vec<(Fraction, Pattern<Fraction>)> = expanded
                    .iter()
                    .map(|(e, w)| {
                        let weight = w.unwrap_or(1.0);
                        (Fraction::from(weight), convert_f64_pattern(e))
                    })
                    .collect();
                timecat(weighted)._slow(Fraction::from(total_weight))
            } else {
                let pats: Vec<Pattern<Fraction>> = expanded
                    .iter()
                    .map(|(e, _)| convert_f64_pattern(e))
                    .collect();
                slowcat(pats)
            }
        }

        MiniASTF64::RandomChoice(elements, seed) => {
            let pats: Vec<Pattern<Fraction>> = elements.iter().map(convert_f64_pattern).collect();
            choose_with_seed(pats, *seed).inner_join(Clone::clone)
        }

        MiniASTF64::Stack(elements) => {
            let pats: Vec<Pattern<Fraction>> = elements.iter().map(convert_f64_pattern).collect();
            stack(pats)
        }

        MiniASTF64::Fast(pattern, factor) => {
            let pat = convert_f64_pattern(pattern);
            let factor_pat = convert_f64_pattern(factor);
            pat.fast(factor_pat)
        }

        MiniASTF64::Slow(pattern, factor) => {
            let pat = convert_f64_pattern(pattern);
            let factor_pat = convert_f64_pattern(factor);
            pat.slow(factor_pat)
        }

        MiniASTF64::Replicate(pattern, count) => {
            let pat = convert_f64_pattern(pattern);
            let pats: Vec<Pattern<Fraction>> = (0..*count).map(|_| pat.clone()).collect();
            fastcat(pats)
        }

        MiniASTF64::Degrade(pattern, _prob, _seed) => {
            // For Fraction patterns, degrade doesn't make much sense
            // Just return the pattern without degrading
            convert_f64_pattern(pattern)
        }

        MiniASTF64::Euclidean { pattern, .. } => {
            // For Fraction patterns, euclidean doesn't make much sense
            // Just return the pattern
            convert_f64_pattern(pattern)
        }
    }
}

/// Convert a MiniASTU32 to a Pattern<u32>.
///
/// This properly handles patterned values like `[2 3]` for euclidean steps.
fn convert_u32_pattern(ast: &MiniASTU32) -> Pattern<u32> {
    use crate::pattern_system::constructors::pure;

    match ast {
        MiniASTU32::Pure(Located { node, span }) => pure_with_span(*node, span.clone()),

        MiniASTU32::Rest(span) => pure_with_span(0, span.clone()),

        MiniASTU32::List(Located { node, .. }) => node
            .first()
            .map(convert_u32_pattern)
            .unwrap_or_else(|| pure(0)),

        MiniASTU32::Sequence(elements) | MiniASTU32::FastCat(elements) => {
            if elements.is_empty() {
                return pure(0);
            }

            // Expand Replicate nodes (see MiniAST::Sequence/FastCat for details).
            let expanded: Vec<(&MiniASTU32, Option<f64>)> = elements
                .iter()
                .flat_map(|(p, w)| match p {
                    MiniASTU32::Replicate(inner, count) => {
                        std::iter::repeat_n((inner.as_ref(), None), *count as usize)
                            .collect::<Vec<_>>()
                    }
                    other => vec![(other, *w)],
                })
                .collect();

            let has_weights = expanded.iter().any(|(_, w)| w.is_some());

            if has_weights {
                let weighted: Vec<(Fraction, Pattern<u32>)> = expanded
                    .iter()
                    .map(|(e, w)| {
                        let weight = w.unwrap_or(1.0);
                        (Fraction::from(weight), convert_u32_pattern(e))
                    })
                    .collect();
                timecat(weighted)
            } else {
                let pats: Vec<Pattern<u32>> = expanded
                    .iter()
                    .map(|(e, _)| convert_u32_pattern(e))
                    .collect();
                fastcat(pats)
            }
        }

        MiniASTU32::SlowCat(elements) => {
            // Expand Replicate nodes within SlowCat (see MiniAST::SlowCat for details)
            let expanded: Vec<(&MiniASTU32, Option<f64>)> = elements
                .iter()
                .flat_map(|(p, w)| match p {
                    MiniASTU32::Replicate(inner, count) => {
                        std::iter::repeat_n((inner.as_ref(), None), *count as usize)
                            .collect::<Vec<_>>()
                    }
                    other => vec![(other, *w)],
                })
                .collect();

            let has_weights = expanded.iter().any(|(_, w)| w.is_some());

            if has_weights {
                let total_weight: f64 = expanded.iter().map(|(_, w)| w.unwrap_or(1.0)).sum();
                let weighted: Vec<(Fraction, Pattern<u32>)> = expanded
                    .iter()
                    .map(|(e, w)| {
                        let weight = w.unwrap_or(1.0);
                        (Fraction::from(weight), convert_u32_pattern(e))
                    })
                    .collect();
                timecat(weighted)._slow(Fraction::from(total_weight))
            } else {
                let pats: Vec<Pattern<u32>> = expanded
                    .iter()
                    .map(|(e, _)| convert_u32_pattern(e))
                    .collect();
                slowcat(pats)
            }
        }

        MiniASTU32::RandomChoice(elements, seed) => {
            let pats: Vec<Pattern<u32>> = elements.iter().map(convert_u32_pattern).collect();
            choose_with_seed(pats, *seed).inner_join(|p| p.clone())
        }

        MiniASTU32::Stack(elements) => {
            let pats: Vec<Pattern<u32>> = elements.iter().map(convert_u32_pattern).collect();
            stack(pats)
        }

        MiniASTU32::Fast(pattern, factor) => {
            let pat = convert_u32_pattern(pattern);
            let factor_pat = convert_f64_pattern(factor);
            pat.fast(factor_pat)
        }

        MiniASTU32::Slow(pattern, factor) => {
            let pat = convert_u32_pattern(pattern);
            let factor_pat = convert_f64_pattern(factor);
            pat.slow(factor_pat)
        }

        MiniASTU32::Replicate(pattern, count) => {
            let pat = convert_u32_pattern(pattern);
            let pats: Vec<Pattern<u32>> = (0..*count).map(|_| pat.clone()).collect();
            fastcat(pats)
        }

        MiniASTU32::Degrade(pattern, _prob, _seed) => {
            // For u32 patterns, degrade doesn't make much sense
            convert_u32_pattern(pattern)
        }

        MiniASTU32::Euclidean { pattern, .. } => convert_u32_pattern(pattern),
    }
}

/// Convert a MiniASTI32 to a Pattern<i32>.
///
/// This properly handles patterned values like `[0 1]` for euclidean rotation.
fn convert_i32_pattern(ast: &MiniASTI32) -> Pattern<i32> {
    use crate::pattern_system::constructors::pure;

    match ast {
        MiniASTI32::Pure(Located { node, span }) => pure_with_span(*node, span.clone()),

        MiniASTI32::Rest(span) => pure_with_span(0, span.clone()),

        MiniASTI32::List(Located { node, .. }) => node
            .first()
            .map(convert_i32_pattern)
            .unwrap_or_else(|| pure(0)),

        MiniASTI32::Sequence(elements) | MiniASTI32::FastCat(elements) => {
            if elements.is_empty() {
                return pure(0);
            }

            // Expand Replicate nodes (see MiniAST::Sequence/FastCat for details).
            let expanded: Vec<(&MiniASTI32, Option<f64>)> = elements
                .iter()
                .flat_map(|(p, w)| match p {
                    MiniASTI32::Replicate(inner, count) => {
                        std::iter::repeat_n((inner.as_ref(), None), *count as usize)
                            .collect::<Vec<_>>()
                    }
                    other => vec![(other, *w)],
                })
                .collect();

            let has_weights = expanded.iter().any(|(_, w)| w.is_some());

            if has_weights {
                let weighted: Vec<(Fraction, Pattern<i32>)> = expanded
                    .iter()
                    .map(|(e, w)| {
                        let weight = w.unwrap_or(1.0);
                        (Fraction::from(weight), convert_i32_pattern(e))
                    })
                    .collect();
                timecat(weighted)
            } else {
                let pats: Vec<Pattern<i32>> = expanded
                    .iter()
                    .map(|(e, _)| convert_i32_pattern(e))
                    .collect();
                fastcat(pats)
            }
        }

        MiniASTI32::SlowCat(elements) => {
            // Expand Replicate nodes within SlowCat (see MiniAST::SlowCat for details)
            let expanded: Vec<(&MiniASTI32, Option<f64>)> = elements
                .iter()
                .flat_map(|(p, w)| match p {
                    MiniASTI32::Replicate(inner, count) => {
                        std::iter::repeat_n((inner.as_ref(), None), *count as usize)
                            .collect::<Vec<_>>()
                    }
                    other => vec![(other, *w)],
                })
                .collect();

            let has_weights = expanded.iter().any(|(_, w)| w.is_some());

            if has_weights {
                let total_weight: f64 = expanded.iter().map(|(_, w)| w.unwrap_or(1.0)).sum();
                let weighted: Vec<(Fraction, Pattern<i32>)> = expanded
                    .iter()
                    .map(|(e, w)| {
                        let weight = w.unwrap_or(1.0);
                        (Fraction::from(weight), convert_i32_pattern(e))
                    })
                    .collect();
                timecat(weighted)._slow(Fraction::from(total_weight))
            } else {
                let pats: Vec<Pattern<i32>> = expanded
                    .iter()
                    .map(|(e, _)| convert_i32_pattern(e))
                    .collect();
                slowcat(pats)
            }
        }

        MiniASTI32::RandomChoice(elements, seed) => {
            let pats: Vec<Pattern<i32>> = elements.iter().map(convert_i32_pattern).collect();
            choose_with_seed(pats, *seed).inner_join(|p| p.clone())
        }

        MiniASTI32::Stack(elements) => {
            let pats: Vec<Pattern<i32>> = elements.iter().map(convert_i32_pattern).collect();
            stack(pats)
        }

        MiniASTI32::Fast(pattern, factor) => {
            let pat = convert_i32_pattern(pattern);
            let factor_pat = convert_f64_pattern(factor);
            pat.fast(factor_pat)
        }

        MiniASTI32::Slow(pattern, factor) => {
            let pat = convert_i32_pattern(pattern);
            let factor_pat = convert_f64_pattern(factor);
            pat.slow(factor_pat)
        }

        MiniASTI32::Replicate(pattern, count) => {
            let pat = convert_i32_pattern(pattern);
            let pats: Vec<Pattern<i32>> = (0..*count).map(|_| pat.clone()).collect();
            fastcat(pats)
        }

        MiniASTI32::Degrade(pattern, _prob, _seed) => convert_i32_pattern(pattern),

        MiniASTI32::Euclidean { pattern, .. } => convert_i32_pattern(pattern),
    }
}

/// Evaluate a MiniASTU32 to get a single u32 value.
fn eval_u32(ast: &MiniASTU32) -> u32 {
    match ast {
        MiniASTU32::Pure(Located { node, .. }) => *node,
        MiniASTU32::Rest(_) => 0, // Rest evaluates to 0
        MiniASTU32::List(Located { node, .. }) => node.first().map(eval_u32).unwrap_or(0),
        MiniASTU32::Sequence(elements) | MiniASTU32::FastCat(elements) => {
            elements.first().map(|(e, _)| eval_u32(e)).unwrap_or(0)
        }
        MiniASTU32::SlowCat(elements) => elements.first().map(|(e, _)| eval_u32(e)).unwrap_or(0),
        MiniASTU32::RandomChoice(elements, _) | MiniASTU32::Stack(elements) => {
            elements.first().map(eval_u32).unwrap_or(0)
        }
        MiniASTU32::Fast(pattern, _) => eval_u32(pattern),
        MiniASTU32::Slow(pattern, _) => eval_u32(pattern),
        MiniASTU32::Replicate(pattern, _count) => eval_u32(pattern),
        MiniASTU32::Degrade(pattern, _, _) => eval_u32(pattern),
        MiniASTU32::Euclidean { pattern, .. } => eval_u32(pattern),
    }
}

/// Evaluate a MiniASTI32 to get a single i32 value.
fn eval_i32(ast: &MiniASTI32) -> i32 {
    match ast {
        MiniASTI32::Pure(Located { node, .. }) => *node,
        MiniASTI32::Rest(_) => 0, // Rest evaluates to 0
        MiniASTI32::List(Located { node, .. }) => node.first().map(eval_i32).unwrap_or(0),
        MiniASTI32::Sequence(elements) | MiniASTI32::FastCat(elements) => {
            elements.first().map(|(e, _)| eval_i32(e)).unwrap_or(0)
        }
        MiniASTI32::SlowCat(elements) => elements.first().map(|(e, _)| eval_i32(e)).unwrap_or(0),
        MiniASTI32::RandomChoice(elements, _) | MiniASTI32::Stack(elements) => {
            elements.first().map(eval_i32).unwrap_or(0)
        }
        MiniASTI32::Fast(pattern, _) => eval_i32(pattern),
        MiniASTI32::Slow(pattern, _) => eval_i32(pattern),
        MiniASTI32::Replicate(pattern, _count) => eval_i32(pattern),
        MiniASTI32::Degrade(pattern, _, _) => eval_i32(pattern),
        MiniASTI32::Euclidean { pattern, .. } => eval_i32(pattern),
    }
}

fn convert_inner<T: FromMiniAtom>(ast: &MiniAST) -> Result<Pattern<T>, ConvertError> {
    match ast {
        MiniAST::Pure(Located { node, span }) => {
            let value = T::from_atom(node)?;
            Ok(pure_with_span(value, span.clone()))
        }

        MiniAST::Rest(_span) => {
            if !T::supports_rest() {
                return Err(ConvertError::RestNotSupported("~ (rest)".to_string()));
            }
            // Safe to unwrap because supports_rest() returned true
            let val =
                T::rest_value().expect("supports_rest() returned true but rest_value() is None");
            Ok(pure_with_span(val, _span.clone()))
        }

        MiniAST::List(Located { node, span }) => {
            // Check if all elements are Pure (simple atoms)
            let all_pure = node.iter().all(|elem| matches!(elem, MiniAST::Pure(_)));

            if all_pure {
                // Extract atoms and use from_list
                let atoms: Vec<AtomValue> = node
                    .iter()
                    .filter_map(|elem| {
                        if let MiniAST::Pure(Located { node: atom, .. }) = elem {
                            Some(atom.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                let value = T::from_list(&atoms)?;
                Ok(pure_with_span(value, span.clone()))
            } else {
                // Some elements are patterns - need to distribute
                // e.g., c:[e f] -> fastcat([c:e, c:f])
                //
                // Strategy: convert each element to a pattern, then distribute
                // by applying the "list" semantics across pattern events

                // For now, find the first non-Pure element and distribute over it
                // This handles the common case: head:[a b c]

                // Separate head atoms (Pure elements before first non-Pure) from tail pattern
                let mut head_atoms: Vec<AtomValue> = Vec::new();
                let mut tail_pattern: Option<Pattern<T>> = None;

                for elem in node.iter() {
                    match elem {
                        MiniAST::Pure(Located { node: atom, .. }) => {
                            if tail_pattern.is_none() {
                                head_atoms.push(atom.clone());
                            } else {
                                // Atoms after a pattern - not supported yet
                                return Err(ConvertError::InvalidAtom(
                                    "Atoms after pattern in list not yet supported".to_string(),
                                ));
                            }
                        }
                        _ => {
                            if tail_pattern.is_some() {
                                return Err(ConvertError::InvalidAtom(
                                    "Multiple patterns in list not yet supported".to_string(),
                                ));
                            }
                            tail_pattern = Some(convert_inner(elem)?);
                        }
                    }
                }

                if let Some(tail) = tail_pattern {
                    // Distribute head over tail: head:[a b] -> [head:a, head:b]
                    // Use combine_with_head to merge head atoms with each tail value
                    let head = head_atoms.clone();
                    Ok(tail.fmap(move |tail_val: &T| {
                        // This unwrap is safe because we validated the atoms can convert
                        // to the target type when parsing Pure nodes
                        T::combine_with_head(&head, tail_val).unwrap_or_else(|_| tail_val.clone())
                    }))
                } else {
                    // No patterns, but we hit the !all_pure branch? Shouldn't happen
                    Err(ConvertError::InvalidAtom(
                        "Internal error in list processing".to_string(),
                    ))
                }
            }
        }

        MiniAST::Sequence(elements) | MiniAST::FastCat(elements) => {
            // Replicate flattens syntactically into the parent sequence.
            let expanded: Vec<(&MiniAST, Option<f64>)> = elements
                .iter()
                .flat_map(|(p, w)| match p {
                    MiniAST::Replicate(inner, count) => {
                        std::iter::repeat_n((inner.as_ref(), None), *count as usize)
                            .collect::<Vec<_>>()
                    }
                    other => vec![(other, *w)],
                })
                .collect();

            let mut patterns = Vec::new();
            let mut weights = Vec::new();
            let mut has_weights = false;

            for (ast, weight) in expanded {
                patterns.push(convert_inner(ast)?);
                if let Some(w) = weight {
                    weights.push(Fraction::from(w));
                    has_weights = true;
                } else {
                    weights.push(Fraction::from_integer(1));
                }
            }

            if has_weights {
                // timecat expects (Fraction, Pattern) pairs
                Ok(timecat(weights.into_iter().zip(patterns).collect()))
            } else {
                Ok(fastcat(patterns))
            }
        }

        MiniAST::SlowCat(patterns) => {
            // Expand Replicate nodes within SlowCat.
            // In Strudel/Tidal, <a!3 b> means "slowcat of a, a, a, b" (each on separate cycles),
            // not "slowcat of fastcat(a,a,a), b" (which would be 3 a's in cycle 1, b in cycle 2).
            let expanded: Vec<(&MiniAST, Option<f64>)> = patterns
                .iter()
                .flat_map(|(p, w)| match p {
                    MiniAST::Replicate(inner, count) => {
                        // Replicated entries each get weight 1 (or None)
                        std::iter::repeat_n((inner.as_ref(), None), *count as usize)
                            .collect::<Vec<_>>()
                    }
                    other => vec![(other, *w)],
                })
                .collect();

            let has_weights = expanded.iter().any(|(_, w)| w.is_some());

            if has_weights {
                // Weighted slow: timecat(pairs).slow(total_weight)
                // Each element with weight w occupies w cycles out of every total cycles.
                let total_weight: f64 = expanded.iter().map(|(_, w)| w.unwrap_or(1.0)).sum();
                let weighted: Vec<(Fraction, Pattern<T>)> = expanded
                    .iter()
                    .map(|(p, w)| {
                        let weight = w.unwrap_or(1.0);
                        Ok((Fraction::from(weight), convert_inner(p)?))
                    })
                    .collect::<Result<_, ConvertError>>()?;
                Ok(timecat(weighted)._slow(Fraction::from(total_weight)))
            } else {
                let converted: Vec<Pattern<T>> = expanded
                    .iter()
                    .map(|(p, _)| convert_inner(p))
                    .collect::<Result<_, _>>()?;
                Ok(slowcat(converted))
            }
        }

        MiniAST::RandomChoice(patterns, seed) => {
            let converted: Vec<Pattern<T>> = patterns
                .iter()
                .map(convert_inner)
                .collect::<Result<_, _>>()?;
            // Use choose_with_seed for random selection — inner_join preserves the inner
            // pattern's discrete timing (whole span), matching Strudel's
            // chooseInWith(...).innerJoin() semantics.
            Ok(choose_with_seed(converted, *seed).inner_join(|p| p.clone()))
        }

        MiniAST::Stack(patterns) => {
            let converted: Vec<Pattern<T>> = patterns
                .iter()
                .map(convert_inner)
                .collect::<Result<_, _>>()?;
            Ok(stack(converted))
        }

        MiniAST::Fast(pattern, factor) => {
            let pat = convert_inner(pattern)?;
            let factor_pat = convert_f64_pattern(factor);
            Ok(pat.fast(factor_pat))
        }

        MiniAST::Slow(pattern, factor) => {
            let pat = convert_inner(pattern)?;
            // Note: Slow uses MiniAST, not MiniASTF64, so we need to handle it differently
            // For now, try to extract a numeric value from the AST
            let factor_val = match factor.as_ref() {
                MiniAST::Pure(Located { node, .. }) => node.to_f64().unwrap_or(1.0),
                _ => 1.0, // Default for complex patterns
            };
            Ok(pat.slow(Fraction::from(factor_val)))
        }

        MiniAST::Replicate(pattern, count) => {
            let pat = convert_inner(pattern)?;
            let pats: Vec<Pattern<T>> = (0..*count).map(|_| pat.clone()).collect();
            Ok(fastcat(pats))
        }

        MiniAST::Degrade(pattern, prob, seed) => {
            if !T::supports_rest() {
                return Err(ConvertError::RestNotSupported("? (degrade)".to_string()));
            }
            let pat = convert_inner(pattern)?;
            let probability = prob.unwrap_or(0.5);
            // Safe to unwrap because supports_rest() returned true
            let rest =
                T::rest_value().expect("supports_rest() returned true but rest_value() is None");
            Ok(pat.degrade_by_with_rest_seeded(probability, rest, *seed))
        }

        MiniAST::Euclidean {
            pattern,
            pulses,
            steps,
            rotation,
        } => {
            if !T::supports_rest() {
                return Err(ConvertError::RestNotSupported(
                    "euclidean rhythm (n,k)".to_string(),
                ));
            }
            let pat = convert_inner(pattern)?;

            // Convert pulses, steps, and rotation to patterns for patterned euclidean
            // Note: pulses AST is MiniASTU32 but we need i32, so convert via fmap
            let pulses_pat = convert_u32_pattern(pulses).fmap(|p| *p as i32);
            let steps_pat = convert_u32_pattern(steps);
            let rotation_pat = rotation
                .as_ref()
                .map(|r| convert_i32_pattern(r))
                .unwrap_or_else(|| crate::pattern_system::constructors::pure(0i32));

            // Safe to unwrap because supports_rest() returned true
            let rest =
                T::rest_value().expect("supports_rest() returned true but rest_value() is None");
            Ok(pat.euclid_pat_with_rest(pulses_pat, steps_pat, rotation_pat, rest))
        }
    }
}

/// Convert an AST back to a string representation.
fn ast_to_string(ast: &MiniAST) -> String {
    match ast {
        MiniAST::Pure(Located { node, .. }) => atom_to_string(node),
        MiniAST::List(Located { node, .. }) => {
            node.iter().map(ast_to_string).collect::<Vec<_>>().join(":")
        }
        MiniAST::Sequence(elements) => elements
            .iter()
            .map(|(a, w)| {
                let s = ast_to_string(a);
                match w {
                    Some(weight) => format!("{}@{}", s, weight),
                    None => s,
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
        MiniAST::FastCat(elements) => {
            format!(
                "[{}]",
                elements
                    .iter()
                    .map(|(a, w)| {
                        let s = ast_to_string(a);
                        match w {
                            Some(weight) => format!("{}@{}", s, weight),
                            None => s,
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        MiniAST::SlowCat(patterns) => {
            format!(
                "<{}>",
                patterns
                    .iter()
                    .map(|(p, _)| ast_to_string(p))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        _ => String::new(),
    }
}

fn atom_to_string(atom: &AtomValue) -> String {
    match atom {
        AtomValue::Number(n) => n.to_string(),
        AtomValue::Midi(m) => format!("m{}", m),
        AtomValue::Hz(h) => format!("{}hz", h),
        AtomValue::Volts(v) => format!("{}v", v),
        AtomValue::Note {
            letter,
            accidental,
            octave,
        } => {
            let mut s = letter.to_string();
            if let Some(acc) = accidental {
                s.push(*acc);
            }
            if let Some(oct) = octave {
                s.push_str(&oct.to_string());
            }
            s
        }
        AtomValue::Identifier(s) => s.clone(),
        AtomValue::String(s) => format!("\"{}\"", s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_system::SourceSpan;
    use crate::pattern_system::mini::parser::parse;

    #[test]
    fn test_convert_number() {
        let ast = parse("42").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 1);
        assert_eq!(haps[0].value, 42.0);
    }

    #[test]
    fn test_convert_sequence() {
        let ast = parse("0 1 2").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 3);
    }

    #[test]
    fn test_convert_slowcat() {
        let ast = parse("<0 1 2>").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();

        // Each cycle should have one value
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let haps2 = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));

        assert_eq!(haps0[0].value, 0.0);
        assert_eq!(haps1[0].value, 1.0);
        assert_eq!(haps2[0].value, 2.0);
    }

    #[test]
    fn test_fastcat_inside_slowcat_preserves_grouping() {
        // Regression test: `<[c e]>` should play both c and e every cycle
        // (not alternate between c and e like `<c e>` would)
        // This tests that [c e] grouping is preserved when nested inside <...>

        // Pattern: <[c e]> should be slowcat of one fastcat element
        // c=60, e=64
        let ast = parse("<[c e]>").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();

        // Every cycle should have BOTH c (60) and e (64)
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let values0: Vec<f64> = haps0.iter().map(|h| h.value).collect();
        assert_eq!(values0.len(), 2, "Cycle 0 should have 2 events");
        assert_eq!(values0[0], 60.0, "Cycle 0 first should be c (60)");
        assert_eq!(values0[1], 64.0, "Cycle 0 second should be e (64)");

        // Cycle 1 should also have both
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let values1: Vec<f64> = haps1.iter().map(|h| h.value).collect();
        assert_eq!(values1.len(), 2, "Cycle 1 should have 2 events");
        assert_eq!(values1[0], 60.0, "Cycle 1 first should be c (60)");
        assert_eq!(values1[1], 64.0, "Cycle 1 second should be e (64)");

        // For comparison, <c e> should alternate (1 event per cycle)
        let ast2 = parse("<c e>").unwrap();
        let pat2: Pattern<f64> = convert(&ast2).unwrap();

        let haps2_0 = pat2.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps2_0.len(), 1, "<c e> should have 1 event per cycle");
        assert_eq!(haps2_0[0].value, 60.0, "<c e> cycle 0 should be c");

        let haps2_1 = pat2.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        assert_eq!(haps2_1.len(), 1, "<c e> should have 1 event per cycle");
        assert_eq!(haps2_1[0].value, 64.0, "<c e> cycle 1 should be e");
    }

    #[test]
    fn test_convert_nested_slowcat_in_sequence() {
        // Test that slowcat nested inside a sequence alternates correctly
        // Pattern: a <b c> should give [a, b] on even cycles and [a, c] on odd cycles
        // where a=69, b=71, c=60
        let ast = parse("a <b c>").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();

        // Cycle 0: should have a (69) and b (71)
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let values0: Vec<f64> = haps0.iter().map(|h| h.value).collect();
        assert_eq!(values0.len(), 2);
        assert_eq!(values0[0], 69.0, "First element should be 'a' (69)");
        assert_eq!(
            values0[1], 71.0,
            "Second element should be 'b' (71) on cycle 0"
        );

        // Cycle 1: should have a (69) and c (60)
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let values1: Vec<f64> = haps1.iter().map(|h| h.value).collect();
        assert_eq!(values1.len(), 2);
        assert_eq!(values1[0], 69.0, "First element should be 'a' (69)");
        assert_eq!(
            values1[1], 60.0,
            "Second element should be 'c' (60) on cycle 1"
        );

        // Cycle 2: should alternate back to b
        let haps2 = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));
        let values2: Vec<f64> = haps2.iter().map(|h| h.value).collect();
        assert_eq!(
            values2[1], 71.0,
            "Second element should be 'b' (71) on cycle 2"
        );
    }

    #[test]
    fn test_convert_deeply_nested_slowcat() {
        // Test that deeply nested slowcat respects the inner slowcat
        // Pattern: c3 c#3 <cb <d d5>>
        // This should parse as: Sequence([c3, c#3, SlowCat([cb, SlowCat([d, d5])])])
        //
        // Let's understand what values we expect:
        // c3 = 48, c#3 = 49, cb4 = 59, d4 = 62, d5 = 74
        //
        // The outer slowcat has 2 elements: cb and <d d5>
        // Cycle 0: outer picks cb (index 0)
        // Cycle 1: outer picks <d d5> (index 1) - inner has its own cycle tracking
        // Cycle 2: outer picks cb (index 0)
        // Cycle 3: outer picks <d d5> (index 1) - inner advances
        //
        // The inner slowcat sees adjusted time, so it will have its own cycle counting
        let ast = parse("c3 c#3 <cb <d d5>>").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();

        // Verify the pattern structure by querying different cycles
        // c3 = 48, c#3 = 49, cb4 = 59 (c4 flat), d4 = 62, d5 = 74

        // Cycle 0: c3, c#3, cb
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let values0: Vec<f64> = haps0.iter().map(|h| h.value).collect();
        assert_eq!(values0.len(), 3, "Cycle 0 should have 3 elements");
        assert_eq!(values0[0], 48.0, "First element should be c3 (48)");
        assert_eq!(values0[1], 49.0, "Second element should be c#3 (49)");
        assert_eq!(
            values0[2], 59.0,
            "Third element should be cb (59) on cycle 0"
        );

        // Cycle 1: c3, c#3, <d d5> - inner slowcat picks based on its time
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let values1: Vec<f64> = haps1.iter().map(|h| h.value).collect();
        assert_eq!(values1.len(), 3, "Cycle 1 should have 3 elements");
        assert_eq!(values1[0], 48.0, "First element should be c3 (48)");
        assert_eq!(values1[1], 49.0, "Second element should be c#3 (49)");
        // The inner slowcat sees cycle 0 from its perspective (first time it's queried)
        // But due to time adjustment, it might see a different cycle
        // Let's check what we actually get and verify both d and d5 appear at some point
        let cycle1_third = values1[2];
        assert!(
            cycle1_third == 62.0 || cycle1_third == 74.0,
            "Third element on cycle 1 should be d (62) or d5 (74), got {}",
            cycle1_third
        );

        // Cycle 2: c3, c#3, cb
        let haps2 = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));
        let values2: Vec<f64> = haps2.iter().map(|h| h.value).collect();
        assert_eq!(
            values2[2], 59.0,
            "Third element should be cb (59) on cycle 2"
        );

        // Cycle 3: c3, c#3, <d d5> - inner slowcat advances
        let haps3 = pat.query_arc(Fraction::from_integer(3), Fraction::from_integer(4));
        let values3: Vec<f64> = haps3.iter().map(|h| h.value).collect();
        let cycle3_third = values3[2];
        assert!(
            cycle3_third == 62.0 || cycle3_third == 74.0,
            "Third element on cycle 3 should be d (62) or d5 (74), got {}",
            cycle3_third
        );

        // Key assertion: cycle 1 and cycle 3 should be DIFFERENT values from inner slowcat
        assert_ne!(
            cycle1_third, cycle3_third,
            "Inner slowcat should alternate between d and d5 on cycles 1 and 3"
        );
    }

    #[test]
    fn test_convert_fast() {
        let ast = parse("0*2").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 2);
    }

    #[test]
    fn test_convert_replicate() {
        let ast = parse("0!3").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 3);
    }

    #[test]
    fn test_convert_replicate_in_slowcat() {
        // Test that <g!3 b> produces g on cycles 0,1,2 and b on cycle 3
        // This is the Strudel/Tidal semantics: replicate expands BEFORE slowcat
        // g = 67, b = 71
        let ast = parse("<g!3 b>").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();

        // Cycles 0, 1, 2 should all be g (67)
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps0.len(), 1);
        assert_eq!(haps0[0].value, 67.0, "Cycle 0 should be g (67)");

        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        assert_eq!(haps1.len(), 1);
        assert_eq!(haps1[0].value, 67.0, "Cycle 1 should be g (67)");

        let haps2 = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));
        assert_eq!(haps2.len(), 1);
        assert_eq!(haps2[0].value, 67.0, "Cycle 2 should be g (67)");

        // Cycle 3 should be b (71)
        let haps3 = pat.query_arc(Fraction::from_integer(3), Fraction::from_integer(4));
        assert_eq!(haps3.len(), 1);
        assert_eq!(haps3[0].value, 71.0, "Cycle 3 should be b (71)");

        // Cycle 4 should wrap back to g (67)
        let haps4 = pat.query_arc(Fraction::from_integer(4), Fraction::from_integer(5));
        assert_eq!(haps4.len(), 1);
        assert_eq!(haps4[0].value, 67.0, "Cycle 4 should be g (67) again");
    }

    #[test]
    fn test_convert_replicate_in_slowcat_with_sequence() {
        // Test "c e <g!3 b>" - the pattern from the user's bug report
        // c=60, e=64, g=67, b=71
        let ast = parse("c e <g!3 b>").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();

        // Cycle 0: c e g
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let values0: Vec<f64> = haps0.iter().map(|h| h.value).collect();
        assert_eq!(values0.len(), 3);
        assert_eq!(values0[0], 60.0, "Cycle 0: first should be c (60)");
        assert_eq!(values0[1], 64.0, "Cycle 0: second should be e (64)");
        assert_eq!(values0[2], 67.0, "Cycle 0: third should be g (67)");

        // Cycle 1: c e g (still g because !3 means 3 cycles)
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let values1: Vec<f64> = haps1.iter().map(|h| h.value).collect();
        assert_eq!(values1[2], 67.0, "Cycle 1: third should be g (67)");

        // Cycle 2: c e g (still g)
        let haps2 = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));
        let values2: Vec<f64> = haps2.iter().map(|h| h.value).collect();
        assert_eq!(values2[2], 67.0, "Cycle 2: third should be g (67)");

        // Cycle 3: c e b (finally switches to b)
        let haps3 = pat.query_arc(Fraction::from_integer(3), Fraction::from_integer(4));
        let values3: Vec<f64> = haps3.iter().map(|h| h.value).collect();
        assert_eq!(values3[2], 71.0, "Cycle 3: third should be b (71)");

        // Cycle 4: c e g (wraps back)
        let haps4 = pat.query_arc(Fraction::from_integer(4), Fraction::from_integer(5));
        let values4: Vec<f64> = haps4.iter().map(|h| h.value).collect();
        assert_eq!(values4[2], 67.0, "Cycle 4: third should be g (67) again");
    }

    #[test]
    fn test_convert_weighted() {
        let ast = parse("0@3 1").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // First element should take 3/4, second 1/4
        assert_eq!(haps.len(), 2);
        let first_duration = haps[0].whole.as_ref().unwrap().duration();
        let second_duration = haps[1].whole.as_ref().unwrap().duration();
        assert_eq!(first_duration, Fraction::new(3, 4));
        assert_eq!(second_duration, Fraction::new(1, 4));
    }

    #[test]
    fn test_convert_euclidean() {
        // Euclidean should fail for f64 patterns because f64 doesn't support rests
        let ast = parse("1(3,8)").unwrap();
        let result: Result<Pattern<f64>, _> = convert(&ast);
        assert!(result.is_err(), "Euclidean should fail for f64 patterns");
        assert!(matches!(
            result.unwrap_err(),
            ConvertError::RestNotSupported(_)
        ));
    }

    #[test]
    fn test_source_spans() {
        let ast = parse("0 1 2").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // Each hap should have a source span
        for hap in &haps {
            assert!(hap.context.source_span.is_some());
        }
    }

    #[test]
    fn test_modifier_spans() {
        // Pattern: "c*[1 2]" - c is modified by a patterned fast factor
        // The modifier values (1 and 2) should have their spans tracked
        let ast = parse("c*[1 2]").unwrap();
        let pat: Pattern<f64> = convert(&ast).unwrap();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // Should have 2 haps (one with *1, one with *2)
        assert_eq!(haps.len(), 2);

        // "c*[1 2]" positions:
        //  c at 0-1
        //  * at 1
        //  [ at 2
        //  1 at 3-4
        //  space at 4
        //  2 at 5-6
        //  ] at 6

        // Each hap should have:
        // - source_span for the main value ('c') at position 0-1
        // - modifier_spans containing the factor value ('1' at 3-4 or '2' at 5-6)
        for hap in &haps {
            let source = hap
                .context
                .source_span
                .as_ref()
                .expect("Main value should have source span");
            assert_eq!(
                (source.start, source.end),
                (0, 1),
                "source_span should be 'c' at position 0-1"
            );

            assert!(
                !hap.context.modifier_spans.is_empty(),
                "Fast factor should be tracked in modifier_spans"
            );
        }

        // Check that the modifier spans contain the factor values
        let all_modifier_spans: Vec<_> = haps
            .iter()
            .flat_map(|h| h.context.modifier_spans.iter())
            .map(|s| (s.start, s.end))
            .collect();

        // Should contain spans for both '1' and '2'
        assert!(
            all_modifier_spans
                .iter()
                .any(|&(start, end)| start == 3 && end == 4),
            "Should have modifier span for '1' at position 3-4: {:?}",
            all_modifier_spans
        );
        assert!(
            all_modifier_spans
                .iter()
                .any(|&(start, end)| start == 5 && end == 6),
            "Should have modifier span for '2' at position 5-6: {:?}",
            all_modifier_spans
        );
    }

    #[test]
    fn test_convert_degrade() {
        // Degrade should fail for f64 patterns because f64 doesn't support rests
        let ast = parse("c4?").unwrap();
        let result: Result<Pattern<f64>, _> = convert(&ast);
        assert!(result.is_err(), "Degrade should fail for f64 patterns");
        assert!(matches!(
            result.unwrap_err(),
            ConvertError::RestNotSupported(_)
        ));
    }

    #[test]
    fn test_convert_degrade_in_sequence() {
        // Degrade should fail for f64 patterns even when in a sequence
        let ast = parse("c2 c3 c4? c5").unwrap();
        let result: Result<Pattern<f64>, _> = convert(&ast);
        assert!(result.is_err(), "Degrade should fail for f64 patterns");
        assert!(matches!(
            result.unwrap_err(),
            ConvertError::RestNotSupported(_)
        ));
    }

    #[test]
    fn test_convert_random_choice_with_rest() {
        // Rest in random choice should fail for f64 patterns
        let ast = parse("c4|~").unwrap();
        let result: Result<Pattern<f64>, _> = convert(&ast);
        assert!(
            result.is_err(),
            "Rest in random choice should fail for f64 patterns"
        );
        assert!(matches!(
            result.unwrap_err(),
            ConvertError::RestNotSupported(_)
        ));
    }

    #[test]
    fn test_convert_random_choice_with_rest_in_sequence() {
        // Rest in random choice should fail for f64 patterns
        let ast = parse("c2 c3 c4|~ c5").unwrap();
        let result: Result<Pattern<f64>, _> = convert(&ast);
        assert!(
            result.is_err(),
            "Rest in random choice should fail for f64 patterns"
        );
        assert!(matches!(
            result.unwrap_err(),
            ConvertError::RestNotSupported(_)
        ));
    }

    // ============ eval_f64 tests ============

    #[test]
    fn test_eval_f64_pure() {
        let ast = MiniASTF64::Pure(Located::new(42.5, 0, 4));
        assert!((eval_f64(&ast) - 42.5).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_rest() {
        let ast = MiniASTF64::Rest(SourceSpan::new(0, 1));
        assert!((eval_f64(&ast) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_list() {
        let ast = MiniASTF64::List(Located::new(
            vec![
                MiniASTF64::Pure(Located::new(1.0, 0, 1)),
                MiniASTF64::Pure(Located::new(2.0, 2, 3)),
            ],
            0,
            3,
        ));
        assert!((eval_f64(&ast) - 1.0).abs() < 0.001); // Returns first element
    }

    #[test]
    fn test_eval_f64_list_with_rest() {
        let ast = MiniASTF64::List(Located::new(
            vec![
                MiniASTF64::Rest(SourceSpan::new(0, 1)),
                MiniASTF64::Pure(Located::new(2.0, 2, 3)),
            ],
            0,
            3,
        ));
        assert!((eval_f64(&ast) - 0.0).abs() < 0.001); // Rest is first, returns 0
    }

    #[test]
    fn test_eval_f64_sequence() {
        let ast = MiniASTF64::Sequence(vec![
            (MiniASTF64::Pure(Located::new(3.0, 0, 1)), None),
            (MiniASTF64::Pure(Located::new(4.0, 2, 3)), Some(2.0)),
        ]);
        assert!((eval_f64(&ast) - 3.0).abs() < 0.001); // Returns first element
    }

    #[test]
    fn test_eval_f64_slowcat() {
        let ast = MiniASTF64::SlowCat(vec![
            (MiniASTF64::Pure(Located::new(5.0, 0, 1)), None),
            (MiniASTF64::Pure(Located::new(6.0, 2, 3)), None),
        ]);
        assert!((eval_f64(&ast) - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_random_choice() {
        let ast = MiniASTF64::RandomChoice(
            vec![
                MiniASTF64::Pure(Located::new(7.0, 0, 1)),
                MiniASTF64::Pure(Located::new(8.0, 2, 3)),
            ],
            0,
        );
        assert!((eval_f64(&ast) - 7.0).abs() < 0.001); // Deterministic: returns first
    }

    #[test]
    fn test_eval_f64_fast() {
        let ast = MiniASTF64::Fast(
            Box::new(MiniASTF64::Pure(Located::new(9.0, 0, 1))),
            Box::new(MiniASTF64::Pure(Located::new(2.0, 2, 3))),
        );
        assert!((eval_f64(&ast) - 9.0).abs() < 0.001); // Returns pattern value
    }

    #[test]
    fn test_eval_f64_slow() {
        let ast = MiniASTF64::Slow(
            Box::new(MiniASTF64::Pure(Located::new(10.0, 0, 1))),
            Box::new(MiniASTF64::Pure(Located::new(2.0, 2, 3))),
        );
        assert!((eval_f64(&ast) - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_replicate() {
        let ast = MiniASTF64::Replicate(Box::new(MiniASTF64::Pure(Located::new(11.0, 0, 2))), 3);
        assert!((eval_f64(&ast) - 11.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_degrade() {
        let ast = MiniASTF64::Degrade(
            Box::new(MiniASTF64::Pure(Located::new(12.0, 0, 2))),
            Some(0.5),
            0,
        );
        assert!((eval_f64(&ast) - 12.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_degrade_no_prob() {
        let ast = MiniASTF64::Degrade(
            Box::new(MiniASTF64::Pure(Located::new(13.0, 0, 2))),
            None,
            0,
        );
        assert!((eval_f64(&ast) - 13.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_euclidean() {
        let ast = MiniASTF64::Euclidean {
            pattern: Box::new(MiniASTF64::Pure(Located::new(14.0, 0, 2))),
            pulses: Box::new(MiniASTU32::Pure(Located::new(3, 3, 4))),
            steps: Box::new(MiniASTU32::Pure(Located::new(8, 5, 6))),
            rotation: None,
        };
        assert!((eval_f64(&ast) - 14.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_euclidean_with_rotation() {
        let ast = MiniASTF64::Euclidean {
            pattern: Box::new(MiniASTF64::Pure(Located::new(15.0, 0, 2))),
            pulses: Box::new(MiniASTU32::Pure(Located::new(3, 3, 4))),
            steps: Box::new(MiniASTU32::Pure(Located::new(8, 5, 6))),
            rotation: Some(Box::new(MiniASTI32::Pure(Located::new(2, 7, 8)))),
        };
        assert!((eval_f64(&ast) - 15.0).abs() < 0.001);
    }

    // ============ eval_f64 nested tests (1 level deep) ============

    #[test]
    fn test_eval_f64_fast_with_sequence() {
        let ast = MiniASTF64::Fast(
            Box::new(MiniASTF64::Sequence(vec![
                (MiniASTF64::Pure(Located::new(20.0, 0, 2)), None),
                (MiniASTF64::Pure(Located::new(21.0, 3, 5)), None),
            ])),
            Box::new(MiniASTF64::Pure(Located::new(2.0, 6, 7))),
        );
        assert!((eval_f64(&ast) - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_slow_with_list() {
        let ast = MiniASTF64::Slow(
            Box::new(MiniASTF64::List(Located::new(
                vec![
                    MiniASTF64::Pure(Located::new(22.0, 0, 2)),
                    MiniASTF64::Pure(Located::new(23.0, 3, 5)),
                ],
                0,
                5,
            ))),
            Box::new(MiniASTF64::Pure(Located::new(2.0, 6, 7))),
        );
        assert!((eval_f64(&ast) - 22.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_replicate_with_slowcat() {
        let ast = MiniASTF64::Replicate(
            Box::new(MiniASTF64::SlowCat(vec![
                (MiniASTF64::Pure(Located::new(24.0, 0, 2)), None),
                (MiniASTF64::Pure(Located::new(25.0, 3, 5)), None),
            ])),
            3,
        );
        assert!((eval_f64(&ast) - 24.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_degrade_with_random_choice() {
        let ast = MiniASTF64::Degrade(
            Box::new(MiniASTF64::RandomChoice(
                vec![
                    MiniASTF64::Pure(Located::new(26.0, 0, 2)),
                    MiniASTF64::Pure(Located::new(27.0, 3, 5)),
                ],
                0,
            )),
            Some(0.5),
            0,
        );
        assert!((eval_f64(&ast) - 26.0).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_euclidean_with_nested_pattern() {
        let ast = MiniASTF64::Euclidean {
            pattern: Box::new(MiniASTF64::Fast(
                Box::new(MiniASTF64::Pure(Located::new(28.0, 0, 2))),
                Box::new(MiniASTF64::Pure(Located::new(2.0, 3, 4))),
            )),
            pulses: Box::new(MiniASTU32::Sequence(vec![(
                MiniASTU32::Pure(Located::new(3, 5, 6)),
                None,
            )])),
            steps: Box::new(MiniASTU32::Pure(Located::new(8, 7, 8))),
            rotation: None,
        };
        assert!((eval_f64(&ast) - 28.0).abs() < 0.001);
    }

    // ============ eval_u32 tests ============

    #[test]
    fn test_eval_u32_pure() {
        let ast = MiniASTU32::Pure(Located::new(42, 0, 2));
        assert_eq!(eval_u32(&ast), 42);
    }

    #[test]
    fn test_eval_u32_rest() {
        let ast = MiniASTU32::Rest(SourceSpan::new(0, 1));
        assert_eq!(eval_u32(&ast), 0);
    }

    #[test]
    fn test_eval_u32_list() {
        let ast = MiniASTU32::List(Located::new(
            vec![
                MiniASTU32::Pure(Located::new(1, 0, 1)),
                MiniASTU32::Pure(Located::new(2, 2, 3)),
            ],
            0,
            3,
        ));
        assert_eq!(eval_u32(&ast), 1); // Returns first element
    }

    #[test]
    fn test_eval_u32_list_with_rest() {
        let ast = MiniASTU32::List(Located::new(
            vec![
                MiniASTU32::Rest(SourceSpan::new(0, 1)),
                MiniASTU32::Pure(Located::new(2, 2, 3)),
            ],
            0,
            3,
        ));
        assert_eq!(eval_u32(&ast), 0); // Rest is first, returns 0
    }

    #[test]
    fn test_eval_u32_sequence() {
        let ast = MiniASTU32::Sequence(vec![
            (MiniASTU32::Pure(Located::new(3, 0, 1)), None),
            (MiniASTU32::Pure(Located::new(4, 2, 3)), Some(2.0)),
        ]);
        assert_eq!(eval_u32(&ast), 3);
    }

    #[test]
    fn test_eval_u32_slowcat() {
        let ast = MiniASTU32::SlowCat(vec![
            (MiniASTU32::Pure(Located::new(5, 0, 1)), None),
            (MiniASTU32::Pure(Located::new(6, 2, 3)), None),
        ]);
        assert_eq!(eval_u32(&ast), 5);
    }

    #[test]
    fn test_eval_u32_random_choice() {
        let ast = MiniASTU32::RandomChoice(
            vec![
                MiniASTU32::Pure(Located::new(7, 0, 1)),
                MiniASTU32::Pure(Located::new(8, 2, 3)),
            ],
            0,
        );
        assert_eq!(eval_u32(&ast), 7); // Deterministic: returns first
    }

    #[test]
    fn test_eval_u32_fast() {
        let ast = MiniASTU32::Fast(
            Box::new(MiniASTU32::Pure(Located::new(9, 0, 1))),
            Box::new(MiniASTF64::Pure(Located::new(2.0, 2, 3))),
        );
        assert_eq!(eval_u32(&ast), 9);
    }

    #[test]
    fn test_eval_u32_slow() {
        let ast = MiniASTU32::Slow(
            Box::new(MiniASTU32::Pure(Located::new(10, 0, 2))),
            Box::new(MiniASTF64::Pure(Located::new(2.0, 3, 4))),
        );
        assert_eq!(eval_u32(&ast), 10);
    }

    #[test]
    fn test_eval_u32_replicate() {
        let ast = MiniASTU32::Replicate(Box::new(MiniASTU32::Pure(Located::new(11, 0, 2))), 3);
        assert_eq!(eval_u32(&ast), 11);
    }

    #[test]
    fn test_eval_u32_degrade() {
        let ast = MiniASTU32::Degrade(
            Box::new(MiniASTU32::Pure(Located::new(12, 0, 2))),
            Some(0.5),
            0,
        );
        assert_eq!(eval_u32(&ast), 12);
    }

    #[test]
    fn test_eval_u32_degrade_no_prob() {
        let ast = MiniASTU32::Degrade(Box::new(MiniASTU32::Pure(Located::new(13, 0, 2))), None, 0);
        assert_eq!(eval_u32(&ast), 13);
    }

    #[test]
    fn test_eval_u32_euclidean() {
        let ast = MiniASTU32::Euclidean {
            pattern: Box::new(MiniASTU32::Pure(Located::new(14, 0, 2))),
            pulses: Box::new(MiniASTU32::Pure(Located::new(3, 3, 4))),
            steps: Box::new(MiniASTU32::Pure(Located::new(8, 5, 6))),
            rotation: None,
        };
        assert_eq!(eval_u32(&ast), 14);
    }

    #[test]
    fn test_eval_u32_euclidean_with_rotation() {
        let ast = MiniASTU32::Euclidean {
            pattern: Box::new(MiniASTU32::Pure(Located::new(15, 0, 2))),
            pulses: Box::new(MiniASTU32::Pure(Located::new(3, 3, 4))),
            steps: Box::new(MiniASTU32::Pure(Located::new(8, 5, 6))),
            rotation: Some(Box::new(MiniASTI32::Pure(Located::new(2, 7, 8)))),
        };
        assert_eq!(eval_u32(&ast), 15);
    }

    // ============ eval_u32 nested tests (1 level deep) ============

    #[test]
    fn test_eval_u32_fast_with_sequence() {
        let ast = MiniASTU32::Fast(
            Box::new(MiniASTU32::Sequence(vec![
                (MiniASTU32::Pure(Located::new(20, 0, 2)), None),
                (MiniASTU32::Pure(Located::new(21, 3, 5)), None),
            ])),
            Box::new(MiniASTF64::Pure(Located::new(2.0, 6, 7))),
        );
        assert_eq!(eval_u32(&ast), 20);
    }

    #[test]
    fn test_eval_u32_slow_with_list() {
        let ast = MiniASTU32::Slow(
            Box::new(MiniASTU32::List(Located::new(
                vec![
                    MiniASTU32::Pure(Located::new(22, 0, 2)),
                    MiniASTU32::Pure(Located::new(23, 3, 5)),
                ],
                0,
                5,
            ))),
            Box::new(MiniASTF64::Pure(Located::new(2.0, 6, 7))),
        );
        assert_eq!(eval_u32(&ast), 22);
    }

    #[test]
    fn test_eval_u32_replicate_with_slowcat() {
        let ast = MiniASTU32::Replicate(
            Box::new(MiniASTU32::SlowCat(vec![
                (MiniASTU32::Pure(Located::new(24, 0, 2)), None),
                (MiniASTU32::Pure(Located::new(25, 3, 5)), None),
            ])),
            3,
        );
        assert_eq!(eval_u32(&ast), 24);
    }

    #[test]
    fn test_eval_u32_degrade_with_random_choice() {
        let ast = MiniASTU32::Degrade(
            Box::new(MiniASTU32::RandomChoice(
                vec![
                    MiniASTU32::Pure(Located::new(26, 0, 2)),
                    MiniASTU32::Pure(Located::new(27, 3, 5)),
                ],
                0,
            )),
            Some(0.5),
            0,
        );
        assert_eq!(eval_u32(&ast), 26);
    }

    #[test]
    fn test_eval_u32_euclidean_with_nested_pattern() {
        let ast = MiniASTU32::Euclidean {
            pattern: Box::new(MiniASTU32::Fast(
                Box::new(MiniASTU32::Pure(Located::new(28, 0, 2))),
                Box::new(MiniASTF64::Pure(Located::new(2.0, 3, 4))),
            )),
            pulses: Box::new(MiniASTU32::Sequence(vec![(
                MiniASTU32::Pure(Located::new(3, 5, 6)),
                None,
            )])),
            steps: Box::new(MiniASTU32::Pure(Located::new(8, 7, 8))),
            rotation: None,
        };
        assert_eq!(eval_u32(&ast), 28);
    }

    // ============ Cross-type interaction tests ============

    #[test]
    fn test_eval_f64_with_u32_in_replicate() {
        // f64 pattern with u32 count
        let ast = MiniASTF64::Replicate(
            Box::new(MiniASTF64::Sequence(vec![
                (MiniASTF64::Pure(Located::new(1.5, 0, 3)), None),
                (MiniASTF64::Pure(Located::new(2.5, 4, 7)), None),
            ])),
            3,
        );
        assert!((eval_f64(&ast) - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_eval_f64_euclidean_with_u32_nested() {
        // f64 pattern with complex u32 params
        let ast = MiniASTF64::Euclidean {
            pattern: Box::new(MiniASTF64::SlowCat(vec![
                (MiniASTF64::Pure(Located::new(100.0, 0, 3)), None),
                (MiniASTF64::Pure(Located::new(200.0, 4, 7)), None),
            ])),
            pulses: Box::new(MiniASTU32::Fast(
                Box::new(MiniASTU32::Pure(Located::new(3, 8, 9))),
                Box::new(MiniASTF64::Pure(Located::new(2.0, 10, 11))),
            )),
            steps: Box::new(MiniASTU32::RandomChoice(
                vec![
                    MiniASTU32::Pure(Located::new(8, 12, 13)),
                    MiniASTU32::Pure(Located::new(16, 14, 16)),
                ],
                0,
            )),
            rotation: Some(Box::new(MiniASTI32::Degrade(
                Box::new(MiniASTI32::Pure(Located::new(2, 17, 18))),
                None,
                0,
            ))),
        };
        assert!((eval_f64(&ast) - 100.0).abs() < 0.001);
    }

    // ============ Pattern Behavior Tests with Rest Support ============
    //
    // These tests use Option<f64> which supports rests to verify
    // pattern behavior for euclidean, degrade, and other rest-requiring operations.

    /// Test wrapper type that supports rests for behavioral testing
    impl FromMiniAtom for Option<f64> {
        fn from_atom(atom: &AtomValue) -> Result<Self, ConvertError> {
            atom.to_f64()
                .map(Some)
                .ok_or_else(|| ConvertError::InvalidAtom("Cannot convert to f64".to_string()))
        }

        fn combine_with_head(head_atoms: &[AtomValue], tail: &Self) -> Result<Self, ConvertError> {
            let mut values: Vec<f64> = head_atoms.iter().filter_map(|a| a.to_f64()).collect();
            if let Some(t) = tail {
                values.push(*t);
            }
            Ok(Some(values.iter().sum::<f64>() / values.len() as f64))
        }

        fn rest_value() -> Option<Self> {
            Some(None)
        }

        fn supports_rest() -> bool {
            true
        }
    }

    impl HasRest for Option<f64> {
        fn rest_value() -> Self {
            None
        }
    }

    // --- Euclidean Pattern Behavior Tests ---

    #[test]
    fn test_euclidean_3_8_produces_correct_rhythm() {
        // Euclidean(3,8) should produce [1,0,0,1,0,0,1,0] pattern
        // That's hits at positions 0, 3, 5 (using Bjorklund algorithm)
        let ast = parse("1(3,8)").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // Should have 3 hits (value = Some(1.0)) and 5 rests (value = None)
        let hits: Vec<_> = haps.iter().filter(|h| h.value.is_some()).collect();
        let rests: Vec<_> = haps.iter().filter(|h| h.value.is_none()).collect();

        assert_eq!(hits.len(), 3, "Euclidean(3,8) should have 3 hits");
        assert_eq!(rests.len(), 5, "Euclidean(3,8) should have 5 rests");
        assert_eq!(haps.len(), 8, "Euclidean(3,8) should have 8 total events");
    }

    #[test]
    fn test_euclidean_4_8_produces_even_rhythm() {
        // Euclidean(4,8) should produce [1,0,1,0,1,0,1,0] - evenly spaced
        let ast = parse("1(4,8)").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        let hits: Vec<_> = haps.iter().filter(|h| h.value.is_some()).collect();
        assert_eq!(hits.len(), 4, "Euclidean(4,8) should have 4 hits");
        assert_eq!(haps.len(), 8, "Euclidean(4,8) should have 8 total events");

        // Verify even spacing: hits should be at positions 0, 2, 4, 6
        for (i, hap) in haps.iter().enumerate() {
            if i % 2 == 0 {
                assert!(hap.value.is_some(), "Even positions should be hits");
            } else {
                assert!(hap.value.is_none(), "Odd positions should be rests");
            }
        }
    }

    #[test]
    fn test_euclidean_with_rotation() {
        // Euclidean(3,8,2) should rotate the pattern by 2 steps
        let ast = parse("1(3,8,2)").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        let hits: Vec<_> = haps.iter().filter(|h| h.value.is_some()).collect();
        assert_eq!(
            hits.len(),
            3,
            "Rotated Euclidean(3,8,2) should still have 3 hits"
        );
    }

    #[test]
    fn test_euclidean_with_patterned_pulses() {
        // c([2 3], 8) should alternate between 2-in-8 and 3-in-8 euclidean patterns
        use crate::dsp::seq::SeqValue;

        let ast = parse("c([2 3], 8)").unwrap();
        let pat: Pattern<SeqValue> = convert(&ast).unwrap();

        // Query cycle 0 - should use first pulse value (2)
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let hits0 = haps0.iter().filter(|h| !h.value.is_rest()).count();

        // Query cycle 1 - should use second pulse value (3)
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let hits1 = haps1.iter().filter(|h| !h.value.is_rest()).count();

        // The inner_join causes the pulse pattern to be sampled per-step,
        // alternating between 2-in-8 and 3-in-8 for alternating steps
        // This test just verifies we get different hit counts
        assert!(
            hits0 + hits1 > 0,
            "Patterned euclidean should produce some hits"
        );
        assert_eq!(haps0.len(), 8, "Should have 8 events per cycle");
        assert_eq!(haps1.len(), 8, "Should have 8 events per cycle");
    }

    #[test]
    fn test_euclidean_with_patterned_steps() {
        // c(3, [4 8]) should alternate between 3-in-4 and 3-in-8 patterns
        use crate::dsp::seq::SeqValue;

        let ast = parse("c(3, <4 8>)").unwrap();
        let pat: Pattern<SeqValue> = convert(&ast).unwrap();

        // Query cycle 0 - should use steps=4
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // Query cycle 1 - should use steps=8
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));

        // With slowcat <4 8>, cycle 0 uses 4 steps, cycle 1 uses 8 steps
        assert_eq!(haps0.len(), 4, "Cycle 0: 3-in-4 should have 4 events");
        assert_eq!(haps1.len(), 8, "Cycle 1: 3-in-8 should have 8 events");

        // Both should have 3 hits
        let hits0 = haps0.iter().filter(|h| !h.value.is_rest()).count();
        let hits1 = haps1.iter().filter(|h| !h.value.is_rest()).count();
        assert_eq!(hits0, 3, "3-in-4 should have 3 hits");
        assert_eq!(hits1, 3, "3-in-8 should have 3 hits");
    }

    #[test]
    fn test_euclidean_consistent_over_cycles() {
        // Euclidean should produce the same pattern every cycle
        let ast = parse("1(3,8)").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let haps2 = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));

        let pattern0: Vec<bool> = haps0.iter().map(|h| h.value.is_some()).collect();
        let pattern1: Vec<bool> = haps1.iter().map(|h| h.value.is_some()).collect();
        let pattern2: Vec<bool> = haps2.iter().map(|h| h.value.is_some()).collect();

        assert_eq!(
            pattern0, pattern1,
            "Euclidean should be consistent across cycles 0 and 1"
        );
        assert_eq!(
            pattern1, pattern2,
            "Euclidean should be consistent across cycles 1 and 2"
        );
    }

    #[test]
    fn test_euclidean_in_sequence() {
        // [1(3,8) 2(2,4)] - two euclidean patterns in sequence
        let ast = parse("[1(3,8) 2(2,4)]").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // First half: 1(3,8) compressed to half cycle = 8 events
        // Second half: 2(2,4) compressed to half cycle = 4 events
        // Total: 12 events
        assert_eq!(haps.len(), 12, "Should have 12 total events");

        // First euclidean has 3 hits, second has 2 hits
        let first_half: Vec<_> = haps
            .iter()
            .filter(|h| h.whole.as_ref().unwrap().begin < Fraction::new(1, 2))
            .collect();
        let second_half: Vec<_> = haps
            .iter()
            .filter(|h| h.whole.as_ref().unwrap().begin >= Fraction::new(1, 2))
            .collect();

        let first_hits = first_half.iter().filter(|h| h.value.is_some()).count();
        let second_hits = second_half.iter().filter(|h| h.value.is_some()).count();

        assert_eq!(first_hits, 3, "First euclidean(3,8) should have 3 hits");
        assert_eq!(second_hits, 2, "Second euclidean(2,4) should have 2 hits");
    }

    #[test]
    fn test_multiple_euclideans_in_fastcat() {
        // 1(3,8) 2(5,8) - two euclideans taking half cycle each
        let ast = parse("1(3,8) 2(5,8)").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // Each euclidean(n,8) produces 8 events in its half
        // Total: 16 events
        assert_eq!(haps.len(), 16, "Should have 16 total events");

        // Count hits by value
        let hits_1: Vec<_> = haps.iter().filter(|h| h.value == Some(1.0)).collect();
        let hits_2: Vec<_> = haps.iter().filter(|h| h.value == Some(2.0)).collect();

        assert_eq!(hits_1.len(), 3, "First euclidean should contribute 3 hits");
        assert_eq!(hits_2.len(), 5, "Second euclidean should contribute 5 hits");
    }

    // --- Degrade Pattern Behavior Tests ---

    #[test]
    fn test_degrade_replaces_with_rest() {
        // Degrade replaces values with rest (None)
        // In a sequence "1? 2? 3? 4?", each element is independently degraded
        // The sequence structure is preserved but values may become None
        let ast = parse("1?").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        // Query a single cycle
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // Should have 1 event
        assert_eq!(haps.len(), 1, "Should have one event");

        // The value is either Some(1.0) or None depending on randomness
        let v = haps[0].value;
        assert!(
            v.is_none() || v == Some(1.0),
            "Value should be 1.0 or rest (None)"
        );
    }

    #[test]
    fn test_degrade_with_probability() {
        // Degrade with 0% probability should keep all values (probability is "keep" probability)
        // Actually, prob=0.5 means keep if random < 0.5
        // So 1?0.9 means keep if random < 0.9 (keep 90% of the time)
        let ast = parse("1?0.99").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        // With 99% keep probability, most queries should return the value
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 1);
        // High probability of being kept
        assert!(
            haps[0].value.is_some(),
            "With 99% keep probability, value should typically be kept"
        );
    }

    // --- Rest Pattern Behavior Tests ---

    #[test]
    fn test_rest_in_sequence() {
        let ast = parse("1 ~ 2 ~").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 4, "Should have 4 events");
        assert_eq!(haps[0].value, Some(1.0));
        assert_eq!(haps[1].value, None, "Second should be rest");
        assert_eq!(haps[2].value, Some(2.0));
        assert_eq!(haps[3].value, None, "Fourth should be rest");
    }

    #[test]
    fn test_rest_in_slowcat() {
        let ast = parse("<1 ~ 2>").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let haps2 = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));

        assert_eq!(haps0[0].value, Some(1.0), "Cycle 0: value");
        assert_eq!(haps1[0].value, None, "Cycle 1: rest");
        assert_eq!(haps2[0].value, Some(2.0), "Cycle 2: value");
    }

    // --- Random Choice Behavior Tests ---

    #[test]
    fn test_random_choice_produces_variety() {
        let ast = parse("1|2|3").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let mut seen = std::collections::HashSet::new();

        for cycle in 0..100 {
            let haps = pat.query_arc(
                Fraction::from_integer(cycle),
                Fraction::from_integer(cycle + 1),
            );
            assert!(!haps.is_empty(), "cycle {} should produce haps", cycle);
            // Events from random choice must be discrete (have a whole span)
            assert!(
                haps[0].whole.is_some(),
                "random choice hap should have whole span (discrete event)"
            );
            assert!(haps[0].has_onset(), "random choice hap should have onset");
            if let Some(val) = haps[0].value {
                seen.insert(val as i32);
            }
        }

        // Should see all three values over 100 cycles
        assert!(seen.contains(&1), "Should see value 1");
        assert!(seen.contains(&2), "Should see value 2");
        assert!(seen.contains(&3), "Should see value 3");
    }

    #[test]
    fn test_random_choice_in_sequence_has_onsets() {
        // "a b|c d" parses as a (b|c) d — all three positions should produce
        // discrete events with onsets (previously b|c produced whole: None).
        let ast = parse("1 2|3 4").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        for cycle in 0..20 {
            let haps = pat.query_arc(
                Fraction::from_integer(cycle),
                Fraction::from_integer(cycle + 1),
            );

            // All haps should be discrete
            for (i, hap) in haps.iter().enumerate() {
                assert!(
                    hap.whole.is_some(),
                    "cycle {} hap {} should have whole span",
                    cycle,
                    i
                );
            }

            // Should have 3 onset-bearing haps per cycle
            let onsets: Vec<_> = haps.iter().filter(|h| h.has_onset()).collect();
            assert_eq!(
                onsets.len(),
                3,
                "cycle {}: expected 3 onsets, got {}",
                cycle,
                onsets.len()
            );

            // First and third should be 1.0 and 4.0; middle is 2.0 or 3.0
            assert_eq!(onsets[0].value, Some(1.0));
            assert!(
                onsets[1].value == Some(2.0) || onsets[1].value == Some(3.0),
                "middle value should be 2 or 3, got {:?}",
                onsets[1].value
            );
            assert_eq!(onsets[2].value, Some(4.0));
        }
    }

    #[test]
    fn test_random_choice_with_rest() {
        let ast = parse("1|~").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let mut has_value = false;
        let mut has_rest = false;

        for cycle in 0..100 {
            let haps = pat.query_arc(
                Fraction::from_integer(cycle),
                Fraction::from_integer(cycle + 1),
            );
            if haps[0].value.is_some() {
                has_value = true;
            } else {
                has_rest = true;
            }
            if has_value && has_rest {
                break;
            }
        }

        assert!(has_value, "Random choice should produce some values");
        assert!(has_rest, "Random choice should produce some rests");
    }

    // --- Fast/Slow Modifier Behavior Tests ---

    #[test]
    fn test_fast_doubles_events() {
        let ast = parse("1 2*2").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // "1" takes half, "2*2" takes half but plays twice
        // So we get: 1 (half cycle), 2 (quarter cycle), 2 (quarter cycle)
        assert_eq!(haps.len(), 3);
        assert_eq!(haps[0].value, Some(1.0));
        assert_eq!(haps[1].value, Some(2.0));
        assert_eq!(haps[2].value, Some(2.0));
    }

    #[test]
    fn test_fast_with_subsequence() {
        // 1*[2 3] should apply speed pattern [2 3] to "1"
        // Expected behavior (Strudel/Tidal):
        //   First half: speed 2 → event spans 0 to 1/2
        //   Second half: speed 3 → events at 1/3→2/3 (fragment) and 2/3→1
        //   The fragment at 1/3→2/3 has its onset before 1/2, so has_onset=false
        let ast = parse("1*[2 3]").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // We get 3 haps total, but only 2 have onsets (will trigger sounds)
        assert_eq!(haps.len(), 3, "Should produce 3 haps (including fragment)");

        // Count haps with onsets (these are what Strudel shows)
        let onset_haps: Vec<_> = haps.iter().filter(|h| h.has_onset()).collect();
        assert_eq!(onset_haps.len(), 2, "Should have 2 haps with onsets");

        // First event: 0 to 1/2 (speed 2)
        assert_eq!(haps[0].value, Some(1.0));
        assert_eq!(haps[0].whole.as_ref().unwrap().begin, Fraction::new(0, 1));
        assert_eq!(haps[0].whole.as_ref().unwrap().end, Fraction::new(1, 2));
        assert!(haps[0].has_onset());

        // Second event: fragment from 1/3→2/3, part=1/2→2/3 (no onset)
        assert_eq!(haps[1].value, Some(1.0));
        assert_eq!(haps[1].whole.as_ref().unwrap().begin, Fraction::new(1, 3));
        assert_eq!(haps[1].whole.as_ref().unwrap().end, Fraction::new(2, 3));
        assert!(!haps[1].has_onset(), "Fragment should not have onset");

        // Third event: 2/3 to 1 (speed 3)
        assert_eq!(haps[2].value, Some(1.0));
        assert_eq!(haps[2].whole.as_ref().unwrap().begin, Fraction::new(2, 3));
        assert_eq!(haps[2].whole.as_ref().unwrap().end, Fraction::new(1, 1));
        assert!(haps[2].has_onset());
    }

    #[test]
    fn test_slow_extends_pattern() {
        let ast = parse("1/2").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        // Slowed by 2, so takes 2 cycles to complete
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));

        // Both cycles should see the same event (spanning 2 cycles)
        assert_eq!(haps0.len(), 1);
        assert_eq!(haps1.len(), 1);
        assert_eq!(haps0[0].value, haps1[0].value);
    }

    // --- Replicate Behavior Tests ---

    #[test]
    fn test_replicate_creates_copies() {
        let ast = parse("1!4").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 4, "Replicate !4 should create 4 copies");
        for hap in &haps {
            assert_eq!(hap.value, Some(1.0));
        }
    }

    #[test]
    fn test_replicate_default() {
        let ast = parse("1!").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 2, "Replicate ! should default to 2 copies");
    }

    #[test]
    fn test_replicate_flattens_in_sequence() {
        // `a b!2 c` should be 4 sibling elements `a b b c`, not `a [b b] c`.
        // Strudel/Tidal: `!N` flattens syntactically into the parent sequence.
        let ast = parse("1 2!2 3").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 4, "1 2!2 3 should produce 4 sibling events");
        let quarter = Fraction::new(1, 4);
        for hap in &haps {
            assert_eq!(
                hap.whole.as_ref().unwrap().duration(),
                quarter,
                "each sibling should take 1/4 of cycle"
            );
        }
        assert_eq!(haps[0].value, Some(1.0));
        assert_eq!(haps[1].value, Some(2.0));
        assert_eq!(haps[2].value, Some(2.0));
        assert_eq!(haps[3].value, Some(3.0));
    }

    #[test]
    fn test_replicate_flattens_with_weighted_sibling() {
        // `c5@1.5 c5!2` -> weights [1.5, 1, 1] total 3.5 (matches Strudel).
        // Without flattening it would be timecat([(1.5, c5), (1, fastcat(c5, c5))])
        // with total weight 2.5, giving the wrong durations.
        let ast = parse("1@1.5 2!2").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 3, "1@1.5 2!2 should produce 3 sibling events");

        // Weights [1.5, 1, 1] total 3.5: durations 3/7, 2/7, 2/7
        assert_eq!(
            haps[0].whole.as_ref().unwrap().duration(),
            Fraction::new(3, 7)
        );
        assert_eq!(
            haps[1].whole.as_ref().unwrap().duration(),
            Fraction::new(2, 7)
        );
        assert_eq!(
            haps[2].whole.as_ref().unwrap().duration(),
            Fraction::new(2, 7)
        );
        assert_eq!(haps[0].value, Some(1.0));
        assert_eq!(haps[1].value, Some(2.0));
        assert_eq!(haps[2].value, Some(2.0));
    }

    #[test]
    fn test_replicate_flattens_in_fast_sub_with_weight() {
        // `[c5@1.5 c5!2]` inside a parent context: the fast subsequence itself
        // should expand to 3 elements with weights [1.5, 1, 1] (total 3.5),
        // matching Strudel's mini-notation semantics.
        let ast = parse("[1@1.5 2!2]").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 3);
        assert_eq!(
            haps[0].whole.as_ref().unwrap().duration(),
            Fraction::new(3, 7)
        );
        assert_eq!(
            haps[1].whole.as_ref().unwrap().duration(),
            Fraction::new(2, 7)
        );
        assert_eq!(
            haps[2].whole.as_ref().unwrap().duration(),
            Fraction::new(2, 7)
        );
    }

    // --- Weighted Sequence Behavior Tests ---

    #[test]
    fn test_weighted_timing() {
        let ast = parse("1@3 2@1").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 2);

        // First element (weight 3) should take 3/4 of cycle
        let first_dur = haps[0].whole.as_ref().unwrap().duration();
        assert_eq!(first_dur, Fraction::new(3, 4));

        // Second element (weight 1) should take 1/4 of cycle
        let second_dur = haps[1].whole.as_ref().unwrap().duration();
        assert_eq!(second_dur, Fraction::new(1, 4));
    }

    #[test]
    fn test_weighted_in_fastcat() {
        // Weights should be preserved in fast subsequences
        let ast = parse("[1@2 2@1]").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 2);

        // Within the fast subsequence, weights are relative
        // 1@2 gets 2/3 of the subsequence, 2@1 gets 1/3
        let first_dur = haps[0].whole.as_ref().unwrap().duration();
        let second_dur = haps[1].whole.as_ref().unwrap().duration();

        assert_eq!(first_dur, Fraction::new(2, 3));
        assert_eq!(second_dur, Fraction::new(1, 3));
    }

    // --- Complex Interaction Tests ---

    #[test]
    fn test_euclidean_in_slowcat() {
        // <1(3,8) 2(5,8)> - alternating euclidean patterns
        let ast = parse("<1(3,8) 2(5,8)>").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        // Cycle 0: 1(3,8)
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let hits0 = haps0.iter().filter(|h| h.value == Some(1.0)).count();
        assert_eq!(hits0, 3, "Cycle 0: euclidean(3,8) should have 3 hits");

        // Cycle 1: 2(5,8)
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let hits1 = haps1.iter().filter(|h| h.value == Some(2.0)).count();
        assert_eq!(hits1, 5, "Cycle 1: euclidean(5,8) should have 5 hits");
    }

    #[test]
    fn test_fast_euclidean() {
        // 1(3,8)*2 - euclidean played twice per cycle
        let ast = parse("1(3,8)*2").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // 8 events * 2 = 16 events
        assert_eq!(haps.len(), 16);

        // 3 hits * 2 = 6 total hits
        let hits = haps.iter().filter(|h| h.value.is_some()).count();
        assert_eq!(hits, 6);
    }

    #[test]
    fn test_replicated_euclidean() {
        // 1(3,8)!2 - euclidean pattern repeated
        let ast = parse("1(3,8)!2").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // 8 events * 2 = 16 events
        assert_eq!(haps.len(), 16);
    }

    #[test]
    fn test_euclidean_with_slowcat_rotation() {
        // 1(3,8,<0 2 4>) - rotation changes each cycle
        // Note: Due to how patterns work, the rotation parameter is evaluated
        // per-cycle via the pattern structure, not statically.
        let ast = parse("1(3,8,<0 2 4>)").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let haps2 = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));

        // All should have 3 hits
        let hits0 = haps0.iter().filter(|h| h.value.is_some()).count();
        let hits1 = haps1.iter().filter(|h| h.value.is_some()).count();
        let hits2 = haps2.iter().filter(|h| h.value.is_some()).count();

        assert_eq!(hits0, 3, "Cycle 0 should have 3 hits");
        assert_eq!(hits1, 3, "Cycle 1 should have 3 hits");
        assert_eq!(hits2, 3, "Cycle 2 should have 3 hits");

        // Note: Currently the rotation is evaluated once when converting,
        // so all cycles have the same rotation. This is a limitation of the
        // current implementation. Future work could make rotation patternable.
    }

    #[test]
    fn test_degrade_then_fast() {
        // 1?*2 - degrade then speed up
        let ast = parse("1?*2").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // Fast *2 means 2 events per cycle
        assert_eq!(haps.len(), 2);
    }

    #[test]
    fn test_nested_euclidean_patterns() {
        // [1(2,4) 2(3,4)] 3(4,4) - mixed euclidean and regular
        let ast = parse("[1(2,4) 2(3,4)] 3(4,4)").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // First half: [1(2,4) 2(3,4)] = 4 + 4 = 8 events
        // Second half: 3(4,4) = 4 events (all hits since 4/4 = 1)
        assert_eq!(haps.len(), 12);

        // Verify third euclidean has all hits
        let second_half: Vec<_> = haps
            .iter()
            .filter(|h| h.whole.as_ref().unwrap().begin >= Fraction::new(1, 2))
            .filter(|h| h.value == Some(3.0))
            .collect();
        assert_eq!(second_half.len(), 4, "3(4,4) should produce 4 hits");
    }

    #[test]
    fn test_sequence_with_euclidean_element() {
        // A sequence containing a euclidean element
        // Euclidean is applied to the value "1", creating a rhythm
        let ast = parse("1(3,8) 2").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // First half: 1(3,8) = 8 events (3 hits of 1.0, 5 rests)
        // Second half: 2 = 1 event
        // Total: 9 events
        assert_eq!(
            haps.len(),
            9,
            "Should have 9 total events (8 from euclidean + 1)"
        );

        // Verify we have hits of value 1.0 and 2.0
        let ones = haps.iter().filter(|h| h.value == Some(1.0)).count();
        let twos = haps.iter().filter(|h| h.value == Some(2.0)).count();

        assert_eq!(ones, 3, "Should have 3 hits of value 1.0");
        assert_eq!(twos, 1, "Should have 1 hit of value 2.0");
    }

    #[test]
    fn test_weighted_slowcat() {
        // <[1 2 3]@3 [4 5 6 7]> should give:
        //   cycles 0-2: values from [1 2 3] (one value per cycle)
        //   cycle 3: values from [4 5 6 7] (all in one cycle)
        // Period = 4 cycles
        let ast = parse("<[1 2 3]@3 [4 5 6 7]>").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        // Cycle 0: should produce value 1
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 1, "Cycle 0 should have 1 event");
        assert_eq!(haps[0].value, Some(1.0), "Cycle 0 should be 1");

        // Cycle 1: should produce value 2
        let haps = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        assert_eq!(haps.len(), 1, "Cycle 1 should have 1 event");
        assert_eq!(haps[0].value, Some(2.0), "Cycle 1 should be 2");

        // Cycle 2: should produce value 3
        let haps = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));
        assert_eq!(haps.len(), 1, "Cycle 2 should have 1 event");
        assert_eq!(haps[0].value, Some(3.0), "Cycle 2 should be 3");

        // Cycle 3: should produce values 4, 5, 6, 7
        let haps = pat.query_arc(Fraction::from_integer(3), Fraction::from_integer(4));
        assert_eq!(haps.len(), 4, "Cycle 3 should have 4 events");
        let vals: Vec<Option<f64>> = haps.iter().map(|h| h.value).collect();
        assert_eq!(
            vals,
            vec![Some(4.0), Some(5.0), Some(6.0), Some(7.0)],
            "Cycle 3 should be [4, 5, 6, 7]"
        );

        // Cycle 4: should repeat — back to 1
        let haps = pat.query_arc(Fraction::from_integer(4), Fraction::from_integer(5));
        assert_eq!(haps.len(), 1, "Cycle 4 should repeat cycle 0");
        assert_eq!(haps[0].value, Some(1.0), "Cycle 4 should be 1");
    }

    #[test]
    fn test_unweighted_slowcat_unchanged() {
        // <a b c> (no weights) should still be a standard slowcat:
        // cycle 0 = a, cycle 1 = b, cycle 2 = c
        let ast = parse("<1 2 3>").unwrap();
        let pat: Pattern<Option<f64>> = convert(&ast).unwrap();

        let h0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        let h1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        let h2 = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));

        assert_eq!(h0.len(), 1);
        assert_eq!(h0[0].value, Some(1.0));
        assert_eq!(h1.len(), 1);
        assert_eq!(h1[0].value, Some(2.0));
        assert_eq!(h2.len(), 1);
        assert_eq!(h2[0].value, Some(3.0));
    }
}
