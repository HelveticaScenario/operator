//! Strudel-style pattern system for generating time-varying values.
//!
//! This module provides a functional reactive programming framework for
//! representing cyclic, time-varying patterns. At its core, a `Pattern<T>`
//! is a lazy query function that generates events (Haps) on demand for
//! any requested time range.
//!
//! # Key Concepts
//!
//! - **Pattern<T>**: A query function `State → Vec<Hap<T>>` that generates events lazily
//! - **Hap<T>**: An event with `whole` (full extent) and `part` (visible portion)
//! - **TimeSpan**: Half-open interval `[begin, end)` using exact rational time
//! - **Fraction**: Exact rational numbers for precise time (avoids float drift)
//!
//! # Example
//!
//! ```ignore
//! use modular_core::pattern_system::{Pattern, Fraction, pure, fastcat};
//!
//! // A pattern that cycles through 0, 1, 2 each cycle
//! let pat = fastcat(vec![pure(0.0), pure(1.0), pure(2.0)]);
//!
//! // Query for events in cycle 0
//! let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
//! assert_eq!(haps.len(), 3);
//! ```

mod fraction;
mod hap;
mod state;
mod timespan;

pub mod applicative;
pub mod combinators;
pub mod constructors;
pub mod euclidean;
pub mod mini;
pub mod monadic;
pub mod random;

pub use fraction::Fraction;
pub use hap::{ArenaHap, ArenaHapContext, DspHap, Hap, HapContext, SourceSpan};
pub use state::State;
pub use timespan::TimeSpan;

pub use combinators::{fastcat, slowcat, stack, timecat};
pub use constructors::{pure, pure_with_span, signal, silence};

// Re-export mini notation types
pub use mini::{FromMiniAtom, HasRest};

use std::sync::Arc;

#[allow(unused_imports)]
use bumpalo::collections::CollectIn;

/// Query function that pushes haps into a caller-supplied bumpalo arena
/// buffer, allowing intermediate haps to live in arena memory.
pub type ArenaQueryFn<T> = Arc<
    dyn for<'b> Fn(&State, &'b bumpalo::Bump, &mut bumpalo::collections::Vec<'b, ArenaHap<'b, T>>)
        + Send
        + Sync,
>;

/// Backing implementation for [`Pattern`]. `Arena` wraps a closure that
/// writes haps into an arena buffer; every other variant is a specialised
/// shape with an inline query body in [`Pattern::query`] /
/// [`Pattern::query_into`].
#[derive(Clone)]
enum PatternImpl<T> {
    Arena(ArenaQueryFn<T>),
    /// A single value, emitted once per cycle. Used by `pure` /
    /// `pure_with_span`.
    PureSpan {
        value: T,
        source_span: Option<SourceSpan>,
    },
    /// A pattern that emits no haps.
    Silence,
    /// Stack of patterns — every child contributes its haps at the query
    /// time span.
    Stack(Arc<[Pattern<T>]>),
    /// Fastcat (sequence) — each child plays in its 1/n slot of every
    /// cycle. Slot offsets are pre-computed at construction.
    Fastcat(Arc<FastcatData<T>>),
    /// Slowcat — one child per cycle, looping through `pats` mod n.
    Slowcat(Arc<SlowcatData<T>>),
    /// Constant-factor time scaling for `_fast(factor)`.
    FastConst(Arc<FastConstData<T>>),
    /// `strip_modifier_spans` wrapper — folds each hap's modifier-side
    /// spans back into the source side at extract time.
    StripModifierSpans(Arc<Pattern<T>>),
    /// `with_modifier_span` wrapper — combines each hap's context with a
    /// leaf context carrying the modifier span.
    WithModifierSpan(Arc<WithModifierSpanData<T>>),
    /// `compress(begin, end)` — pattern squeezed into [begin, end) of every
    /// cycle. Used by `timecat` (mini-notation `[a@N b@M]/X` weighted seqs).
    Compress(Arc<CompressData<T>>),
    /// Constant-arg Euclidean rhythm: emits N slot haps per cycle, each
    /// holding `value` or `rest` according to the Bjorklund rhythm, with
    /// the (K, N, R) source spans attached as modifier spans.
    EuclidConst(Arc<EuclidConstData<T>>),
}

