//! Pattern combinators for combining multiple patterns.
//!
//! These operations combine patterns in various ways:
//! - `stack` - Play patterns simultaneously
//! - `slowcat` - Concatenate patterns, one per cycle
//! - `fastcat` - Concatenate patterns within one cycle
//! - `timecat` - Concatenate patterns with explicit weights

use super::{ArenaHap, Fraction, Pattern, State};
use bumpalo::collections::Vec as BumpVec;

/// Play multiple patterns simultaneously.
///
/// All patterns play at the same time; queries return all their haps merged.
///
/// # Example
/// ```ignore
/// let pat = stack(vec![pure(0), pure(1)]);
/// // Both 0 and 1 play simultaneously
/// ```
pub fn stack<T: Clone + Send + Sync + 'static>(pats: Vec<Pattern<T>>) -> Pattern<T> {
    if pats.is_empty() {
        return super::constructors::silence();
    }

    // Calculate LCM of steps for proper alignment
    let steps = pats
        .iter()
        .filter_map(|p| p.steps())
        .fold(None, |acc, s| match acc {
            None => Some(s.clone()),
            Some(a) => Some(lcm(&a, s)),
        });

    Pattern::new_stack(pats, steps)
}

/// Concatenate patterns, one pattern per cycle (slowcat).
///
/// Each pattern plays for exactly one cycle, then the next pattern plays.
/// The sequence repeats after all patterns have played.
///
/// # Example
/// ```ignore
/// let pat = slowcat(vec![pure(0), pure(1), pure(2)]);
/// // Cycle 0: plays 0
/// // Cycle 1: plays 1
/// // Cycle 2: plays 2
/// // Cycle 3: plays 0 (repeats)
/// ```
pub fn slowcat<T: Clone + Send + Sync + 'static>(pats: Vec<Pattern<T>>) -> Pattern<T> {
    if pats.is_empty() {
        return super::constructors::silence();
    }
    Pattern::new_slowcat(pats)
}

/// Concatenate patterns within one cycle (fastcat/sequence).
///
/// All patterns play sequentially within a single cycle, each taking
/// equal time (1/n of the cycle).
///
/// # Example
/// ```ignore
/// let pat = fastcat(vec![pure(0), pure(1), pure(2)]);
/// // All three values play within one cycle
/// // 0 plays from 0 to 1/3
/// // 1 plays from 1/3 to 2/3
/// // 2 plays from 2/3 to 1
/// ```
pub fn fastcat<T: Clone + Send + Sync + 'static>(pats: Vec<Pattern<T>>) -> Pattern<T> {
    if pats.is_empty() {
        return super::constructors::silence();
    }

    if pats.len() == 1 {
        return pats.into_iter().next().unwrap();
    }

    let n = pats.len();
    let steps = Fraction::from_integer(n as i64);

    // Each pattern occupies 1/n of the cycle directly. Composing
    // slowcat + fast for the same effect would warp event times.
    Pattern::new_fastcat(pats, steps)
}

/// Concatenate patterns with explicit weights (timeCat).
///
/// Each pattern plays for a duration proportional to its weight.
///
/// # Example
/// ```ignore
/// let pat = timecat(vec![
///     (Fraction::from_integer(3), pure(0)),  // Takes 3/4 of cycle
///     (Fraction::from_integer(1), pure(1)),  // Takes 1/4 of cycle
/// ]);
/// ```
pub fn timecat<T: Clone + Send + Sync + 'static>(
    weighted_pats: Vec<(Fraction, Pattern<T>)>,
) -> Pattern<T> {
    if weighted_pats.is_empty() {
        return super::constructors::silence();
    }

    // Calculate total weight
    let total: Fraction = weighted_pats
        .iter()
        .map(|(w, _)| w.clone())
        .fold(Fraction::from_integer(0), |a, b| a + b);

    if total.is_zero() {
        return super::constructors::silence();
    }

    // Build compressed patterns
    let mut compressed: Vec<Pattern<T>> = Vec::new();
    let mut begin = Fraction::from_integer(0);

    for (weight, pat) in weighted_pats {
        if weight.is_zero() {
            continue;
        }

        let end = &begin + &weight;
        let start_frac = &begin / &total;
        let end_frac = &end / &total;

        if start_frac >= end_frac {
            continue;
        }
        // Compress this pattern to fit in its time slot
        compressed.push(Pattern::new_compress(pat, start_frac, end_frac));

        begin = end;
    }

    stack(compressed).with_steps(total)
}

// ===== Helper implementations on Pattern =====

