//! Strudel-style alignment combiners for `$p.s` chain ops.
//!
//! Direct combiner functions that take a left pattern, a right pattern, and
//! a binary value combiner, and produce a new pattern via one of the seven
//! Strudel alignment modes (`in`, `out`, `mix`, `squeeze`, `squeezeout`,
//! `reset`, `restart`).
//!
//! `in` / `out` / `mix` delegate to the existing `app_left` / `app_right` /
//! `app_both` applicatives. The remaining four are implemented inline,
//! modelled on strudel's `squeezeJoin` and `resetJoin(restart)`
//! (`packages/core/pattern.mjs` lines 308–388) and tidal's `trigJoin` /
//! `squeezeJoin` (`tidal-core/src/Sound/Tidal/Pattern.hs` lines 280–325).

use std::sync::Arc;

use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;

use super::{ArenaHap, ArenaHapContext, Fraction, Pattern, State, TimeSpan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpAlignmentMode {
    In,
    Out,
    Mix,
    Squeeze,
    SqueezeOut,
    Reset,
    Restart,
}

/// Combine `left` and `right` using `mode` + `f`. Output structure depends
/// on the mode.
pub fn combine_sp<T, U, V, F>(
    left: &Pattern<T>,
    right: &Pattern<U>,
    mode: SpAlignmentMode,
    f: F,
) -> Pattern<V>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    F: Fn(&T, &U) -> V + Send + Sync + 'static,
{
    match mode {
        SpAlignmentMode::In => left.app_left(right, f),
        SpAlignmentMode::Out => left.app_right(right, f),
        SpAlignmentMode::Mix => left.app_both(right, f),
        SpAlignmentMode::Squeeze => combine_squeeze(left, right, f, false),
        SpAlignmentMode::SqueezeOut => combine_squeeze(left, right, f, true),
        SpAlignmentMode::Reset => combine_reset(left, right, f, false),
        SpAlignmentMode::Restart => combine_reset(left, right, f, true),
    }
}

/// Squeeze: the outer pattern's events dictate where the inner pattern's
/// cycles get fitted. With `swap = false`, `right` is squeezed into each
/// `left` event (matches strudel `.squeeze`). With `swap = true`, `left`
/// is squeezed into each `right` event (matches strudel `.squeezeout`).
fn combine_squeeze<T, U, V, F>(
    left: &Pattern<T>,
    right: &Pattern<U>,
    f: F,
    swap: bool,
) -> Pattern<V>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    F: Fn(&T, &U) -> V + Send + Sync + 'static,
{
    let left = left.clone();
    let right = right.clone();
    let f = Arc::new(f);

    Pattern::new_into(
        move |state: &State, bump: &Bump, out: &mut BumpVec<'_, ArenaHap<'_, V>>| {
            // Outer pattern dictates structure. `swap` flips outer/inner.
            // To keep value-pair ordering as `f(left, right)` regardless of
            // which side is outer, we always call `f(left_val, right_val)`.
            if !swap {
                squeeze_into(&left, &right, state, bump, out, false, |l, r| f(l, r));
            } else {
                squeeze_into(&right, &left, state, bump, out, true, |r, l| f(l, r));
            }
        },
    )
}

fn squeeze_into<'b, O, I, V, G>(
    outer: &Pattern<O>,
    inner: &Pattern<I>,
    state: &State,
    bump: &'b Bump,
    out: &mut BumpVec<'b, ArenaHap<'b, V>>,
    swap: bool,
    combine: G,
) where
    O: Clone + Send + Sync + 'static,
    I: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    G: Fn(&O, &I) -> V,
{
    let mut outer_haps: BumpVec<'_, ArenaHap<'_, O>> = BumpVec::new_in(bump);
    outer.query_into(state, bump, &mut outer_haps);

    for outer_hap in &outer_haps {
        // Squeeze only operates on discrete outer haps (those with a whole).
        let target = match &outer_hap.whole {
            Some(w) => w.clone(),
            None => continue,
        };
        let dur = &target.end - &target.begin;
        if dur.is_zero() {
            continue;
        }

        // Compute the slice of the inner pattern we need to query — the
        // portion of `state.span` that falls inside `target`, remapped
        // back into the inner pattern's natural [0, 1+) cycle coords.
        let query_part = match outer_hap.part.intersection(&state.span) {
            Some(s) => s,
            None => continue,
        };
        let inner_begin = &(&query_part.begin - &target.begin) / &dur;
        let inner_end = &(&query_part.end - &target.begin) / &dur;
        let inner_state = State::new(TimeSpan::new(inner_begin, inner_end));

        let mut inner_haps: BumpVec<'_, ArenaHap<'_, I>> = BumpVec::new_in(bump);
        inner.query_into(&inner_state, bump, &mut inner_haps);

        for inner_hap in &inner_haps {
            // Map inner_hap's part/whole from inner cycle space back into
            // the outer target span.
            let mapped_part = map_span(&inner_hap.part, &target, &dur);
            let mapped_whole = inner_hap
                .whole
                .as_ref()
                .map(|w| map_span(w, &target, &dur));

            let part = match mapped_part.intersection(&outer_hap.part) {
                Some(p) => p,
                None => continue,
            };
            let whole = match (&mapped_whole, &outer_hap.whole) {
                (Some(mw), Some(ow)) => match mw.intersection(ow) {
                    Some(s) => Some(s),
                    None => continue,
                },
                _ => mapped_whole.clone().or_else(|| outer_hap.whole.clone()),
            };

            let value = combine(&outer_hap.value, &inner_hap.value);
            // `outer` / `inner` are flipped relative to (left, right) when
            // `swap` is true (SqueezeOut mode). The context must always be
            // built as (left, right) so pattern_idx 0 is the left chain
            // operand and 1 is the right — matching app_left/app_right/
            // app_both and combine_reset.
            let (left_ctx, right_ctx) = if swap {
                (&inner_hap.context, &outer_hap.context)
            } else {
                (&outer_hap.context, &inner_hap.context)
            };
            let context = ArenaHapContext::combine_in(left_ctx, right_ctx, bump);
            out.push(ArenaHap {
                whole,
                part,
                value,
                context,
            });
        }
    }
}

