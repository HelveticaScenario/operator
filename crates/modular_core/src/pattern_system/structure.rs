//! Structure-imposing combinators: `struct` and `beat`.
//!
//! Both take their event timing from somewhere other than the source
//! pattern — a boolean pattern for `struct`, computed cycle divisions for
//! `beat` — and sample the source pattern's values at those times.

use super::{Fraction, Pattern, constructors::pure, constructors::silence};

impl<T: Clone + Send + Sync + 'static> Pattern<T> {
    /// Strudel's `struct` (`keepif` with `out` alignment): timing comes from
    /// `bool_pat`, values are sampled from `self`. Truthy slots emit the
    /// sampled value with the boolean hap's span; falsy slots emit `rest`
    /// so the pattern always produces a hap when queried.
    pub fn struct_with_rest(&self, bool_pat: &Pattern<bool>, rest: T) -> Pattern<T> {
        self.app_right(
            bool_pat,
            move |val, is_true| {
                if *is_true { val.clone() } else { rest.clone() }
            },
        )
    }

    /// Strudel's `beat`: for each `(t, div)` pair, place the sampled source
    /// value in the beat slot `[t/div, (t+1)/div)` of every cycle, with
    /// silence elsewhere. `t` wraps modulo `div` and may be fractional. Slots
    /// not fully inside the cycle are silent: a negative `t` — unless an exact 
    /// multiple of `div`, which lands on beat 0 — and a fractional `t` within
    /// `1` of the cycle end produce no onset. A non-positive `div` yields silence
    /// (like `fast(0)`).
    pub fn beat_with(&self, t_pat: Pattern<Fraction>, div_pat: Pattern<Fraction>) -> Pattern<T> {
        let src = self.clone();
        t_pat.inner_join(move |t| {
            let src = src.clone();
            let t = t.clone();
            div_pat.clone().inner_join(move |div| {
                let zero = Fraction::from_integer(0);
                if div.is_zero() || *div < zero {
                    return silence();
                }
                // t mod div preserving t's sign: the
                // euclidean remainder lands in [0, div); a negative t with a
                // nonzero remainder shifts down into (-div, 0).
                let mut t_mod = &t - &((&t / div).floor() * div.clone());
                if t < zero && !t_mod.is_zero() {
                    t_mod = &t_mod - div;
                }
                let b = &t_mod / div;
                let e = &(&t_mod + Fraction::from_integer(1)) / div;
                if b < zero || e > Fraction::from_integer(1) {
                    return silence();
                }
                let src = src.clone();
                src.inner_join(move |x| {
                    Pattern::new_compress(pure(x.clone()), b.clone(), e.clone())
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_system::combinators::{fastcat, stack};
    use crate::pattern_system::mini::{convert, parse_ast};

    fn bool_pat(source: &str) -> Pattern<bool> {
        convert::<bool>(&parse_ast(source).unwrap()).unwrap()
    }

    fn frac(n: i64, d: i64) -> Fraction {
        Fraction::new(n, d)
    }

    #[test]
    fn struct_takes_timing_from_bool_pattern() {
        let src = fastcat(vec![pure("c"), pure("e")]);
        let pat = src.struct_with_rest(&bool_pat("x ~ x x"), "REST");
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 4);
        let expected = [
            ("c", frac(0, 1), frac(1, 4)),
            ("REST", frac(1, 4), frac(1, 2)),
            ("e", frac(1, 2), frac(3, 4)),
            ("e", frac(3, 4), frac(1, 1)),
        ];
        for (hap, (value, begin, end)) in haps.iter().zip(expected) {
            assert_eq!(hap.value, value);
            let whole = hap.whole.as_ref().unwrap();
            assert_eq!(whole.begin, begin);
            assert_eq!(whole.end, end);
        }
    }

    #[test]
    fn struct_zero_and_rest_slots_both_produce_rest() {
        let pat = pure("c").struct_with_rest(&bool_pat("1 0 ~ 1"), "REST");
        let values: Vec<_> = pat
            .query_arc(Fraction::from_integer(0), Fraction::from_integer(1))
            .into_iter()
            .map(|h| h.value)
            .collect();
        assert_eq!(values, vec!["c", "REST", "REST", "c"]);
    }

    #[test]
    fn struct_bool_pattern_alternates_per_cycle() {
        let src = fastcat(vec![pure("c"), pure("e"), pure("g")]);
        let pat = src.struct_with_rest(&bool_pat("x ~ <x ~>"), "REST");

        let cycle0: Vec<_> = pat
            .query_arc(Fraction::from_integer(0), Fraction::from_integer(1))
            .into_iter()
            .map(|h| h.value)
            .collect();
        assert_eq!(cycle0, vec!["c", "REST", "g"]);

        let cycle1: Vec<_> = pat
            .query_arc(Fraction::from_integer(1), Fraction::from_integer(2))
            .into_iter()
            .map(|h| h.value)
            .collect();
        assert_eq!(cycle1, vec!["c", "REST", "REST"]);
    }

    #[test]
    fn beat_places_onsets_at_listed_divisions() {
        let t_pat = stack(vec![
            pure(Fraction::from_integer(0)),
            pure(Fraction::from_integer(7)),
            pure(Fraction::from_integer(10)),
        ]);
        let pat = pure("c").beat_with(t_pat, pure(Fraction::from_integer(16)));
        let mut haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        haps.sort_by(|a, b| a.part.begin.cmp(&b.part.begin));

        assert_eq!(haps.len(), 3);
        let expected = [
            (frac(0, 16), frac(1, 16)),
            (frac(7, 16), frac(8, 16)),
            (frac(10, 16), frac(11, 16)),
        ];
        for (hap, (begin, end)) in haps.iter().zip(expected) {
            assert_eq!(hap.value, "c");
            assert!(hap.has_onset());
            let whole = hap.whole.as_ref().unwrap();
            assert_eq!(whole.begin, begin);
            assert_eq!(whole.end, end);
            assert_eq!(hap.part.begin, begin);
            assert_eq!(hap.part.end, end);
        }
    }

    #[test]
    fn beat_wraps_t_modulo_div() {
        let pat = pure("c").beat_with(
            pure(Fraction::from_integer(17)),
            pure(Fraction::from_integer(16)),
        );
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 1);
        let whole = haps[0].whole.as_ref().unwrap();
        assert_eq!(whole.begin, frac(1, 16));
        assert_eq!(whole.end, frac(2, 16));
    }

    #[test]
    fn beat_accepts_fractional_t() {
        let pat = pure("c").beat_with(pure(frac(1, 2)), pure(Fraction::from_integer(4)));
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 1);
        let whole = haps[0].whole.as_ref().unwrap();
        assert_eq!(whole.begin, frac(1, 8));
        assert_eq!(whole.end, frac(3, 8));
    }

    #[test]
    fn beat_non_positive_div_is_silence() {
        for div in [Fraction::from_integer(0), Fraction::from_integer(-4)] {
            let pat = pure("c").beat_with(pure(Fraction::from_integer(0)), pure(div));
            let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
            assert!(haps.is_empty());
        }
    }

    #[test]
    fn beat_negative_t_is_silence_unless_multiple_of_div() {
        // Strudel's sign-preserving mod puts a negative t's slot before the
        // cycle start, and the _compress guard silences it.
        for t in [-1i64, -7, -17] {
            let pat = pure("c").beat_with(
                pure(Fraction::from_integer(t)),
                pure(Fraction::from_integer(16)),
            );
            let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
            assert!(haps.is_empty(), "beat({t}, 16) should be silent");
        }
        // An exact negative multiple of div has remainder 0 → beat 0 fires.
        let pat = pure("c").beat_with(
            pure(Fraction::from_integer(-16)),
            pure(Fraction::from_integer(16)),
        );
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 1);
        let whole = haps[0].whole.as_ref().unwrap();
        assert_eq!(whole.begin, frac(0, 16));
        assert_eq!(whole.end, frac(1, 16));
    }

    #[test]
    fn beat_fractional_slot_past_cycle_end_is_silence() {
        // t = 15.5, div = 16: the slot would end at 16.5/16 > 1, so the
        // _compress guard silences it rather than clipping.
        let pat = pure("c").beat_with(pure(frac(31, 2)), pure(Fraction::from_integer(16)));
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert!(haps.is_empty());
        // A fractional slot fully inside the cycle still fires.
        let pat = pure("c").beat_with(pure(frac(29, 2)), pure(Fraction::from_integer(16)));
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps.len(), 1);
        assert_eq!(haps[0].whole.as_ref().unwrap().begin, frac(29, 32));
    }

    #[test]
    fn beat_patterned_div_changes_grid_per_cycle() {
        let div_pat = convert::<f64>(&parse_ast("<16 8>").unwrap())
            .unwrap()
            .fmap(|v| Fraction::from(*v));
        let pat = pure("c").beat_with(pure(Fraction::from_integer(0)), div_pat);

        let cycle0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(cycle0.len(), 1);
        assert_eq!(cycle0[0].whole.as_ref().unwrap().end, frac(1, 16));

        let cycle1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        assert_eq!(cycle1.len(), 1);
        assert_eq!(
            cycle1[0].whole.as_ref().unwrap().end,
            &Fraction::from_integer(1) + frac(1, 8)
        );
    }

    #[test]
    fn beat_fragments_slot_across_source_hap_boundary() {
        // The beat slot [1/3, 2/3) straddles the source boundary at 1/2, so
        // the join emits one fragment per source hap: both keep the whole
        // slot as their whole, but only the first fragment carries the onset.
        let src = fastcat(vec![pure("c"), pure("e")]);
        let pat = src.beat_with(
            pure(Fraction::from_integer(1)),
            pure(Fraction::from_integer(3)),
        );
        let mut haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        haps.sort_by(|a, b| a.part.begin.cmp(&b.part.begin));

        assert_eq!(haps.len(), 2);
        for hap in &haps {
            let whole = hap.whole.as_ref().unwrap();
            assert_eq!(whole.begin, frac(1, 3));
            assert_eq!(whole.end, frac(2, 3));
        }
        assert_eq!(haps[0].value, "c");
        assert!(haps[0].has_onset());
        assert_eq!(haps[0].part.end, frac(1, 2));
        assert_eq!(haps[1].value, "e");
        assert!(!haps[1].has_onset());
        assert_eq!(haps[1].part.begin, frac(1, 2));
    }

    fn t_pat(source: &str) -> Pattern<Fraction> {
        convert::<f64>(&parse_ast(source).unwrap())
            .unwrap()
            .fmap(|v| Fraction::from(*v))
    }

    /// Query one integer cycle and return each onset's beat index within
    /// that cycle, as `part.begin` scaled by `div`.
    fn onset_beats(pat: &Pattern<&'static str>, cycle: i64, div: i64) -> Vec<Fraction> {
        let mut haps = pat.query_arc(
            Fraction::from_integer(cycle),
            Fraction::from_integer(cycle + 1),
        );
        haps.sort_by(|a, b| a.part.begin.cmp(&b.part.begin));
        haps.iter()
            .filter(|h| h.has_onset())
            .map(|h| (&h.part.begin - &Fraction::from_integer(cycle)) * Fraction::from_integer(div))
            .collect()
    }

    fn beats(values: &[i64]) -> Vec<Fraction> {
        values.iter().map(|v| Fraction::from_integer(*v)).collect()
    }

    #[test]
    fn beat_alternating_t_voice_changes_per_cycle() {
        // "0,2,<4 6>": the slowcat voice alternates its beat per cycle while
        // the other stack voices stay put.
        let pat = pure("c").beat_with(t_pat("0,2,<4 6>"), pure(Fraction::from_integer(16)));
        assert_eq!(onset_beats(&pat, 0, 16), beats(&[0, 2, 4]));
        assert_eq!(onset_beats(&pat, 1, 16), beats(&[0, 2, 6]));
        assert_eq!(onset_beats(&pat, 2, 16), beats(&[0, 2, 4]));
    }

    #[test]
    fn beat_comma_inside_angle_brackets_stacks_voices() {
        // "<4,6>" is a stack of alternation voices — both fire every cycle
        // (matching Strudel's mini-notation, where alternation needs spaces).
        let pat = pure("c").beat_with(t_pat("0,2,<4,6>"), pure(Fraction::from_integer(16)));
        assert_eq!(onset_beats(&pat, 0, 16), beats(&[0, 2, 4, 6]));
        assert_eq!(onset_beats(&pat, 1, 16), beats(&[0, 2, 4, 6]));
    }

    #[test]
    fn beat_nested_alternation_in_t() {
        // "0,<2 <4 6>>": the inner alternation advances only on the cycles
        // the outer one selects it, giving the period-4 sequence 2,4,2,6.
        let pat = pure("c").beat_with(t_pat("0,<2 <4 6>>"), pure(Fraction::from_integer(16)));
        assert_eq!(onset_beats(&pat, 0, 16), beats(&[0, 2]));
        assert_eq!(onset_beats(&pat, 1, 16), beats(&[0, 4]));
        assert_eq!(onset_beats(&pat, 2, 16), beats(&[0, 2]));
        assert_eq!(onset_beats(&pat, 3, 16), beats(&[0, 6]));
    }
}