/// Backing data for [`PatternImpl::EuclidConst`].
pub(crate) struct EuclidConstData<T> {
    pub value: T,
    pub rest: T,
    pub value_span: SourceSpan,
    /// Source spans for the (K, N, R) atoms, attached as modifier spans
    /// on each emitted hap.
    pub pulses_span: SourceSpan,
    pub steps_span: SourceSpan,
    pub rotation_span: Option<SourceSpan>,
    /// Pre-computed slot data: (begin_offset, end_offset, is_pulse).
    pub slots: Arc<[(Fraction, Fraction, bool)]>,
}

/// Backing data for [`PatternImpl::Compress`].
pub(crate) struct CompressData<T> {
    pub pat: Pattern<T>,
    pub begin: Fraction,
    pub end: Fraction,
    pub duration: Fraction,
}

/// Backing data for [`PatternImpl::WithModifierSpan`].
pub(crate) struct WithModifierSpanData<T> {
    pub pat: Pattern<T>,
    pub span: SourceSpan,
}

/// Backing data for [`PatternImpl::FastConst`].
pub(crate) struct FastConstData<T> {
    pub pat: Pattern<T>,
    pub factor: Fraction,
}

/// Backing data for [`PatternImpl::Fastcat`].
pub(crate) struct FastcatData<T> {
    pub pats: Arc<[Pattern<T>]>,
    pub n: usize,
    pub n_frac: Fraction,
    pub slot_offsets: Arc<[(Fraction, Fraction)]>,
}

/// Backing data for [`PatternImpl::Slowcat`].
pub(crate) struct SlowcatData<T> {
    pub pats: Arc<[Pattern<T>]>,
    pub n: usize,
    pub n_frac: Fraction,
}

/// A pattern is a lazy, query-based generator of time-varying values.
///
/// Patterns don't store events - they generate them on demand when queried.
/// This enables infinite, cyclic patterns that can be composed, transformed,
/// and combined without materializing the entire timeline.
#[derive(Clone)]
pub struct Pattern<T> {
    /// The query function that generates events.
    query: PatternImpl<T>,
    /// Number of steps per cycle (for alignment operations).
    steps: Option<Fraction>,
}

