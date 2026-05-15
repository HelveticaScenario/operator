//! Pattern constructors for creating basic patterns.
//!
//! These are the fundamental building blocks:
//! - `pure(value)` - A single repeating value, once per cycle
//! - `silence()` - No events
//! - `signal(fn)` - A continuous signal (no discrete events)

use super::{Fraction, Hap, HapContext, Pattern, SourceSpan, State};

/// Create a pattern that repeats a single value once per cycle.
///
/// # Example
/// ```ignore
/// let pat = pure(440.0);
/// let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(2));
/// // Returns 2 haps, one for each cycle
/// ```
pub fn pure<T: Clone + Send + Sync + 'static>(value: T) -> Pattern<T> {
    Pattern::new(move |state: &State| {
        state
            .span
            .span_cycles()
            .into_iter()
            .map(|subspan| {
                let whole = subspan.begin.whole_cycle();
                Hap::new(Some(whole), subspan, value.clone())
            })
            .collect()
    })
    .with_steps(Fraction::from_integer(1))
}

/// Create a pattern that repeats a single value once per cycle, with source span tracking.
///
/// This version includes source location information for editor highlighting.
pub fn pure_with_span<T: Clone + Send + Sync + 'static>(value: T, span: SourceSpan) -> Pattern<T> {
    Pattern::new(move |state: &State| {
        state
            .span
            .span_cycles()
            .into_iter()
            .map(|subspan| {
                let whole = subspan.begin.whole_cycle();
                Hap::with_context(
                    Some(whole),
                    subspan,
                    value.clone(),
                    HapContext::with_span(span.clone()),
                )
            })
            .collect()
    })
    .with_steps(Fraction::from_integer(1))
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
    Pattern::new(|_state: &State| Vec::new()).with_steps(Fraction::from_integer(1))
}

/// Create a gap (silence) with a specific step count.
///
/// This is useful for polymeter calculations.
pub fn gap<T: Clone + Send + Sync + 'static>(steps: Fraction) -> Pattern<T> {
    Pattern::new(|_state: &State| Vec::new()).with_steps(steps)
}

/// Create a continuous signal pattern.
///
/// Unlike discrete patterns, signals have no `whole` span - they're sampled
/// continuously at the query time. The function receives the time and returns
/// a value.
///
/// # Example
/// ```ignore
/// // Sawtooth wave (0 to 1 within each cycle)
/// let saw = signal(|t| t.cycle_pos().to_f64());
/// ```
pub fn signal<T, F>(f: F) -> Pattern<T>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(&Fraction) -> T + Send + Sync + 'static,
{
    Pattern::new(move |state: &State| {
        vec![Hap::new(
            None, // No whole span for continuous signals
            state.span.clone(),
            f(&state.span.begin),
        )]
    })
}

/// Create a signal pattern that also receives the controls.
///
/// Useful for signals that need access to random seeds or other control values.
pub fn signal_with_controls<T, F>(f: F) -> Pattern<T>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(&Fraction, &super::Controls) -> T + Send + Sync + 'static,
{
    Pattern::new(move |state: &State| {
        vec![Hap::new(
            None,
            state.span.clone(),
            f(&state.span.begin, &state.controls),
        )]
    })
}

/// Sawtooth wave signal (0 to 1 within each cycle).
pub fn saw() -> Pattern<f64> {
    signal(|t| t.cycle_pos().to_f64())
}

/// Inverted sawtooth wave signal (1 to 0 within each cycle).
pub fn isaw() -> Pattern<f64> {
    signal(|t| 1.0 - t.cycle_pos().to_f64())
}

/// Triangle wave signal (0 to 1 to 0 within each cycle).
pub fn tri() -> Pattern<f64> {
    signal(|t| {
        let pos = t.cycle_pos().to_f64();
        if pos < 0.5 {
            pos * 2.0
        } else {
            (1.0 - pos) * 2.0
        }
    })
}

/// Square wave signal (0 then 1 within each cycle).
pub fn square() -> Pattern<f64> {
    signal(|t| {
        if t.cycle_pos().to_f64() < 0.5 {
            0.0
        } else {
            1.0
        }
    })
}

/// Sine wave signal (0 to 1 to 0 to -1 to 0, but shifted to 0-1 range).
pub fn sine() -> Pattern<f64> {
    signal(|t| {
        let pos = t.cycle_pos().to_f64();
        (1.0 + (pos * std::f64::consts::TAU).sin()) / 2.0
    })
}

/// Cosine wave signal (shifted sine, starts at 1).
pub fn cosine() -> Pattern<f64> {
    signal(|t| {
        let pos = t.cycle_pos().to_f64();
        (1.0 + (pos * std::f64::consts::TAU).cos()) / 2.0
    })
}

/// Time signal (returns the current time as a fraction).
pub fn time() -> Pattern<Fraction> {
    signal(|t| t.clone())
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
    fn test_silence() {
        let pat: Pattern<i32> = silence();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(10));

        assert!(haps.is_empty());
    }

    #[test]
    fn test_saw() {
        let pat = saw();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 1);
        assert!(!haps[0].has_onset()); // Continuous signal
        assert_eq!(haps[0].value, 0.0); // Starts at 0
    }

    #[test]
    fn test_sine() {
        let pat = sine();

        // At t=0, sine should be 0.5 (shifted sine starts at 0.5)
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::new(1, 100));
        assert!((haps[0].value - 0.5).abs() < 0.01);

        // At t=0.25, sine should be 1.0 (peak)
        let haps = pat.query_arc(Fraction::new(1, 4), Fraction::new(26, 100));
        assert!((haps[0].value - 1.0).abs() < 0.01);
    }
}
