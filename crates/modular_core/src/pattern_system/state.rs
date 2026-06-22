//! Query context for pattern evaluation.
//!
//! Carries the time span being queried.

use super::TimeSpan;

/// Query context containing the time span.
#[derive(Clone, Debug)]
pub struct State {
    pub span: TimeSpan,
}

impl State {
    /// Create a new state with the given span.
    pub fn new(span: TimeSpan) -> Self {
        Self { span }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_system::Fraction;

    #[test]
    fn test_state_creation() {
        let span = TimeSpan::new(Fraction::from_integer(0), Fraction::from_integer(1));
        let state = State::new(span.clone());

        assert_eq!(state.span, span);
    }
}