impl<T: Clone + Send + Sync + 'static> Pattern<T> {
    /// Build a `pure` / `pure_with_span` leaf pattern: one hap per cycle
    /// holding `value`, optionally tagged with `source_span`.
    pub fn new_pure(value: T, source_span: Option<SourceSpan>) -> Self {
        Pattern {
            query: PatternImpl::PureSpan { value, source_span },
            steps: Some(Fraction::from_integer(1)),
        }
    }

    /// Build a silence pattern that emits no haps. `steps` becomes the
    /// pattern's per-cycle step count.
    pub fn new_silence(steps: Fraction) -> Self {
        Pattern {
            query: PatternImpl::Silence,
            steps: Some(steps),
        }
    }

    /// Build a stack pattern from a list of children. `steps` is the LCM of
    /// the children's step counts (None if any child is step-less).
    pub fn new_stack(pats: Vec<Pattern<T>>, steps: Option<Fraction>) -> Self {
        Pattern {
            query: PatternImpl::Stack(pats.into()),
            steps,
        }
    }

    /// Build a slowcat pattern: one child per cycle, looping through the
    /// list.
    pub fn new_slowcat(pats: Vec<Pattern<T>>) -> Self {
        let n = pats.len();
        let n_frac = Fraction::from_integer(n as i64);
        Pattern {
            query: PatternImpl::Slowcat(Arc::new(SlowcatData {
                pats: pats.into(),
                n,
                n_frac,
            })),
            steps: None,
        }
    }

    /// Build a `_fast(factor)` pattern with a constant time-scale factor.
    /// Used by `*N` in mini-notation when N is a literal.
    pub fn new_fast_const(pat: Pattern<T>, factor: Fraction) -> Self {
        let steps = pat.steps.clone();
        Pattern {
            query: PatternImpl::FastConst(Arc::new(FastConstData { pat, factor })),
            steps,
        }
    }

    /// Build a constant-arg Euclidean pattern for `value(K, N[, R])` in
    /// mini-notation. Each cycle emits N slot haps holding `value` or
    /// `rest` per the Bjorklund rhythm, with the (K, N, R) source spans
    /// attached as modifier spans on each hap.
    pub fn new_euclid_const(
        value: T,
        rest: T,
        value_span: SourceSpan,
        pulses: i32,
        steps: u32,
        rotation: i32,
        pulses_span: SourceSpan,
        steps_span: SourceSpan,
        rotation_span: Option<SourceSpan>,
    ) -> Self {
        let n = steps as usize;
        let n_frac = Fraction::from_integer(n as i64);
        // Pre-compute (begin_off, end_off, is_pulse) per slot.
        let rhythm =
            crate::pattern_system::euclidean::euclidean_rhythm(pulses, steps, Some(rotation));
        let slots: Arc<[(Fraction, Fraction, bool)]> = rhythm
            .iter()
            .enumerate()
            .map(|(i, &is_pulse)| {
                let i_frac = Fraction::from_integer(i as i64);
                let begin = &i_frac / &n_frac;
                let end = (&i_frac + Fraction::from_integer(1)) / &n_frac;
                (begin, end, is_pulse)
            })
            .collect::<Vec<_>>()
            .into();
        Pattern {
            query: PatternImpl::EuclidConst(Arc::new(EuclidConstData {
                value,
                rest,
                value_span,
                pulses_span,
                steps_span,
                rotation_span,
                slots,
            })),
            steps: Some(n_frac),
        }
    }

    /// Build a `compress(begin, end)` pattern: squeeze the source pattern
    /// into the [begin, end) sub-interval of every cycle.
    pub fn new_compress(pat: Pattern<T>, begin: Fraction, end: Fraction) -> Self {
        let duration = &end - &begin;
        Pattern {
            query: PatternImpl::Compress(Arc::new(CompressData {
                pat,
                begin,
                end,
                duration,
            })),
            steps: None,
        }
    }

    /// Build a fastcat pattern: each child plays in its 1/n slot of every
    /// cycle. Slot offsets are pre-computed at construction. `steps` is
    /// the pattern's per-cycle step count.
    pub fn new_fastcat(pats: Vec<Pattern<T>>, steps: Fraction) -> Self {
        let n = pats.len();
        let n_frac = Fraction::from_integer(n as i64);
        let slot_offsets: Arc<[(Fraction, Fraction)]> = (0..n)
            .map(|i| {
                let i_frac = Fraction::from_integer(i as i64);
                let begin = &i_frac / &n_frac;
                let end = (&i_frac + Fraction::from_integer(1)) / &n_frac;
                (begin, end)
            })
            .collect::<Vec<_>>()
            .into();
        Pattern {
            query: PatternImpl::Fastcat(Arc::new(FastcatData {
                pats: pats.into(),
                n,
                n_frac,
                slot_offsets,
            })),
            steps: Some(steps),
        }
    }

    pub fn new_into<F>(query: F) -> Self
    where
        F: for<'b> Fn(
                &State,
                &'b bumpalo::Bump,
                &mut bumpalo::collections::Vec<'b, ArenaHap<'b, T>>,
            )
            + Send
            + Sync
            + 'static,
    {
        Pattern {
            query: PatternImpl::Arena(Arc::new(query)),
            steps: None,
        }
    }

    /// Push haps directly into a caller-supplied bumpalo arena buffer.
    /// Each variant writes haps into `out` using arena storage for any
    /// intermediate state.
    pub fn query_into<'b>(
        &self,
        state: &State,
        bump: &'b bumpalo::Bump,
        out: &mut bumpalo::collections::Vec<'b, ArenaHap<'b, T>>,
    ) {
        match &self.query {
            PatternImpl::Arena(f) => f(state, bump, out),
            PatternImpl::PureSpan { value, source_span } => {
                let leaf = match source_span {
                    Some(s) => ArenaHapContext::with_span_in(s.clone(), bump),
                    None => ArenaHapContext::empty_ref(),
                };
                state.span.for_each_cycle_span(|subspan| {
                    let whole = subspan.begin.whole_cycle();
                    out.push(ArenaHap {
                        whole: Some(whole),
                        part: subspan.clone(),
                        value: value.clone(),
                        context: leaf,
                    });
                });
            }
            PatternImpl::Silence => {}
            PatternImpl::Stack(pats) => {
                for pat in pats.iter() {
                    pat.query_into(state, bump, out);
                }
            }
            PatternImpl::Fastcat(data) => {
                fastcat_query_into(data, state, bump, out);
            }
            PatternImpl::Slowcat(data) => {
                slowcat_query_into(data, state, bump, out);
            }
            PatternImpl::FastConst(data) => {
                fast_const_query_into(data, state, bump, out);
            }
            PatternImpl::StripModifierSpans(inner) => {
                let start = out.len();
                inner.query_into(state, bump, out);
                // Walk the haps we just pushed and wrap their contexts so
                // that any modifier-side spans extract as source.
                for hap in &mut out[start..] {
                    hap.context = ArenaHapContext::strip_in(hap.context, bump);
                }
            }
            PatternImpl::WithModifierSpan(data) => {
                let start = out.len();
                data.pat.query_into(state, bump, out);
                // Construct the modifier leaf once and combine into each hap's context.
                let leaf = ArenaHapContext::with_span_in(data.span.clone(), bump);
                for hap in &mut out[start..] {
                    hap.context = ArenaHapContext::combine_in(hap.context, leaf, bump);
                }
            }
            PatternImpl::Compress(data) => {
                compress_query_into(data, state, bump, out);
            }
            PatternImpl::EuclidConst(data) => {
                euclid_const_query_into(data, state, bump, out);
            }
        }
    }

    /// Query for a specific time range, materialising haps into a
    /// heap-allocated `Vec<Hap<T>>`. The arena-aware
    /// [`Pattern::query_cycle_all_into`] / [`Pattern::query_into`] is the
    /// zero-allocation alternative.
    pub fn query_arc(&self, begin: Fraction, end: Fraction) -> Vec<Hap<T>> {
        let state = State::new(TimeSpan::new(begin, end));
        let bump = bumpalo::Bump::new();
        let mut out: bumpalo::collections::Vec<'_, ArenaHap<'_, T>> =
            bumpalo::collections::Vec::new_in(&bump);
        self.query_into(&state, &bump, &mut out);
        out.iter().map(|h| h.to_owned()).collect()
    }

    /// Get the number of steps per cycle (if set).
    pub fn steps(&self) -> Option<&Fraction> {
        self.steps.as_ref()
    }

    /// Set the number of steps per cycle.
    pub fn with_steps(mut self, steps: Fraction) -> Self {
        self.steps = Some(steps);
        self
    }

    // ===== Functor Operations =====

    /// Map a function over the values (functor fmap).
    pub fn fmap<U, F>(&self, f: F) -> Pattern<U>
    where
        U: Clone + Send + Sync + 'static,
        F: Fn(&T) -> U + Clone + Send + Sync + 'static,
    {
        let pat = self.clone();
        let steps = self.steps.clone();
        let mut result = Pattern::new_into(
            move |state: &State,
                  bump: &bumpalo::Bump,
                  out: &mut bumpalo::collections::Vec<'_, ArenaHap<'_, U>>| {
                let mut scratch: bumpalo::collections::Vec<'_, ArenaHap<'_, T>> =
                    bumpalo::collections::Vec::new_in(bump);
                pat.query_into(state, bump, &mut scratch);
                out.reserve(scratch.len());
                for hap in scratch {
                    out.push(ArenaHap {
                        whole: hap.whole,
                        part: hap.part,
                        value: f(&hap.value),
                        context: hap.context,
                    });
                }
            },
        );
        if let Some(s) = steps {
            result.steps = Some(s);
        }
        result
    }

    /// Add a modifier span to all haps in this pattern.
    /// Used for tracking which operators are active during editor highlighting.
    pub fn with_modifier_span(&self, span: SourceSpan) -> Pattern<T> {
        Pattern {
            query: PatternImpl::WithModifierSpan(Arc::new(WithModifierSpanData {
                pat: self.clone(),
                span,
            })),
            steps: self.steps.clone(),
        }
    }

    /// Remove all modifier spans from haps in this pattern.
    /// Used before combining patterns so that inner modifier spans
    /// (e.g. from euclidean sub-expressions) don't shift the positional
    /// index that `extract_pattern_spans` relies on.
    pub fn strip_modifier_spans(&self) -> Pattern<T> {
        Pattern {
            query: PatternImpl::StripModifierSpans(Arc::new(self.clone())),
            steps: self.steps.clone(),
        }
    }

    // ===== DSP Fast-Path Methods =====
    //
    // f64-flavoured wrappers used by DSP code. The pattern itself runs
    // exact rational arithmetic; these wrappers convert at the boundary.

    /// Query at a point and return the first matching hap (if any).
    pub fn query_at_first(&self, t: f64) -> Option<Hap<T>> {
        let cycle = t.floor();
        let haps = self.query_arc(Fraction::from(cycle), Fraction::from(cycle + 1.0));
        haps.into_iter().find(|hap| hap.part_contains_f64(t))
    }

    /// Get ALL events in a cycle as DspHaps (including fragments without onsets).
    ///
    /// Unlike `query_cycle_dsp`, this returns all haps that *intersect* the cycle,
    /// not just those with onsets. This is useful for caching where you need to
    /// handle haps that span across cycle boundaries.
    pub fn query_cycle_all(&self, cycle: i64) -> Vec<DspHap<T>> {
        let begin = Fraction::from_integer(cycle);
        let end = Fraction::from_integer(cycle + 1);
        self.query_arc(begin, end)
            .into_iter()
            .map(|h| h.to_dsp_hap())
            .collect()
    }

    /// Arena-aware variant of [`query_cycle_all`]. Writes the cycle's haps
    /// directly into the caller's bumpalo arena.
    pub fn query_cycle_all_into<'b>(
        &self,
        cycle: i64,
        bump: &'b bumpalo::Bump,
        out: &mut bumpalo::collections::Vec<'b, ArenaHap<'b, T>>,
    ) {
        let begin = Fraction::from_integer(cycle);
        let end = Fraction::from_integer(cycle + 1);
        self.query_into(&State::new(TimeSpan::new(begin, end)), bump, out);
    }
}

