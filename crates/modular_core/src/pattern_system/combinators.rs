//! Pattern combinators for combining multiple patterns.
//!
//! These operations combine patterns in various ways:
//! - `stack` - Play patterns simultaneously
//! - `slowcat` - Concatenate patterns, one per cycle
//! - `fastcat` - Concatenate patterns within one cycle
//! - `timecat` - Concatenate patterns with explicit weights

use super::{ArenaHap, Fraction, Pattern, State};
use bumpalo::Bump;
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

/// Arrange patterns over multiple cycles (Strudel's `arrange`).
///
/// Each section is `(Some(cycles), pat)` for a finite section occupying
/// `cycles` whole cycles, or `(None, pat)` for an **infinite tail** that loops
/// forever once reached. A `None` section must be unique and last (the caller
/// validates this; sections after it can never play).
///
/// Finite arrangement mirrors Strudel bit-for-bit:
/// `timecat(sections.map(|(n,p)| (n, p.fast(n))))._slow(total)`, where
/// `total = Σ cycles`. Each section plays at its native rate, progressing
/// through its own cycles, and the whole loops with period `total`. The
/// per-section `fast(n)` is what makes a section advance through `n` of its
/// own cycles rather than repeating its cycle 0.
///
/// With an infinite tail, the finite prefix plays once over cycles
/// `[0, Σ prefix)`, then the tail section loops forever from cycle `Σ prefix`.
pub fn arrange<T: Clone + Send + Sync + 'static>(
    sections: Vec<(Option<Fraction>, Pattern<T>)>,
) -> Pattern<T> {
    if sections.is_empty() {
        return super::constructors::silence();
    }

    match sections.iter().position(|(cycles, _)| cycles.is_none()) {
        Some(inf_idx) => {
            // Split into the finite prefix and the infinite tail section.
            // Anything after the tail is unreachable; the caller rejects it,
            // so we simply drop it here.
            let mut sections = sections;
            let inf_pat = sections.remove(inf_idx).1;
            sections.truncate(inf_idx);
            let finite: Vec<(Fraction, Pattern<T>)> = sections
                .into_iter()
                .map(|(cycles, pat)| (cycles.expect("prefix sections are finite"), pat))
                .collect();
            let s_k = sum_fractions(finite.iter().map(|(c, _)| c));
            let prefix = if finite.is_empty() {
                None
            } else {
                Some(arrange_finite(finite))
            };
            arrange_with_tail(prefix, s_k, inf_pat)
        }
        None => {
            let finite: Vec<(Fraction, Pattern<T>)> = sections
                .into_iter()
                .map(|(cycles, pat)| (cycles.expect("all sections finite"), pat))
                .collect();
            arrange_finite(finite)
        }
    }
}

fn sum_fractions<'a, I: Iterator<Item = &'a Fraction>>(iter: I) -> Fraction {
    iter.fold(Fraction::from_integer(0), |acc, c| acc + c.clone())
}

/// Finite arrangement: `timecat([(n, p.fast(n)) …])._slow(total)`.
fn arrange_finite<T: Clone + Send + Sync + 'static>(
    sections: Vec<(Fraction, Pattern<T>)>,
) -> Pattern<T> {
    if sections.is_empty() {
        return super::constructors::silence();
    }
    let total = sum_fractions(sections.iter().map(|(c, _)| c));
    if total.is_zero() {
        return super::constructors::silence();
    }
    let weighted: Vec<(Fraction, Pattern<T>)> = sections
        .into_iter()
        .map(|(n, pat)| (n.clone(), Pattern::new_fast_const(pat, n)))
        .collect();
    timecat(weighted)._slow(total)
}

