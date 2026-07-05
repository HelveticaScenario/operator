//! Deterministic pseudo-random patterns.
//!
//! These patterns generate pseudo-random values that are deterministic
//! based on time. The same query at the same time always returns the
//! same value, enabling reproducible randomness in patterns.

use super::{ArenaHap, ArenaHapContext, Fraction, Pattern, State, constructors::signal};
use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Generate a deterministic hash from a time value and seed.
fn time_hash(time: &Fraction, seed: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    time.numer().hash(&mut hasher);
    time.denom().hash(&mut hasher);
    hasher.finish()
}

/// Convert a hash to a float in [0, 1).
fn hash_to_float(hash: u64) -> f64 {
    (hash as f64) / (u64::MAX as f64)
}

/// Generate a deterministic hash based on cycle number and seed.
fn cycle_hash(time: &Fraction, seed: u64) -> u64 {
    let cycle = time.sam();
    time_hash(&cycle, seed)
}

/// Continuous random signal in [0, 1) keyed by sample time and `seed`.
/// Independent seeds produce independent streams.
pub fn rand_with_offset(seed: u64) -> Pattern<f64> {
    signal(move |t| hash_to_float(time_hash(t, seed)))
}

/// Random signal that holds one value per cycle, keyed by `seed`.
/// Queries split at cycle boundaries, so a query spanning several cycles
/// yields one hap per cycle, each with an independent draw. This is what
/// lets `[a|b]*16` make 16 independent choices per outer cycle: `fast`
/// queries 16 inner cycles at once, and each must sample separately.
pub fn rand_cycle_with_offset(seed: u64) -> Pattern<f64> {
    Pattern::new_into(
        move |state: &State, _bump: &Bump, out: &mut BumpVec<'_, ArenaHap<'_, f64>>| {
            state.span.for_each_cycle_span(|subspan| {
                out.push(ArenaHap {
                    whole: None,
                    part: subspan.clone(),
                    value: hash_to_float(cycle_hash(&subspan.begin, seed)),
                    context: ArenaHapContext::empty_ref(),
                });
            });
        },
    )
}

/// Choose randomly from `values` per cycle, keyed by `seed`.
pub fn choose_with_seed<T: Clone + Send + Sync + 'static>(values: Vec<T>, seed: u64) -> Pattern<T> {
    if values.is_empty() {
        panic!("choose requires at least one value");
    }
    let len = values.len();
    rand_cycle_with_offset(seed).fmap(move |r| {
        let idx = (r * len as f64).floor() as usize;
        values[idx.min(len - 1)].clone()
    })
}