impl<T: Clone + Send + Sync + 'static> Pattern<T> {
    /// Speed up the pattern by a `Pattern<Fraction>` factor.
    pub fn fast(&self, factor_pat: Pattern<Fraction>) -> Pattern<T> {
        let pat = self.clone();

        factor_pat.inner_join_into(move |f, state, bump, out| {
            if f.is_zero() {
                return;
            }
            let factor_clone = f.clone();
            let new_span = state.span.with_time(|t| t * &factor_clone);
            let mut scratch: BumpVec<'_, ArenaHap<'_, T>> = BumpVec::new_in(bump);
            pat.query_into(&State::new(new_span), bump, &mut scratch);
            out.reserve(scratch.len());
            for hap in scratch {
                let new_part = hap.part.with_time(|t| t / &factor_clone);
                let new_whole = hap.whole.as_ref().map(|w| w.with_time(|t| t / &factor_clone));
                out.push(crate::pattern_system::ArenaHap {
                    whole: new_whole,
                    part: new_part,
                    value: hap.value,
                    context: hap.context,
                });
            }
        })
    }

    /// Slow down the pattern by a `Pattern<Fraction>` factor.
    pub fn slow(&self, factor_pat: Pattern<Fraction>) -> Pattern<T> {
        let pat = self.clone();

        factor_pat.inner_join_into(move |f, state, bump, out| {
            if f.is_zero() {
                return;
            }
            let inv = Fraction::from_integer(1) / f.clone();
            let new_span = state.span.with_time(|t| t * &inv);
            let mut scratch: BumpVec<'_, ArenaHap<'_, T>> = BumpVec::new_in(bump);
            pat.query_into(&State::new(new_span), bump, &mut scratch);
            out.reserve(scratch.len());
            for hap in scratch {
                let new_part = hap.part.with_time(|t| t / &inv);
                let new_whole = hap.whole.as_ref().map(|w| w.with_time(|t| t / &inv));
                out.push(crate::pattern_system::ArenaHap {
                    whole: new_whole,
                    part: new_part,
                    value: hap.value,
                    context: hap.context,
                });
            }
        })
    }

    /// Internal constant-factor slow (no pattern overhead).
    pub(crate) fn _slow(&self, factor: Fraction) -> Pattern<T> {
        if factor.is_zero() {
            return super::constructors::silence();
        }
        Pattern::new_fast_const(self.clone(), Fraction::from_integer(1) / factor)
    }

}

/// Compute the least common multiple of two fractions.
fn lcm(a: &Fraction, b: &Fraction) -> Fraction {
    let gcd = gcd(a, b);
    if gcd.is_zero() {
        Fraction::from_integer(0)
    } else {
        (a * b).abs() / gcd
    }
}

/// Compute the greatest common divisor of two fractions.
fn gcd(a: &Fraction, b: &Fraction) -> Fraction {
    // For fractions, GCD(a/b, c/d) = GCD(ad, bc) / (bd)
    // Simplified: use Euclidean algorithm on the values
    let mut x = a.abs();
    let mut y = b.abs();

    if x.is_zero() {
        return y;
    }
    if y.is_zero() {
        return x;
    }

    // Limit iterations to prevent infinite loops
    for _ in 0..100 {
        if y.is_zero() {
            return x;
        }
        let temp = y.clone();
        // x mod y for fractions
        let div = (&x / &y).floor();
        y = &x - &(&div * &temp);
        x = temp;
    }

    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_system::constructors::pure;

    #[test]
    fn test_stack() {
        let pat = stack(vec![pure(0), pure(1)]);
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 2);
        let values: Vec<_> = haps.iter().map(|h| h.value).collect();
        assert!(values.contains(&0));
        assert!(values.contains(&1));
    }

    #[test]
    fn test_stack_empty() {
        let pat: Pattern<i32> = stack(vec![]);
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert!(haps.is_empty());
    }

    #[test]
    fn test_slowcat() {
        let pat = slowcat(vec![pure(0), pure(1), pure(2)]);

        // Each cycle should have only one value
        for i in 0..6 {
            let haps = pat.query_arc(Fraction::from_integer(i), Fraction::from_integer(i + 1));
            assert_eq!(haps.len(), 1);
            assert_eq!(haps[0].value, (i % 3) as i32);
        }
    }

    #[test]
    fn test_fastcat() {
        let pat = fastcat(vec![pure(0), pure(1), pure(2)]);
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 3);
        // Values should be in order
        assert_eq!(haps[0].value, 0);
        assert_eq!(haps[1].value, 1);
        assert_eq!(haps[2].value, 2);

        // Each should take 1/3 of the cycle
        assert_eq!(haps[0].part.duration(), Fraction::new(1, 3));
        assert_eq!(haps[1].part.duration(), Fraction::new(1, 3));
        assert_eq!(haps[2].part.duration(), Fraction::new(1, 3));
    }

    #[test]
    fn test_fast() {
        let pat = pure(42).fast(pure(Fraction::from_integer(2)));
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // Should get 2 events in one cycle
        assert_eq!(haps.len(), 2);
    }

    #[test]
    fn test_slow() {
        let pat = pure(42).slow(pure(Fraction::from_integer(2)));
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // Event should span 2 cycles, so querying 1 cycle should give 1 partial event
        assert_eq!(haps.len(), 1);
        // The whole should span 2 cycles
        assert_eq!(
            haps[0].whole.as_ref().unwrap().duration(),
            Fraction::from_integer(2)
        );
    }

    #[test]
    fn test_lcm() {
        assert_eq!(
            lcm(&Fraction::from_integer(3), &Fraction::from_integer(4)),
            Fraction::from_integer(12)
        );
        assert_eq!(
            lcm(&Fraction::from_integer(6), &Fraction::from_integer(4)),
            Fraction::from_integer(12)
        );
    }

    #[test]
    fn slowcat_alternates_non_numeric_values() {
        let first = "sig1".to_string();
        let second = "sig2".to_string();

        let pat = slowcat(vec![pure(first.clone()), pure(second.clone())]);
        // Each cycle should have only one value
        for i in 0..6 {
            let haps = pat.query_arc(Fraction::from_integer(i), Fraction::from_integer(i + 1));
            assert_eq!(haps.len(), 1);
            if i % 2 == 0 {
                assert_eq!(haps[0].value, first);
            } else {
                assert_eq!(haps[0].value, second);
            }
        }
    }
}