/// Arena-direct query body for [`PatternImpl::Fastcat`].
fn fastcat_query_into<'b, T: Clone + Send + Sync + 'static>(
    data: &FastcatData<T>,
    state: &State,
    bump: &'b bumpalo::Bump,
    out: &mut bumpalo::collections::Vec<'b, ArenaHap<'b, T>>,
) {
    let n = data.n;
    let pats = &data.pats;
    let n_frac = &data.n_frac;
    let slot_offsets = &data.slot_offsets;
    // Both the per-slot query and the result mapping are linear in t.
    // Compute their constants once per cycle so the per-slot work is
    // a single multiply + add.
    //
    // Forward (query):  t' = n*t - cs*(n-1) - i        ↔  t' = a_q * t + b_q
    //   where a_q = n, b_q = -(cs*(n-1) + i)
    // Inverse (result): t' = (t + i)/n + cs*(n-1)/n     ↔  t' = a_r * t + b_r
    //   where a_r = 1/n, b_r = (i + cs*(n-1)) / n
    state.span.for_each_cycle_span(|cycle_span| {
        let cycle_start = cycle_span.begin.floor();
        let cs_times_nm1 = &cycle_start * &(n_frac - Fraction::from_integer(1));
        let inv_n = Fraction::from_integer(1) / n_frac.clone();
        // When the cycle span is an integer-aligned [N, N+1) interval, each
        // part_span lies wholly inside it and the intersect collapses to
        // part_span itself.
        let full_cycle = cycle_span.begin.is_integer()
            && cycle_span.end.is_integer()
            && cycle_span.end.numer() == cycle_span.begin.numer() + 1;
        for i in 0..n {
            let i_frac = Fraction::from_integer(i as i64);
            let (begin_off, end_off) = &slot_offsets[i];
            let part_begin = &cycle_start + begin_off;
            let part_end = &cycle_start + end_off;
            let part_span = TimeSpan {
                begin: part_begin,
                end: part_end,
            };
            let query_part = if full_cycle {
                part_span.clone()
            } else {
                let Some(qp) = cycle_span.intersection(&part_span) else {
                    continue;
                };
                qp
            };

            // Forward transform: t' = n*t - (cs*(n-1) + i)
            let b_q = &cs_times_nm1 + &i_frac;
            let query_transformed = query_part.with_time(|t| t * n_frac - &b_q);

            let mut scratch: bumpalo::collections::Vec<'_, ArenaHap<'_, T>> =
                bumpalo::collections::Vec::new_in(bump);
            pats[i].query_into(&State::new(query_transformed), bump, &mut scratch);

            // Inverse transform: t' = (t + i + cs*(n-1)) / n
            //                       = t * (1/n) + (i + cs*(n-1)) / n
            let b_r_num = &cs_times_nm1 + &i_frac;
            let b_r = &b_r_num / n_frac;
            out.reserve(scratch.len());
            for hap in scratch {
                let new_part = hap.part.with_time(|t| t * &inv_n + &b_r);
                let new_whole = hap.whole.as_ref().map(|w| w.with_time(|t| t * &inv_n + &b_r));
                out.push(ArenaHap {
                    whole: new_whole,
                    part: new_part,
                    value: hap.value,
                    context: hap.context,
                });
            }
        }
    });
}