/// Infinite-tail arrangement: play `prefix` (a finite arrangement) over cycles
/// `[0, s_k)`, then `inf_pat` shifted to start at cycle `s_k` and loop forever.
fn arrange_with_tail<T: Clone + Send + Sync + 'static>(
    prefix: Option<Pattern<T>>,
    s_k: Fraction,
    inf_pat: Pattern<T>,
) -> Pattern<T> {
    Pattern::new_into(
        move |state: &State, bump: &Bump, out: &mut BumpVec<'_, ArenaHap<'_, T>>| {
            state.span.for_each_cycle_span(|sub| {
                // Each sub-span lies within one integer cycle; route by it. The
                // finite prefix and the infinite tail meet exactly at cycle s_k.
                let cycle = sub.begin.floor();
                if cycle < s_k {
                    if let Some(prefix) = &prefix {
                        prefix.query_into(&State::new(sub.clone()), bump, out);
                    }
                } else {
                    // Query the tail at its own frame (shifted back by s_k), then
                    // map results forward by s_k. Context passes through, so the
                    // tail section keeps its highlight offset.
                    let qspan = sub.with_time(|t| t - &s_k);
                    let mut scratch: BumpVec<'_, ArenaHap<'_, T>> = BumpVec::new_in(bump);
                    inf_pat.query_into(&State::new(qspan), bump, &mut scratch);
                    out.reserve(scratch.len());
                    for hap in scratch {
                        let new_part = hap.part.with_time(|t| t + &s_k);
                        let new_whole = hap.whole.as_ref().map(|w| w.with_time(|t| t + &s_k));
                        out.push(ArenaHap {
                            whole: new_whole,
                            part: new_part,
                            value: hap.value,
                            context: hap.context,
                        });
                    }
                }
            });
        },
    )
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
                let new_whole = hap
                    .whole
                    .as_ref()
                    .map(|w| w.with_time(|t| t / &factor_clone));
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

    /// Query one integer cycle and return the values of its onset haps.
    fn onset_values(pat: &Pattern<&'static str>, cycle: i64) -> Vec<&'static str> {
        pat.query_arc(Fraction::from_integer(cycle), Fraction::from_integer(cycle + 1))
            .into_iter()
            .filter(|h| h.has_onset())
            .map(|h| h.value)
            .collect()
    }

    #[test]
    fn test_arrange_finite_advances_sections() {
        // A plays one cycle per result cycle while in its window; B likewise.
        // Crucially, after the arrangement loops (period 6), section A resumes
        // at its cycle 4 — it advances, it does not repeat its cycle 0.
        let a = slowcat(vec![
            pure("a0"),
            pure("a1"),
            pure("a2"),
            pure("a3"),
            pure("a4"),
            pure("a5"),
        ]);
        let b = slowcat(vec![pure("b0"), pure("b1"), pure("b2"), pure("b3")]);
        let arr = arrange(vec![
            (Some(Fraction::from_integer(4)), a),
            (Some(Fraction::from_integer(2)), b),
        ]);

        assert_eq!(onset_values(&arr, 0), vec!["a0"]);
        assert_eq!(onset_values(&arr, 1), vec!["a1"]);
        assert_eq!(onset_values(&arr, 2), vec!["a2"]);
        assert_eq!(onset_values(&arr, 3), vec!["a3"]);
        assert_eq!(onset_values(&arr, 4), vec!["b0"]);
        assert_eq!(onset_values(&arr, 5), vec!["b1"]);
        // Loop boundary: A advances to its cycle 4, B to its cycle 2.
        assert_eq!(onset_values(&arr, 6), vec!["a4"]);
        assert_eq!(onset_values(&arr, 7), vec!["a5"]);
        assert_eq!(onset_values(&arr, 10), vec!["b2"]);
    }

    #[test]
    fn test_arrange_infinite_tail_loops_forever() {
        let a = slowcat(vec![pure("a0"), pure("a1")]);
        let b = slowcat(vec![
            pure("b0"),
            pure("b1"),
            pure("b2"),
            pure("b3"),
            pure("b4"),
        ]);
        let arr = arrange(vec![(Some(Fraction::from_integer(2)), a), (None, b)]);

        // Finite prefix plays once over cycles 0..2.
        assert_eq!(onset_values(&arr, 0), vec!["a0"]);
        assert_eq!(onset_values(&arr, 1), vec!["a1"]);
        // Tail starts at cycle 2 and advances through its own cycles forever.
        assert_eq!(onset_values(&arr, 2), vec!["b0"]);
        assert_eq!(onset_values(&arr, 3), vec!["b1"]);
        assert_eq!(onset_values(&arr, 6), vec!["b4"]);
        // cycle 10 → tail cycle 8 → 8 mod 5 = 3.
        assert_eq!(onset_values(&arr, 10), vec!["b3"]);
    }

    #[test]
    fn test_arrange_single_infinite_is_identity() {
        let p = slowcat(vec![pure("p0"), pure("p1")]);
        let arr = arrange(vec![(None, p.clone())]);
        for c in 0..5 {
            assert_eq!(
                onset_values(&arr, c),
                onset_values(&p, c),
                "arrange([Infinity, P]) must equal P at cycle {c}"
            );
        }
    }

    #[test]
    fn test_arrange_single_finite_section_budget_is_noop() {
        // A lone finite section reduces to `timecat([(n, p.fast(n))])._slow(n)`,
        // which equals `p` exactly: the cycle budget `n` does not gate playback,
        // so the section keeps advancing through its own cycles past cycle `n`
        // rather than resetting at the budget.
        let p = slowcat(vec![
            pure("p0"),
            pure("p1"),
            pure("p2"),
            pure("p3"),
            pure("p4"),
        ]);
        let arr = arrange(vec![(Some(Fraction::from_integer(3)), p.clone())]);
        for c in 0..5 {
            assert_eq!(
                onset_values(&arr, c),
                onset_values(&p, c),
                "lone finite section equals p (budget is a no-op) at cycle {c}"
            );
        }
    }

    #[test]
    fn test_arrange_drops_sections_after_infinite_tail() {
        // `arrange` is public and takes raw cycle counts; the DSL validator
        // rejects sections after an infinite tail upstream, but the combinator
        // also drops them defensively (they could never play). Only the finite
        // prefix and the tail are reachable here — the trailing section never is.
        let a = slowcat(vec![pure("a0"), pure("a1")]);
        let b = slowcat(vec![pure("b0"), pure("b1")]);
        let c = slowcat(vec![pure("c0"), pure("c1")]);
        let arr = arrange(vec![
            (Some(Fraction::from_integer(2)), a),
            (None, b),
            (Some(Fraction::from_integer(3)), c),
        ]);

        // Prefix a over cycles [0, 2), then b loops forever from cycle 2.
        assert_eq!(onset_values(&arr, 0), vec!["a0"]);
        assert_eq!(onset_values(&arr, 1), vec!["a1"]);
        assert_eq!(onset_values(&arr, 2), vec!["b0"]);
        assert_eq!(onset_values(&arr, 3), vec!["b1"]);
        assert_eq!(onset_values(&arr, 50), vec!["b0"]); // (50 - 2) mod 2 = 0
        // The dropped section never plays at any queried cycle.
        let played: Vec<&str> = (0..24).flat_map(|cyc| onset_values(&arr, cyc)).collect();
        assert!(
            !played.iter().any(|v| v.starts_with('c')),
            "dropped trailing section must never play, got {played:?}"
        );
    }

    #[test]
    fn test_arrange_finite_zero_sum_is_silence() {
        // A finite arrangement whose cycle counts sum to zero lowers to silence,
        // guarding the subsequent `_slow(0)`. The DSL validator rejects 0-cycle
        // sections upstream, so this is reachable only via the public combinator.
        let p = slowcat(vec![pure("p0"), pure("p1")]);
        let arr = arrange(vec![(Some(Fraction::from_integer(0)), p)]);
        for c in 0..3 {
            assert!(
                onset_values(&arr, c).is_empty(),
                "zero-sum arrange is silent at cycle {c}"
            );
        }
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
