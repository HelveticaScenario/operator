//! Scale-degree value type shared by the `$p.s` pattern path.
//!
//! `$p.s(source, scale)` parses integer scale degrees into [`IntervalValue`]s,
//! folds chained sources through `sp_combine` with [`add_interval_values`] /
//! [`sub_interval_values`], then resolves each degree to a V/Oct voltage. The
//! `$cycle` seq runtime (`seq_value::from_sp_payload`) consumes the result.

/// Value type for scale-degree patterns: either a degree or a rest.
#[derive(Clone, Debug)]
pub enum IntervalValue {
    /// Scale degree (can be negative for downward movement)
    Degree(i32),
    /// Rest - no output, gate low
    Rest,
}

impl IntervalValue {
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
        use crate::pattern_system::mini::ast::AtomValue;
        use crate::pattern_system::mini::convert::ConvertError;
        match atom {
            AtomValue::Number(n) => {
                if !n.is_finite() || n.fract() != 0.0 {
                    return Err(ConvertError::InvalidAtom(format!(
                        "IntervalValue requires integer scale degrees, got {n}"
                    )));
                }
                Ok(IntervalValue::Degree(*n as i32))
            }
            AtomValue::Hz(_) => Err(ConvertError::InvalidAtom(
                "IntervalValue does not accept Hz atoms; $p.s interprets atoms as scale-degree integers (use $p for unquantized pitch)".into(),
            )),
            AtomValue::Note { .. } => Err(ConvertError::InvalidAtom(
                "IntervalValue does not accept note atoms; $p.s interprets atoms as scale-degree integers (use $p for unquantized pitch)".into(),
            )),
            AtomValue::Truthy => Err(ConvertError::InvalidAtom(
                "'x' is a structure marker with no pitch; it is only valid inside .struct() boolean patterns".into(),
            )),
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

/// Error returned when a pattern source string is empty or whitespace-only.
/// A rest (`~`) is a real atom and stays valid.
pub(crate) const EMPTY_PATTERN_SOURCE_ERR: &str =
    "empty pattern source: a pattern string must contain at least one atom";

/// Add two `IntervalValue`s. Rest + anything = Rest.
pub(crate) fn add_interval_values(a: &IntervalValue, b: &IntervalValue) -> IntervalValue {
    match (a.degree(), b.degree()) {
        (Some(da), Some(db)) => IntervalValue::Degree(da + db),
        _ => IntervalValue::Rest,
    }
}

/// Subtract two `IntervalValue`s. Rest - anything (or anything - Rest) = Rest.
pub(crate) fn sub_interval_values(a: &IntervalValue, b: &IntervalValue) -> IntervalValue {
    match (a.degree(), b.degree()) {
        (Some(da), Some(db)) => IntervalValue::Degree(da - db),
        _ => IntervalValue::Rest,
    }
}
