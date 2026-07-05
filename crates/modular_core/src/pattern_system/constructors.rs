//! Pattern constructors for creating basic patterns.
//!
//! These are the fundamental building blocks:
//! - `pure(value)` - A single repeating value, once per cycle
//! - `silence()` - No events
//! - `signal(fn)` - A continuous signal (no discrete events)

use super::{ArenaHap, ArenaHapContext, Fraction, Pattern, SourceSpan, State};
use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;

/// Create a pattern that repeats a single value once per cycle.
///
/// # Example
/// ```ignore
/// let pat = pure(440.0);
/// let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(2));
/// // Returns 2 haps, one for each cycle
/// ```
pub fn pure<T: Clone + Send + Sync + 'static>(value: T) -> Pattern<T> {
    Pattern::new_pure(value, None)
}

/// Create a pattern that repeats a single value once per cycle, with source span tracking.
///
/// This version includes source location information for editor highlighting.
pub fn pure_with_span<T: Clone + Send + Sync + 'static>(value: T, span: SourceSpan) -> Pattern<T> {
    Pattern::new_pure(value, Some(span))
}

/// Create a pattern that produces no events.
///
/// # Example
/// ```ignore
/// let pat: Pattern<i32> = silence();
/// let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
/// assert!(haps.is_empty());
/// ```
pub fn silence<T: Clone + Send + Sync + 'static>() -> Pattern<T> {
    Pattern::new_silence(Fraction::from_integer(1))
}

/// Create a continuous signal pattern.
///
/// Unlike discrete patterns, signals have no `whole` span - they're sampled
/// continuously. The function receives the midpoint of the queried span, so
/// the sample is a pure function of that span: any consumer that queries an
/// event's own span (e.g. `app_left`) gets a draw determined by the event
/// alone, not by how the enclosing query is shaped.
///
/// # Example
/// ```ignore
/// // Sawtooth wave (0 to 1 within each cycle)
/// let saw = signal(|t| (t - &t.sam()).to_f64());
/// ```
pub fn signal<T, F>(f: F) -> Pattern<T>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(&Fraction) -> T + Send + Sync + 'static,
{
    Pattern::new_into(
        move |state: &State, _bump: &Bump, out: &mut BumpVec<'_, ArenaHap<'_, T>>| {
            out.push(ArenaHap {
                whole: None,
                part: state.span.clone(),
                value: f(&state.span.midpoint()),
                context: ArenaHapContext::empty_ref(),
            });
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pure() {
        let pat = pure(42);
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 1);
        assert_eq!(haps[0].value, 42);
        assert!(haps[0].has_onset());
    }

    #[test]
    fn test_pure_multi_cycle() {
        let pat = pure(42);
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(3));

        assert_eq!(haps.len(), 3);
        for hap in &haps {
            assert_eq!(hap.value, 42);
        }
    }

    #[test]
    fn test_pure_with_span() {
        let pat = pure_with_span(42, SourceSpan::new(0, 2));
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 1);
        assert_eq!(haps[0].context.source_span, Some(SourceSpan::new(0, 2)));
    }

    #[test]
    fn test_signal_samples_span_midpoint() {
        let pat = signal(|t| t.to_f64());
        let haps = pat.query_arc(Fraction::new(1, 4), Fraction::new(3, 4));

        assert_eq!(haps.len(), 1);
        assert_eq!(haps[0].value, 0.5);
    }

    #[test]
    fn test_silence() {
        let pat: Pattern<i32> = silence();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(10));

        assert!(haps.is_empty());
    }
}