/// Arena-direct query body for [`PatternImpl::Slowcat`]: pick child
/// `cycle mod n` and shift its haps back into the requested cycle.
fn slowcat_query_into<'b, T: Clone + Send + Sync + 'static>(
    data: &SlowcatData<T>,
    state: &State,
    bump: &'b bumpalo::Bump,
    out: &mut bumpalo::collections::Vec<'b, ArenaHap<'b, T>>,
) {
    let n = data.n;
    if n == 0 {
        return;
    }
    let pats = &data.pats;
    let n_frac = &data.n_frac;
    state.span.for_each_cycle_span(|subspan| {
        // When the subspan begins on an integer, the cycle number, modulo
        // and offset can all be derived from i64 math.
        let begin_is_integer = subspan.begin.is_integer();
        let cycle_num = if begin_is_integer {
            subspan.begin.numer()
        } else {
            subspan.begin.floor().numer()
        };
        let pat_idx = ((cycle_num % n as i64) + n as i64) as usize % n;
        let pat = &pats[pat_idx];

        let offset = if begin_is_integer {
            Fraction::from_integer(cycle_num - cycle_num.div_euclid(n as i64))
        } else {
            let begin_floor = Fraction::from_integer(cycle_num);
            &begin_floor - (&subspan.begin / n_frac).floor()
        };

        let query_span = subspan.with_time(|t| t - &offset);
        let mut scratch: bumpalo::collections::Vec<'_, ArenaHap<'_, T>> =
            bumpalo::collections::Vec::new_in(bump);
        pat.query_into(&State::new(query_span), bump, &mut scratch);

        out.reserve(scratch.len());
        for hap in scratch {
            let new_part = hap.part.with_time(|t| t + &offset);
            let new_whole = hap.whole.as_ref().map(|w| w.with_time(|t| t + &offset));
            out.push(ArenaHap {
                whole: new_whole,
                part: new_part,
                value: hap.value,
                context: hap.context,
            });
        }
    });
}