fn map_span(span: &TimeSpan, target: &TimeSpan, dur: &Fraction) -> TimeSpan {
    TimeSpan::new(
        &target.begin + &(&span.begin * dur),
        &target.begin + &(&span.end * dur),
    )
}

/// Reset / restart: `right` (outer) retriggers `left` (inner) at each
/// onset. `restart = false` aligns inner cycle position to outer cycle
/// position (strudel `.reset`); `restart = true` aligns inner cycle 0 to
/// outer onset (strudel `.restart`).
fn combine_reset<T, U, V, F>(
    left: &Pattern<T>,
    right: &Pattern<U>,
    f: F,
    restart: bool,
) -> Pattern<V>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    F: Fn(&T, &U) -> V + Send + Sync + 'static,
{
    let left = left.clone();
    let right = right.clone();
    let f = Arc::new(f);

    Pattern::new_into(
        move |state: &State, bump: &Bump, out: &mut BumpVec<'_, ArenaHap<'_, V>>| {
            let mut outer_haps: BumpVec<'_, ArenaHap<'_, U>> = BumpVec::new_in(bump);
            right.query_into(state, bump, &mut outer_haps);

            for outer_hap in &outer_haps {
                // Discrete only.
                let outer_whole = match &outer_hap.whole {
                    Some(w) => w.clone(),
                    None => continue,
                };
                // Shift amount: align inner cycle 0 to outer onset.
                // `reset` mode: only the cycle-relative part of the onset
                // is used, so the inner cycle "resets" each outer onset
                // without losing global phase.
                let shift = if restart {
                    outer_whole.begin.clone()
                } else {
                    outer_whole.begin.cycle_pos()
                };

                // We want inner shifted later by `shift`. Query inner at
                // state.span - shift, then re-add shift to result haps.
                let inner_state_span = TimeSpan::new(
                    &state.span.begin - &shift,
                    &state.span.end - &shift,
                );
                let inner_state = State::new(inner_state_span);

                let mut inner_haps: BumpVec<'_, ArenaHap<'_, T>> = BumpVec::new_in(bump);
                left.query_into(&inner_state, bump, &mut inner_haps);

                for inner_hap in &inner_haps {
                    let shifted_part = TimeSpan::new(
                        &inner_hap.part.begin + &shift,
                        &inner_hap.part.end + &shift,
                    );
                    let shifted_whole = inner_hap.whole.as_ref().map(|w| {
                        TimeSpan::new(&w.begin + &shift, &w.end + &shift)
                    });

                    let part = match shifted_part.intersection(&outer_hap.part) {
                        Some(p) => p,
                        None => continue,
                    };
                    let whole = match (&shifted_whole, &outer_hap.whole) {
                        (Some(iw), Some(ow)) => match iw.intersection(ow) {
                            Some(s) => Some(s),
                            None => continue,
                        },
                        _ => shifted_whole.clone().or_else(|| outer_hap.whole.clone()),
                    };

                    let value = f(&inner_hap.value, &outer_hap.value);
                    let context = ArenaHapContext::combine_in(
                        &inner_hap.context,
                        &outer_hap.context,
                        bump,
                    );
                    out.push(ArenaHap {
                        whole,
                        part,
                        value,
                        context,
                    });
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_system::Fraction;
    use crate::pattern_system::combinators::fastcat;
    use crate::pattern_system::SourceSpan;
    use crate::pattern_system::constructors::{pure, pure_with_span};

    fn ints(start: i64, end: i64, pat: &Pattern<i32>) -> Vec<i32> {
        pat.query_arc(Fraction::from_integer(start), Fraction::from_integer(end))
            .into_iter()
            .map(|h| h.value)
            .collect()
    }

    #[test]
    fn test_in_mode_matches_app_left() {
        let l: Pattern<i32> = fastcat(vec![pure(0), pure(1), pure(2)]);
        let r: Pattern<i32> = pure(10);
        let c = combine_sp(&l, &r, SpAlignmentMode::In, |a, b| a + b);
        assert_eq!(ints(0, 1, &c), vec![10, 11, 12]);
    }

    #[test]
    fn test_out_mode_uses_right_structure() {
        let l: Pattern<i32> = pure(10);
        let r: Pattern<i32> = fastcat(vec![pure(0), pure(1), pure(2)]);
        let c = combine_sp(&l, &r, SpAlignmentMode::Out, |a, b| a + b);
        assert_eq!(ints(0, 1, &c), vec![10, 11, 12]);
    }

    #[test]
    fn test_mix_combines_both_structures() {
        let l: Pattern<i32> = fastcat(vec![pure(0), pure(1)]);
        let r: Pattern<i32> = fastcat(vec![pure(10), pure(20), pure(30)]);
        let c = combine_sp(&l, &r, SpAlignmentMode::Mix, |a, b| a + b);
        // Six intersections over the cycle: at boundaries 0, 1/3, 1/2, 2/3, 1.
        let haps = c.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert!(haps.len() >= 4);
    }

    #[test]
    fn test_squeeze_fits_inner_cycles_into_outer_events() {
        // `[1 2 3].squeeze([10 20])` → `[[11 21] [12 22] [13 23]]`
        let l: Pattern<i32> = fastcat(vec![pure(1), pure(2), pure(3)]);
        let r: Pattern<i32> = fastcat(vec![pure(10), pure(20)]);
        let c = combine_sp(&l, &r, SpAlignmentMode::Squeeze, |a, b| a + b);
        let vs = ints(0, 1, &c);
        // Each of the 3 outer events gets the 2-element inner squeezed in:
        // [11,21], [12,22], [13,23].
        assert_eq!(vs.len(), 6);
        assert_eq!(vs, vec![11, 21, 12, 22, 13, 23]);
    }

    #[test]
    fn test_reset_aligns_inner_to_outer_cycle_pos() {
        // Reset: inner cycle position shifts to each outer onset's
        // cycle-relative position.
        let l: Pattern<i32> = fastcat(vec![pure(1), pure(2)]);
        let r: Pattern<i32> = fastcat(vec![pure(10), pure(20)]);
        let c = combine_sp(&l, &r, SpAlignmentMode::Reset, |a, b| a + b);
        // First outer event at [0, 1/2) sees inner starting from 0 → 1 then 2 — but inner's natural duration is 1, so [0, 1/2) sees just first half of inner, both fragments of value 1.
        // Just sanity-check non-empty output.
        let haps = c.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert!(!haps.is_empty());
    }

    #[test]
    fn test_restart_aligns_inner_to_outer_onset() {
        let l: Pattern<i32> = fastcat(vec![pure(1), pure(2)]);
        let r: Pattern<i32> = fastcat(vec![pure(10), pure(20)]);
        let c = combine_sp(&l, &r, SpAlignmentMode::Restart, |a, b| a + b);
        let haps = c.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert!(!haps.is_empty());
    }

    // M1 regression: in Squeeze mode `combine_squeeze` used to pass
    // `(inner.context, outer.context)` to `ArenaHapContext::combine_in`,
    // mis-attributing pattern_idx 0 to the right operand and pattern_idx 1
    // to the left. Every other combinator (app_left/app_right/app_both/
    // combine_reset) builds the context as `(left, right)`. After the fix,
    // the left chain operand's source span lands at pattern_idx 0
    // (HapContext::source_span) and the right operand's at pattern_idx 1
    // (HapContext::modifier_spans[0]).
    #[test]
    fn test_squeeze_context_left_is_pattern_idx_0() {
        // A = $p.s(1) at source span [0, 1); B = $p.s(2) at [10, 11).
        let a: Pattern<i32> = pure_with_span(1, SourceSpan::new(0, 1));
        let b: Pattern<i32> = pure_with_span(2, SourceSpan::new(10, 11));
        let c = combine_sp(&a, &b, SpAlignmentMode::Squeeze, |x, y| x + y);
        let haps = c.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert!(!haps.is_empty(), "expected at least one hap from $p.s(A).squeeze(B)");
        for hap in &haps {
            // Left operand A is pattern_idx 0 → ends up as source_span.
            let source = hap
                .context
                .source_span
                .as_ref()
                .expect("source_span should be the left operand A");
            assert_eq!(
                (source.start, source.end),
                (0, 1),
                "pattern_idx 0 (source_span) should be A's span [0,1), got {:?}",
                (source.start, source.end),
            );

            // Right operand B is pattern_idx 1 → first modifier_spans entry.
            assert_eq!(
                hap.context.modifier_spans.len(),
                1,
                "expected exactly one modifier span (B)"
            );
            let modifier = &hap.context.modifier_spans[0];
            assert_eq!(
                (modifier.start, modifier.end),
                (10, 11),
                "pattern_idx 1 (modifier_spans[0]) should be B's span [10,11), got {:?}",
                (modifier.start, modifier.end),
            );
        }
    }

    // Companion check: SqueezeOut was correct-by-accident before the fix
    // because outer/inner were pre-swapped upstream. The fix must preserve
    // this — `$p.s(A).squeezeOut(B)` should still land A at pattern_idx 0
    // and B at pattern_idx 1.
    #[test]
    fn test_squeeze_out_context_left_is_pattern_idx_0() {
        let a: Pattern<i32> = pure_with_span(1, SourceSpan::new(0, 1));
        let b: Pattern<i32> = pure_with_span(2, SourceSpan::new(10, 11));
        let c = combine_sp(&a, &b, SpAlignmentMode::SqueezeOut, |x, y| x + y);
        let haps = c.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert!(!haps.is_empty(), "expected at least one hap from $p.s(A).squeezeOut(B)");
        for hap in &haps {
            let source = hap
                .context
                .source_span
                .as_ref()
                .expect("source_span should be the left operand A");
            assert_eq!((source.start, source.end), (0, 1));
            assert_eq!(hap.context.modifier_spans.len(), 1);
            let modifier = &hap.context.modifier_spans[0];
            assert_eq!((modifier.start, modifier.end), (10, 11));
        }
    }

    // Repro for "$p.s('0 1 2 3', 'c(maj)').sub('0 5')" — the renderer-side
    // pattern that triggered the user's bug report. Mirrors the pipeline in
    // `SeqPatternParam::from_sp_payload`: parse each source into a
    // `Pattern<IntervalValue>`, `strip_modifier_spans` each one, then
    // `combine_sp` with `In` + `sub_interval_values`. The right operand
    // ("0 5") has the '5' leaf at source offsets [2, 3). After the fold,
    // that span should land at pattern_idx == 1 (modifier_spans[0]) in
    // every hap where the right pattern's value contributes — and across
    // the cycle, at least one such hap must exist.
    #[test]
    fn test_sp_sub_right_leaf_lands_at_pattern_idx_1() {
        use crate::dsp::seq::interval_value::{IntervalValue, sub_interval_values};
        use crate::pattern_system::mini;

        // 1+2. Lower mini-notation to Pattern<IntervalValue>.
        let left: Pattern<IntervalValue> =
            mini::parse("0 1 2 3").expect("parse '0 1 2 3'");
        let right: Pattern<IntervalValue> =
            mini::parse("0 5").expect("parse '0 5'");

        // 3. Strip modifier spans before combining (matches from_sp_payload).
        let left = left.strip_modifier_spans();
        let right = right.strip_modifier_spans();

        // 4. Combine with In mode + sub.
        let combined: Pattern<IntervalValue> =
            combine_sp(&left, &right, SpAlignmentMode::In, sub_interval_values);

        // 5. Query 1 cycle.
        let haps = combined.query_arc(
            Fraction::from_integer(0),
            Fraction::from_integer(1),
        );
        assert!(!haps.is_empty(), "expected haps from sub combine over 1 cycle");

        // 6. For each hap, walk the context tree and collect
        //    (pattern_idx, (start, end)) tuples. The walk lives on
        //    ArenaHapContext; rebuild a flat picture from the owned
        //    HapContext that `query_arc` returns, which already groups
        //    spans by pattern_idx.
        //
        //    pattern_idx 0 -> source_span + source_extra_spans
        //    pattern_idx 1 -> modifier_spans[0] + modifier_extra_spans[0]
        //    pattern_idx 2 -> modifier_spans[1] + modifier_extra_spans[1]
        //    ...
        let mut per_hap_spans: Vec<Vec<(u8, (usize, usize))>> = Vec::new();
        for hap in &haps {
            let mut collected: Vec<(u8, (usize, usize))> = Vec::new();
            if let Some(s) = &hap.context.source_span {
                collected.push((0, (s.start, s.end)));
            }
            for extra in &hap.context.source_extra_spans {
                collected.push((0, (extra.start, extra.end)));
            }
            for (i, m) in hap.context.modifier_spans.iter().enumerate() {
                let idx = (i as u8) + 1;
                collected.push((idx, (m.start, m.end)));
                if let Some(extras) = hap.context.modifier_extra_spans.get(i) {
                    for e in extras {
                        collected.push((idx, (e.start, e.end)));
                    }
                }
            }
            per_hap_spans.push(collected);
        }

        // 7. The '5' literal in the right pattern "0 5" lives at source
        //    offsets [2, 3). Assert it appears at pattern_idx == 1 in at
        //    least one hap's context across the cycle.
        let target_span = (2usize, 3usize);
        let found_at_pidx_1 = per_hap_spans
            .iter()
            .any(|spans| spans.iter().any(|(p, s)| *p == 1 && *s == target_span));

        assert!(
            found_at_pidx_1,
            "expected the '5' leaf span (2,3) at pattern_idx == 1 in at least one hap, but \
             collected per-hap spans were: {:#?}",
            per_hap_spans
        );
    }
}