impl<T: Clone + Send + Sync + 'static> Pattern<T> {
    /// Replace events with `rest` based on probability `1 - prob`, keyed by
    /// `seed`. Preserves time slots so callers can cache by slot.
    pub fn degrade_by_with_rest_seeded(&self, prob: f64, rest: T, seed: u64) -> Pattern<T> {
        let pat = self.clone();
        let rand_pat = rand_with_offset(seed);
        pat.app_left(&rand_pat, move |val, r| {
            if *r < prob { val.clone() } else { rest.clone() }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_system::constructors::pure;

    #[test]
    fn test_rand_deterministic() {
        let pat = rand_with_offset(0);

        let haps1 = pat.query_arc(Fraction::from_integer(0), Fraction::new(1, 100));
        let haps2 = pat.query_arc(Fraction::from_integer(0), Fraction::new(1, 100));

        assert_eq!(haps1.len(), 1);
        assert_eq!(haps2.len(), 1);
        assert_eq!(haps1[0].value, haps2[0].value);
    }

    #[test]
    fn test_rand_different_times() {
        let pat = rand_with_offset(0);

        let haps1 = pat.query_arc(Fraction::from_integer(0), Fraction::new(1, 100));
        let haps2 = pat.query_arc(Fraction::new(1, 2), Fraction::new(51, 100));

        assert_ne!(haps1[0].value, haps2[0].value);
    }

    #[test]
    fn test_rand_in_range() {
        let pat = rand_with_offset(0);
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::new(1, 100));

        assert!(haps[0].value >= 0.0);
        assert!(haps[0].value < 1.0);
    }

    #[test]
    fn test_rand_cycle_splits_multi_cycle_queries() {
        let pat = rand_cycle_with_offset(0);
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(16));

        assert_eq!(haps.len(), 16, "expected one hap per cycle");
        for (i, h) in haps.iter().enumerate() {
            assert_eq!(h.part.begin, Fraction::from_integer(i as i64));
            assert_eq!(h.part.end, Fraction::from_integer(i as i64 + 1));
        }
        let distinct: std::collections::HashSet<u64> =
            haps.iter().map(|h| h.value.to_bits()).collect();
        assert!(
            distinct.len() > 1,
            "each cycle should draw independently, got a single value across 16 cycles"
        );
    }

    #[test]
    fn test_choose_independent_per_slot_under_fast() {
        // `[0|1]*16` compresses 16 choose cycles into one outer cycle, so
        // the 16 slots must pick independently rather than sharing one draw.
        use crate::pattern_system::mini::parse;

        let pat = parse::<f64>("[0|1]*16").unwrap();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 16);
        let distinct: std::collections::HashSet<u64> =
            haps.iter().map(|h| h.value.to_bits()).collect();
        assert_eq!(
            distinct.len(),
            2,
            "expected both alternatives to appear across the 16 slots"
        );
    }

    #[test]
    fn test_choose() {
        let pat = choose_with_seed(vec!["a", "b", "c"], 0);

        let mut found = std::collections::HashSet::new();
        for i in 0..20 {
            let haps = pat.query_arc(
                Fraction::from_integer(i),
                Fraction::from_integer(i) + Fraction::new(1, 100),
            );
            if !haps.is_empty() {
                found.insert(haps[0].value);
            }
        }

        assert!(found.len() > 1, "Choose should produce different values");
    }

    #[test]
    fn test_degrade_by_with_rest_seeded() {
        let pat = pure(42i32);
        let rest_value = -1i32;

        let degraded = pat.degrade_by_with_rest_seeded(0.0, rest_value, 0);
        for i in 0..10 {
            let haps = degraded.query_arc(Fraction::from_integer(i), Fraction::from_integer(i + 1));
            assert_eq!(haps.len(), 1);
            assert_eq!(haps[0].value, rest_value);
        }

        let kept = pat.degrade_by_with_rest_seeded(1.0, rest_value, 0);
        for i in 0..10 {
            let haps = kept.query_arc(Fraction::from_integer(i), Fraction::from_integer(i + 1));
            assert_eq!(haps.len(), 1);
            assert_eq!(haps[0].value, 42);
        }

        let mixed = pat.degrade_by_with_rest_seeded(0.5, rest_value, 0);
        let mut kept_count = 0;
        for i in 0..100 {
            let haps = mixed.query_arc(Fraction::from_integer(i), Fraction::from_integer(i + 1));
            assert_eq!(haps.len(), 1);
            if haps[0].value == 42 {
                kept_count += 1;
            }
        }
        assert!(kept_count > 20 && kept_count < 80);
    }

    #[test]
    fn test_degrade_group_events_decide_independently() {
        // Simulates [1 2 3 4]? — ONE degrade applied over a fastcat. Every
        // event must draw its own keep/drop from the shared random signal.
        // With independent 50/50 draws, P(all four agree) = 1/8 per cycle
        // (~50 of 400); events sharing a single draw per query would agree
        // in all 400.
        use crate::pattern_system::combinators::fastcat;

        let elements: Vec<Pattern<i32>> = (1..=4).map(pure).collect();
        let pat = fastcat(elements).degrade_by_with_rest_seeded(0.5, -1, 3);

        let mut all_same_count = 0;
        let num_cycles = 400;
        for c in 0..num_cycles {
            let haps = pat.query_arc(Fraction::from_integer(c), Fraction::from_integer(c + 1));
            assert_eq!(haps.len(), 4, "fastcat of 4 should yield 4 haps");
            let first_kept = haps[0].value != -1;
            if haps.iter().all(|h| (h.value != -1) == first_kept) {
                all_same_count += 1;
            }
        }
        assert!(
            all_same_count < 200,
            "Events under one degrade appear to share a draw: {all_same_count}/400 cycles had all-same decisions"
        );
    }

    #[test]
    fn test_degrade_decision_agrees_across_query_shapes() {
        // A degrade draw is a function of the event's own span, so querying a
        // cycle whole versus as two half-cycle spans must agree on the
        // event's keep/drop decision.
        let pat = pure(1i32).degrade_by_with_rest_seeded(0.5, -1, 7);

        for c in 0..100 {
            let whole = pat.query_arc(Fraction::from_integer(c), Fraction::from_integer(c + 1));
            assert_eq!(whole.len(), 1);

            let mid = Fraction::from_integer(c) + Fraction::new(1, 2);
            let first_half = pat.query_arc(Fraction::from_integer(c), mid.clone());
            let second_half = pat.query_arc(mid, Fraction::from_integer(c + 1));
            assert_eq!(first_half.len(), 1);
            assert_eq!(second_half.len(), 1);

            assert_eq!(
                first_half[0].value, whole[0].value,
                "cycle {c}: first-half fragment disagrees with whole-cycle query"
            );
            assert_eq!(
                second_half[0].value, whole[0].value,
                "cycle {c}: second-half fragment disagrees with whole-cycle query"
            );
        }
    }

    #[test]
    fn test_degrade_independence_in_fastcat() {
        // Simulates [0?, 1?, 2?] — three degraded elements in a fastcat.
        // Each should get an independent random stream even though fastcat
        // normalises their inner times to the same values.
        // Uses explicit seeds (as the mini-notation parser would assign).
        use crate::pattern_system::combinators::fastcat;
        use crate::pattern_system::constructors::pure;

        let elements: Vec<Pattern<i32>> = (0..3)
            .map(|i| pure(i).degrade_by_with_rest_seeded(0.5, -1, i as u64))
            .collect();
        let pat = fastcat(elements);

        // Collect keep/drop decisions across many cycles.
        // For each cycle we get 3 events (one per fastcat element).
        // If they were correlated, the 3 decisions within a cycle would
        // always be identical.
        let mut all_same_count = 0;
        let num_cycles = 200;
        for c in 0..num_cycles {
            let haps = pat.query_arc(Fraction::from_integer(c), Fraction::from_integer(c + 1));
            assert_eq!(haps.len(), 3, "fastcat of 3 should yield 3 haps");
            let decisions: Vec<bool> = haps.iter().map(|h| h.value != -1).collect();
            if decisions[0] == decisions[1] && decisions[1] == decisions[2] {
                all_same_count += 1;
            }
        }
        // With independent 50/50 decisions, P(all same) = 0.25 per cycle.
        // Over 200 cycles expect ~50.  If correlated, all_same_count = 200.
        assert!(
            all_same_count < 100,
            "Degraded elements in fastcat appear correlated: {all_same_count}/200 cycles had all-same decisions"
        );
    }

    #[test]
    fn test_choose_independence_in_fastcat() {
        // Simulates [a|b, a|b] — two random-choice elements in a fastcat.
        // Each should pick independently.
        // Uses explicit seeds (as the mini-notation parser would assign).
        use crate::pattern_system::combinators::fastcat;

        let elements: Vec<Pattern<&str>> = (0..2)
            .map(|i| choose_with_seed(vec!["a", "b"], i as u64))
            .collect();
        let pat = fastcat(elements);

        let mut combos = std::collections::HashMap::<String, usize>::new();
        let num_cycles = 400;
        for c in 0..num_cycles {
            let haps = pat.query_arc(Fraction::from_integer(c), Fraction::from_integer(c + 1));
            assert_eq!(haps.len(), 2);
            let key = format!("{}{}", haps[0].value, haps[1].value);
            *combos.entry(key).or_default() += 1;
        }
        // With independence, expect ~100 each of aa, ab, ba, bb.
        // If correlated, we'd only see aa and bb.
        assert!(
            combos.len() == 4,
            "Expected all 4 combinations (aa, ab, ba, bb), got: {:?}",
            combos
        );
        for (combo, count) in &combos {
            assert!(
                *count > 50 && *count < 200,
                "Combination {combo} has {count}/400 — expected ~100"
            );
        }
    }

    #[test]
    fn test_deterministic_seeds_from_parse() {
        // Verify that parsing the same pattern twice produces identical
        // seed assignments, and that different patterns get different seeds.
        use crate::pattern_system::mini::parse_ast;

        let ast1 = parse_ast("a? b?").unwrap();
        let ast2 = parse_ast("a? b?").unwrap();
        // Same input → identical AST (including seeds)
        assert_eq!(ast1, ast2, "Same pattern should produce identical ASTs");

        // Verify seeds are distinct within the pattern
        if let crate::pattern_system::mini::ast::MiniAST::Sequence(elements) = &ast1 {
            if let (crate::pattern_system::mini::ast::MiniAST::Degrade(_, _, seed0), _) =
                &elements[0]
            {
                if let (crate::pattern_system::mini::ast::MiniAST::Degrade(_, _, seed1), _) =
                    &elements[1]
                {
                    assert_ne!(
                        seed0, seed1,
                        "Different ? operators should get different seeds"
                    );
                }
            }
        }
    }
}