/// Arena-direct query body for [`PatternImpl::FastConst`]: scale the
/// query span up by `factor`, then map result spans back down by it.
fn fast_const_query_into<'b, T: Clone + Send + Sync + 'static>(
    data: &FastConstData<T>,
    state: &State,
    bump: &'b bumpalo::Bump,
    out: &mut bumpalo::collections::Vec<'b, ArenaHap<'b, T>>,
) {
    let pat = &data.pat;
    let factor = &data.factor;
    let new_span = state.span.with_time(|t| t * factor);
    let mut scratch: bumpalo::collections::Vec<'_, ArenaHap<'_, T>> =
        bumpalo::collections::Vec::new_in(bump);
    pat.query_into(&State::new(new_span), bump, &mut scratch);
    out.reserve(scratch.len());
    for hap in scratch {
        let new_part = hap.part.with_time(|t| t / factor);
        let new_whole = hap.whole.as_ref().map(|w| w.with_time(|t| t / factor));
        out.push(ArenaHap {
            whole: new_whole,
            part: new_part,
            value: hap.value,
            context: hap.context,
        });
    }
}

/// Arena-direct query body for [`PatternImpl::Compress`]: query the source
/// at the time-stretch that fills [begin, end), then map results back
/// into that sub-interval of each cycle.
fn compress_query_into<'b, T: Clone + Send + Sync + 'static>(
    data: &CompressData<T>,
    state: &State,
    bump: &'b bumpalo::Bump,
    out: &mut bumpalo::collections::Vec<'b, ArenaHap<'b, T>>,
) {
    let pat = &data.pat;
    let begin_clone = &data.begin;
    let end_clone = &data.end;
    let duration = &data.duration;
    // Forward and inverse mappings are both linear in t.
    //   Forward (query):  t' = (t - cb)/d + cycle   ↔  t' = a_q * t + b_q
    //     a_q = 1/d,  b_q = cycle - cb/d
    //   Inverse (result): t' = (t - cycle)*d + cb   ↔  t' = a_r * t + b_r
    //     a_r = d,    b_r = cb - cycle*d
    let inv_d = Fraction::from_integer(1) / duration.clone();
    state.span.for_each_cycle_span(|cycle_span| {
        let cycle = cycle_span.begin.sam();
        let compressed_begin = &cycle + begin_clone;
        let compressed_end = &cycle + end_clone;
        let compressed_span = TimeSpan::new(compressed_begin.clone(), compressed_end);
        // For an integer-aligned cycle_span, compressed_span (a sub-range
        // of the same cycle) lies wholly inside it.
        let full_cycle = cycle_span.begin.is_integer()
            && cycle_span.end.is_integer()
            && cycle_span.end.numer() == cycle_span.begin.numer() + 1;
        let intersect = if full_cycle {
            compressed_span.clone()
        } else {
            let Some(i) = cycle_span.intersection(&compressed_span) else {
                return;
            };
            i
        };
        let b_q = &cycle - &(&compressed_begin * &inv_d);
        let inner_span = intersect.with_time(|t| t * &inv_d + &b_q);

        let mut scratch: bumpalo::collections::Vec<'_, ArenaHap<'_, T>> =
            bumpalo::collections::Vec::new_in(bump);
        pat.query_into(&State::new(inner_span), bump, &mut scratch);

        let b_r = &compressed_begin - &(&cycle * duration);
        for hap in scratch {
            let new_part = hap.part.with_time(|t| t * duration + &b_r);
            let new_whole = hap.whole.as_ref().map(|w| w.with_time(|t| t * duration + &b_r));
            if let Some(final_part) = new_part.intersection(&cycle_span) {
                out.push(ArenaHap {
                    whole: new_whole,
                    part: final_part,
                    value: hap.value.clone(),
                    context: hap.context,
                });
            }
        }
    });
}

