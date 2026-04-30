//! Pattern combinators for combining multiple patterns.
//!
//! These operations combine patterns in various ways:
//! - `stack` - Play patterns simultaneously
//! - `slowcat` - Concatenate patterns, one per cycle
//! - `fastcat` - Concatenate patterns within one cycle
//! - `timecat` - Concatenate patterns with explicit weights

use super::{Fraction, Hap, Pattern, State, TimeSpan};

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

    let mut result =
        Pattern::new(move |state: &State| pats.iter().flat_map(|pat| pat.query(state)).collect());

    if let Some(s) = steps {
        result.set_steps(s);
    }

    result
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

    let n = pats.len();

    Pattern::new(move |state: &State| {
        // Split the query at cycle boundaries first
        state
            .span
            .span_cycles()
            .into_iter()
            .flat_map(|subspan| {
                // Which pattern for this cycle?
                let cycle_num = subspan.begin.sam().to_f64() as i64;
                let pat_idx = ((cycle_num % n as i64) + n as i64) as usize % n;
                let pat = &pats[pat_idx];

                // Calculate offset to adjust times
                // Each pattern should see its own cycle count: floor(global_cycle / n)
                // So we offset by: global_cycle - floor(global_cycle / n)
                let n_frac = Fraction::from_integer(n as i64);
                let offset = subspan.begin.floor() - (&subspan.begin / &n_frac).floor();

                // Query with adjusted time
                let query_span = subspan.with_time(|t| t - &offset);
                let haps = pat.query(&state.set_span(query_span));

                // Adjust result times back
                haps.into_iter()
                    .map(|hap| hap.with_span_transform(|span| span.with_time(|t| t + &offset)))
                    .collect::<Vec<_>>()
            })
            .collect()
    })
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
    let n_frac = Fraction::from_integer(n as i64);
    let steps = n_frac.clone();

    // Direct implementation: each pattern takes 1/n of each cycle
    // This avoids the time distortion issues with slowcat + fast
    let mut result = Pattern::new(move |state: &State| {
        state
            .span
            .span_cycles()
            .into_iter()
            .flat_map(|cycle_span| {
                // For each cycle, split into n equal parts
                (0..n)
                    .flat_map(|i| {
                        let i_frac = Fraction::from_integer(i as i64);
                        let cycle_start = cycle_span.begin.floor();

                        // This pattern's portion: [cycle + i/n, cycle + (i+1)/n)
                        let part_begin = &cycle_start + &i_frac / &n_frac;
                        let part_end =
                            &cycle_start + (&i_frac + Fraction::from_integer(1)) / &n_frac;
                        let part_span = TimeSpan {
                            begin: part_begin.clone(),
                            end: part_end.clone(),
                        };

                        // Intersect with the query span
                        if let Some(query_part) = cycle_span.intersection(&part_span) {
                            // Transform times so pattern sees [cycle, cycle+1)
                            // i.e., stretch by n and shift
                            let query_transformed = query_part.with_time(|t| {
                                (t - &cycle_start) * &n_frac - &i_frac + &cycle_start
                            });

                            let haps = pats[i].query(&state.set_span(query_transformed));

                            // Transform results back
                            haps.into_iter()
                                .map(|hap| {
                                    hap.with_span_transform(|span| {
                                        span.with_time(|t| {
                                            (t - &cycle_start + &i_frac) / &n_frac + &cycle_start
                                        })
                                    })
                                })
                                .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    });
    result.set_steps(steps);
    result
}

/// Alias for fastcat (Tidal/Strudel naming).
pub fn sequence<T: Clone + Send + Sync + 'static>(pats: Vec<Pattern<T>>) -> Pattern<T> {
    fastcat(pats)
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

        // Compress this pattern to fit in its time slot
        let compressed_pat = pat.compress(&start_frac, &end_frac);
        compressed.push(compressed_pat);

        begin = end;
    }

    stack(compressed).with_steps(total)
}

// ===== Helper implementations on Pattern =====

impl<T: Clone + Send + Sync + 'static> Pattern<T> {
    /// Speed up the pattern by a factor.
    ///
    /// Accepts both constant values and patterns:
    /// - `pattern.fast(2)` - constant 2x speed
    /// - `pattern.fast(Fraction::from(2))` - constant 2x speed
    /// - `pattern.fast(some_pattern)` - patterned speed factor
    pub fn fast<F: super::IntoPattern<Fraction> + 'static>(&self, factor: F) -> Pattern<T> {
        let factor_pat = factor.into_pattern();
        let pat = self.clone();

        factor_pat.inner_join(move |f| pat._fast(f.clone()))
    }

    /// Internal constant-factor fast (no pattern overhead).
    pub(crate) fn _fast(&self, factor: Fraction) -> Pattern<T> {
        if factor.is_zero() {
            return super::constructors::silence();
        }

        let query = self.query.clone();
        let factor_clone = factor.clone();

        Pattern::new(move |state: &State| {
            // Speed up queries
            let new_span = state.span.with_time(|t| t * &factor_clone);
            let haps = query(&state.set_span(new_span));

            // Slow down results
            haps.into_iter()
                .map(|hap| hap.with_span_transform(|span| span.with_time(|t| t / &factor_clone)))
                .collect()
        })
    }

    /// Slow down the pattern by a factor.
    ///
    /// Accepts both constant values and patterns:
    /// - `pattern.slow(2)` - constant half speed
    /// - `pattern.slow(Fraction::from(2))` - constant half speed
    /// - `pattern.slow(some_pattern)` - patterned slow factor
    pub fn slow<F: super::IntoPattern<Fraction> + 'static>(&self, factor: F) -> Pattern<T> {
        let factor_pat = factor.into_pattern();
        let pat = self.clone();

        factor_pat.inner_join(move |f| pat._slow(f.clone()))
    }

    /// Internal constant-factor slow (no pattern overhead).
    pub(crate) fn _slow(&self, factor: Fraction) -> Pattern<T> {
        if factor.is_zero() {
            return super::constructors::silence();
        }

        self._fast(Fraction::from_integer(1) / factor)
    }

    /// Compress a pattern to fit within a portion of each cycle.
    ///
    /// The pattern's first cycle is squeezed into the range [begin, end) of each cycle.
    pub fn compress(&self, begin: &Fraction, end: &Fraction) -> Pattern<T> {
        if begin >= end {
            return super::constructors::silence();
        }

        let duration = end - begin;
        let begin_clone = begin.clone();
        let end_clone = end.clone();
        let query = self.query.clone();

        Pattern::new(move |state: &State| {
            // For each cycle in the query
            state
                .span
                .span_cycles()
                .into_iter()
                .flat_map(|cycle_span| {
                    let cycle = cycle_span.begin.sam();

                    // Calculate the compressed span within this cycle
                    let compressed_begin = &cycle + &begin_clone;
                    let compressed_end = &cycle + &end_clone;
                    let compressed_span = TimeSpan::new(compressed_begin.clone(), compressed_end);

                    // Intersect with the query span
                    if let Some(intersect) = cycle_span.intersection(&compressed_span) {
                        // Transform to query the inner pattern
                        let inner_begin =
                            (&intersect.begin - &compressed_begin) / &duration + &cycle;
                        let inner_end = (&intersect.end - &compressed_begin) / &duration + &cycle;
                        let inner_span = TimeSpan::new(inner_begin, inner_end);

                        let haps = query(&state.set_span(inner_span));

                        // Transform results back
                        haps.into_iter()
                            .filter_map(|hap| {
                                let new_part = TimeSpan::new(
                                    (&hap.part.begin - &cycle) * &duration + &compressed_begin,
                                    (&hap.part.end - &cycle) * &duration + &compressed_begin,
                                );

                                let new_whole = hap.whole.map(|w| {
                                    TimeSpan::new(
                                        (&w.begin - &cycle) * &duration + &compressed_begin,
                                        (&w.end - &cycle) * &duration + &compressed_begin,
                                    )
                                });

                                // Only include if part intersects original query
                                if let Some(final_part) = new_part.intersection(&cycle_span) {
                                    Some(Hap::with_context(
                                        new_whole,
                                        final_part,
                                        hap.value.clone(),
                                        hap.context.clone(),
                                    ))
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    }
                })
                .collect()
        })
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
        let pat = pure(42).fast(Fraction::from_integer(2));
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        // Should get 2 events in one cycle
        assert_eq!(haps.len(), 2);
    }

    #[test]
    fn test_slow() {
        let pat = pure(42).slow(Fraction::from_integer(2));
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
