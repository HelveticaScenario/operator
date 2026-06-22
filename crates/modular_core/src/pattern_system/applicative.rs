//! Applicative functor operations for pattern combination.
//!
//! These operations combine two patterns using a function:
//! - `app_left` - structure from the left (function) pattern
//! - `app_right` - structure from the right (value) pattern
//! - `app_both` - structure from both (intersection of wholes); strudel's `mix`

use super::{ArenaHap, ArenaHapContext, Pattern, State};
use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;
use std::sync::Arc;

impl<T: Clone + Send + Sync + 'static> Pattern<T> {
    /// Combine patterns using left (inner) structure.
    ///
    /// The timing structure comes from the left pattern. For each hap in the
    /// left pattern, we query the right pattern at that time and combine.
    pub fn app_left<U, V, F>(&self, pat_val: &Pattern<U>, f: F) -> Pattern<V>
    where
        U: Clone + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
        F: Fn(&T, &U) -> V + Send + Sync + 'static,
    {
        let pat_fn = self.clone();
        let pat_val = pat_val.clone();
        let f = Arc::new(f);

        Pattern::new_into(
            move |state: &State, bump: &Bump, out: &mut BumpVec<'_, ArenaHap<'_, V>>| {
                let mut haps_fn: BumpVec<'_, ArenaHap<'_, T>> = BumpVec::new_in(bump);
                pat_fn.query_into(state, bump, &mut haps_fn);

                // Query pat_val once at the full state span. Each left hap's
                // `whole_or_part` lies within that span, so the intersect with
                // each left hap recovers the per-sub-query result for any
                // pattern whose hap layout is cycle-determined.
                let mut haps_val: BumpVec<'_, ArenaHap<'_, U>> = BumpVec::new_in(bump);
                pat_val.query_into(state, bump, &mut haps_val);

                for hap_fn in &haps_fn {
                    // Iterate haps_val in source order so the output preserves
                    // the (left, right) ordering that downstream consumers
                    // expect.
                    let lookup_span = hap_fn.whole_or_part();
                    let fn_part = &hap_fn.part;
                    let window_begin = if fn_part.begin > lookup_span.begin {
                        &fn_part.begin
                    } else {
                        &lookup_span.begin
                    };
                    let window_end = if fn_part.end < lookup_span.end {
                        &fn_part.end
                    } else {
                        &lookup_span.end
                    };

                    for hap_val in haps_val.iter() {
                        if &hap_val.part.begin >= window_end || hap_val.part.end <= *window_begin {
                            continue;
                        }
                        if let Some(part) = fn_part.intersection(&hap_val.part) {
                            let value = f(&hap_fn.value, &hap_val.value);
                            let context = ArenaHapContext::combine_in(
                                &hap_fn.context,
                                &hap_val.context,
                                bump,
                            );
                            out.push(ArenaHap {
                                whole: hap_fn.whole.clone(),
                                part,
                                value,
                                context,
                            });
                        }
                    }
                }
            },
        )
    }

    /// Combine patterns using right (outer) structure.
    ///
    /// The timing structure comes from the right pattern. For each hap in the
    /// right pattern, we query the left pattern at that time and combine.
    pub fn app_right<U, V, F>(&self, pat_val: &Pattern<U>, f: F) -> Pattern<V>
    where
        U: Clone + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
        F: Fn(&T, &U) -> V + Send + Sync + 'static,
    {
        let pat_fn = self.clone();
        let pat_val = pat_val.clone();
        let f = Arc::new(f);

        Pattern::new_into(
            move |state: &State, bump: &Bump, out: &mut BumpVec<'_, ArenaHap<'_, V>>| {
                let mut haps_val: BumpVec<'_, ArenaHap<'_, U>> = BumpVec::new_in(bump);
                pat_val.query_into(state, bump, &mut haps_val);

                // Query pat_fn once at the full state span. For
                // cycle-deterministic patterns (the mini-notation case) the
                // result matches a per-right-hap re-query.
                let mut haps_fn: BumpVec<'_, ArenaHap<'_, T>> = BumpVec::new_in(bump);
                pat_fn.query_into(state, bump, &mut haps_fn);

                for hap_val in &haps_val {
                    let lookup_span = hap_val.whole_or_part();
                    let val_part = &hap_val.part;
                    let window_begin = if val_part.begin > lookup_span.begin {
                        &val_part.begin
                    } else {
                        &lookup_span.begin
                    };
                    let window_end = if val_part.end < lookup_span.end {
                        &val_part.end
                    } else {
                        &lookup_span.end
                    };
                    for hap_fn in haps_fn.iter() {
                        if &hap_fn.part.begin >= window_end || hap_fn.part.end <= *window_begin {
                            continue;
                        }
                        if let Some(part) = hap_fn.part.intersection(&hap_val.part) {
                            let value = f(&hap_fn.value, &hap_val.value);
                            let context = ArenaHapContext::combine_in(
                                &hap_fn.context,
                                &hap_val.context,
                                bump,
                            );

                            out.push(ArenaHap {
                                whole: hap_val.whole.clone(),
                                part,
                                value,
                                context,
                            });
                        }
                    }
                }
            },
        )
    }

    /// Combine patterns preserving structure from both sides.
    ///
    /// Output haps are emitted at every intersection of left and right haps.
    /// The new `whole` is the intersection of the two wholes (drops the hap
    /// if either whole exists and they don't intersect). Models strudel's
    /// `appBoth` — the `mix` alignment mode.
    pub fn app_both<U, V, F>(&self, pat_val: &Pattern<U>, f: F) -> Pattern<V>
    where
        U: Clone + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
        F: Fn(&T, &U) -> V + Send + Sync + 'static,
    {
        let pat_fn = self.clone();
        let pat_val = pat_val.clone();
        let f = Arc::new(f);

        Pattern::new_into(
            move |state: &State, bump: &Bump, out: &mut BumpVec<'_, ArenaHap<'_, V>>| {
                let mut haps_fn: BumpVec<'_, ArenaHap<'_, T>> = BumpVec::new_in(bump);
                pat_fn.query_into(state, bump, &mut haps_fn);
                let mut haps_val: BumpVec<'_, ArenaHap<'_, U>> = BumpVec::new_in(bump);
                pat_val.query_into(state, bump, &mut haps_val);

                for hap_fn in &haps_fn {
                    for hap_val in haps_val.iter() {
                        let new_whole = match (&hap_fn.whole, &hap_val.whole) {
                            (Some(a), Some(b)) => match a.intersection(b) {
                                Some(s) => Some(s),
                                None => continue,
                            },
                            (None, w) | (w, None) => w.clone(),
                        };
                        if let Some(part) = hap_fn.part.intersection(&hap_val.part) {
                            let value = f(&hap_fn.value, &hap_val.value);
                            let context = ArenaHapContext::combine_in(
                                &hap_fn.context,
                                &hap_val.context,
                                bump,
                            );
                            out.push(ArenaHap {
                                whole: new_whole,
                                part,
                                value,
                                context,
                            });
                        }
                    }
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::pattern_system::Fraction;
    use crate::pattern_system::Pattern;
    use crate::pattern_system::combinators::fastcat;
    use crate::pattern_system::constructors::pure;

    #[test]
    fn test_app_left_structure() {
        let left: Pattern<i32> = fastcat(vec![pure(1), pure(2)]);
        let right = pure(10);

        let combined = left.app_left(&right, |a, b| a + b);
        let haps = combined.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 2);
        assert_eq!(haps[0].value, 11);
        assert_eq!(haps[1].value, 12);
    }

    #[test]
    fn test_app_right_structure() {
        let left = pure(10);
        let right: Pattern<i32> = fastcat(vec![pure(1), pure(2)]);

        let combined = left.app_right(&right, |a, b| a + b);
        let haps = combined.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 2);
        assert_eq!(haps[0].value, 11);
        assert_eq!(haps[1].value, 12);
    }
}