/// Arena-direct query body for [`PatternImpl::EuclidConst`]. Each cycle
/// emits `N` slot haps holding `value` or `rest` per the precomputed
/// Bjorklund slots, tagged with the (value, K, N, R) modifier spans.
fn euclid_const_query_into<'b, T: Clone + Send + Sync + 'static>(
    data: &EuclidConstData<T>,
    state: &State,
    bump: &'b bumpalo::Bump,
    out: &mut bumpalo::collections::Vec<'b, ArenaHap<'b, T>>,
) {
    // Build the per-hap context tree once. Every emitted hap shares it:
    //   ((value_leaf · pulses_mod) · steps_mod) · rotation_mod
    let value_leaf = ArenaHapContext::with_span_in(data.value_span.clone(), bump);
    let p_leaf = ArenaHapContext::with_span_in(data.pulses_span.clone(), bump);
    let s_leaf = ArenaHapContext::with_span_in(data.steps_span.clone(), bump);
    let mut ctx = ArenaHapContext::combine_in(value_leaf, p_leaf, bump);
    ctx = ArenaHapContext::combine_in(ctx, s_leaf, bump);
    if let Some(rs) = &data.rotation_span {
        let r_leaf = ArenaHapContext::with_span_in(rs.clone(), bump);
        ctx = ArenaHapContext::combine_in(ctx, r_leaf, bump);
    }

    state.span.for_each_cycle_span(|cycle_span| {
        let cycle_start = cycle_span.begin.floor();
        for (begin_off, end_off, is_pulse) in data.slots.iter() {
            let part_begin = &cycle_start + begin_off;
            let part_end = &cycle_start + end_off;
            let whole_span = TimeSpan::new(part_begin, part_end);
            if let Some(part) = cycle_span.intersection(&whole_span) {
                let value = if *is_pulse {
                    data.value.clone()
                } else {
                    data.rest.clone()
                };
                out.push(ArenaHap {
                    whole: Some(whole_span),
                    part,
                    value,
                    context: ctx,
                });
            }
        }
    });
}

