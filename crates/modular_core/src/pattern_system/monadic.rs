//! Monadic operations for pattern composition.
//!
//! Monadic bind (flatMap) allows patterns where the value determines
//! the next pattern. Different join strategies determine how the
//! inner pattern's timing relates to the outer pattern.

use super::{ArenaHap, ArenaHapContext, Pattern, State, TimeSpan};
use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;
use std::sync::Arc;

impl<T: Clone + Send + Sync + 'static> Pattern<T> {
    /// Generalized bind with custom whole-span combination.
    pub fn bind_whole<U, W, F>(&self, whole_fn: W, f: F) -> Pattern<U>
    where
        U: Clone + Send + Sync + 'static,
        W: Fn(Option<&TimeSpan>, Option<&TimeSpan>) -> Option<TimeSpan> + Send + Sync + 'static,
        F: Fn(&T) -> Pattern<U> + Send + Sync + 'static,
    {
        let outer = self.clone();
        let f = Arc::new(f);
        let whole_fn = Arc::new(whole_fn);

        Pattern::new_into(
            move |state: &State, bump: &Bump, out: &mut BumpVec<'_, ArenaHap<'_, U>>| {
                let mut outer_haps: BumpVec<'_, ArenaHap<'_, T>> = BumpVec::new_in(bump);
                outer.query_into(state, bump, &mut outer_haps);

                for outer_hap in &outer_haps {
                    let inner_pat = f(&outer_hap.value);
                    // Query inner pattern constrained to outer hap's part
                    let inner_state = State::new(outer_hap.part.clone());
                    let mut inner_haps: BumpVec<'_, ArenaHap<'_, U>> = BumpVec::new_in(bump);
                    inner_pat.query_into(&inner_state, bump, &mut inner_haps);

                    for inner_hap in &inner_haps {
                        if let Some(part) = outer_hap.part.intersection(&inner_hap.part) {
                            let whole =
                                whole_fn(outer_hap.whole.as_ref(), inner_hap.whole.as_ref());
                            // Merge contexts: inner is primary (its source_span is kept),
                            // outer's source_span becomes a modifier_span.
                            let context = ArenaHapContext::combine_in(
                                &inner_hap.context,
                                &outer_hap.context,
                                bump,
                            );

                            out.push(ArenaHap {
                                whole,
                                part,
                                value: inner_hap.value.clone(),
                                context,
                            });
                        }
                    }
                }
            },
        )
    }

    /// Inner join - preserves inner pattern structure.
    ///
    /// The whole span comes from the inner pattern.
    pub fn inner_join<U, F>(&self, f: F) -> Pattern<U>
    where
        U: Clone + Send + Sync + 'static,
        F: Fn(&T) -> Pattern<U> + Send + Sync + 'static,
    {
        self.bind_whole(|_outer, inner| inner.cloned(), f)
    }

    /// Inner-join variant whose closure writes haps directly into the
    /// caller's arena, with no `Pattern<U>` materialised per outer hap.
    /// Used by `fast`/`slow`/`late`/`early` and other patterned-factor
    /// transforms.
    pub fn inner_join_into<U, F>(&self, f: F) -> Pattern<U>
    where
        U: Clone + Send + Sync + 'static,
        F: for<'b> Fn(
                &T,
                &State,
                &'b Bump,
                &mut BumpVec<'b, ArenaHap<'b, U>>,
            )
            + Send
            + Sync
            + 'static,
    {
        let outer = self.clone();
        let f = std::sync::Arc::new(f);
        Pattern::new_into(
            move |state: &State, bump: &Bump, out: &mut BumpVec<'_, ArenaHap<'_, U>>| {
                let mut outer_haps: BumpVec<'_, ArenaHap<'_, T>> = BumpVec::new_in(bump);
                outer.query_into(state, bump, &mut outer_haps);

                for outer_hap in &outer_haps {
                    let inner_state = State::new(outer_hap.part.clone());
                    let mut inner_haps: BumpVec<'_, ArenaHap<'_, U>> = BumpVec::new_in(bump);
                    f(&outer_hap.value, &inner_state, bump, &mut inner_haps);

                    for inner_hap in &inner_haps {
                        if let Some(part) = outer_hap.part.intersection(&inner_hap.part) {
                            // inner_join: inner's whole wins.
                            let context = ArenaHapContext::combine_in(
                                &inner_hap.context,
                                &outer_hap.context,
                                bump,
                            );
                            out.push(ArenaHap {
                                whole: inner_hap.whole.clone(),
                                part,
                                value: inner_hap.value.clone(),
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
    use crate::pattern_system::constructors::pure;

    #[test]
    fn test_inner_join() {
        let outer = pure(5);
        let result = outer.inner_join(|n| pure(*n + 1));

        let haps = result.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 1);
        assert_eq!(haps[0].value, 6);
    }
}