impl<T: Clone + Send + Sync + 'static> std::fmt::Debug for Pattern<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pattern {{ steps: {:?} }}", self.steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pure_pattern() {
        let pat = pure(42);
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 1);
        assert_eq!(haps[0].value, 42);
        assert!(haps[0].has_onset());
    }

    #[test]
    fn test_silence_pattern() {
        let pat: Pattern<i32> = silence();
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 0);
    }

    #[test]
    fn test_fmap() {
        let pat = pure(10);
        let doubled = pat.fmap(|x| x * 2);
        let haps = doubled.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 1);
        assert_eq!(haps[0].value, 20);
    }

    #[test]
    fn test_fastcat() {
        let pat = fastcat(vec![pure(0), pure(1), pure(2)]);
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 3);
        assert_eq!(haps[0].value, 0);
        assert_eq!(haps[1].value, 1);
        assert_eq!(haps[2].value, 2);
    }

    #[test]
    fn test_stack() {
        let pat = stack(vec![pure(0), pure(1)]);
        let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));

        assert_eq!(haps.len(), 2);
        // Both values should be present (order may vary)
        let values: Vec<_> = haps.iter().map(|h| h.value).collect();
        assert!(values.contains(&0));
        assert!(values.contains(&1));
    }

    #[test]
    fn test_slowcat() {
        let pat = slowcat(vec![pure(0), pure(1), pure(2)]);

        // Cycle 0 should have value 0
        let haps0 = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
        assert_eq!(haps0.len(), 1);
        assert_eq!(haps0[0].value, 0);

        // Cycle 1 should have value 1
        let haps1 = pat.query_arc(Fraction::from_integer(1), Fraction::from_integer(2));
        assert_eq!(haps1.len(), 1);
        assert_eq!(haps1[0].value, 1);

        // Cycle 2 should have value 2
        let haps2 = pat.query_arc(Fraction::from_integer(2), Fraction::from_integer(3));
        assert_eq!(haps2.len(), 1);
        assert_eq!(haps2[0].value, 2);

        // Cycle 3 should wrap back to 0
        let haps3 = pat.query_arc(Fraction::from_integer(3), Fraction::from_integer(4));
        assert_eq!(haps3.len(), 1);
        assert_eq!(haps3[0].value, 0);
    }

    // ===== DSP Fast-Path Pattern Methods =====

    #[test]
    fn test_query_at_first() {
        let pat = fastcat(vec![pure(0), pure(1), pure(2)]);

        let h = pat.query_at_first(0.4);
        assert!(h.is_some());
        assert_eq!(h.unwrap().value, 1);
    }

    #[test]
    fn test_query_at_first_none() {
        let pat: Pattern<i32> = silence();
        let h = pat.query_at_first(0.5);
        assert!(h.is_none());
    }

    #[test]
    fn test_query_cycle_all_includes_fragments() {
        // query_cycle_all should include all haps intersecting the cycle,
        // including fragments (haps without onsets that started in a prior cycle)
        let pat = pure(42);
        let events = pat.query_cycle_all(0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].value, 42);
        // The hap should have an onset since it starts in this cycle
        assert!(events[0].has_onset());
    }

    #[test]
    fn test_strip_modifier_spans_preserves_in_source_extra() {
        use crate::pattern_system::constructors::pure_with_span;
        use crate::pattern_system::hap::SourceSpan;

        let pat = pure_with_span(1.0f64, SourceSpan::new(0, 1))
            .with_modifier_span(SourceSpan::new(5, 6));
        let stripped = pat.strip_modifier_spans();
        let haps = stripped.query_arc(
            Fraction::from_integer(0.into()),
            Fraction::from_integer(1.into()),
        );
        assert_eq!(haps.len(), 1);
        assert!(haps[0].context.modifier_spans.is_empty());
        assert_eq!(haps[0].context.source_extra_spans.len(), 1);
        assert_eq!(haps[0].context.source_extra_spans[0].to_tuple(), (5, 6));
    }
}
