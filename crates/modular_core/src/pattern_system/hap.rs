//! Hap (happening/event) type for pattern events.
//!
//! A Hap represents an event occurrence within a pattern. The key distinction
//! is between `whole` (the full logical extent of the event) and `part`
//! (the portion visible in the current query window).
//!
//! Two flavours:
//! - `Hap<T>` — owned, used by the `query` API.
//! - `ArenaHap<'b, T>` — bumpalo-allocated, used by `query_into` for
//!   zero-allocation paths. Combinators construct these directly into a
//!   per-query arena.

use bumpalo::Bump;

use super::TimeSpan;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Source location in the original pattern string.
/// Used for editor highlighting.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct SourceSpan {
    /// Start offset in the source string (0-indexed).
    pub start: usize,
    /// End offset in the source string (exclusive).
    pub end: usize,
}

impl SourceSpan {
    /// Create a new source span.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Convert to a tuple for serialization.
    pub fn to_tuple(&self) -> (usize, usize) {
        (self.start, self.end)
    }
}

/// Context information attached to a Hap.
/// Contains source spans for editor highlighting.
#[derive(Clone, Debug, Default)]
pub struct HapContext {
    /// Primary source location (the main atom/value).
    pub source_span: Option<SourceSpan>,
    /// Extra spans associated with source_span (from pattern-internal modifiers
    /// like `*<4 6>`, preserved through strip_modifier_spans).
    pub source_extra_spans: Vec<SourceSpan>,
    /// Spans from the right side of each `combine()`, in combine order.
    pub modifier_spans: Vec<SourceSpan>,
    /// Extra spans for each modifier_spans entry (parallel indexing).
    /// Carries source_extra_spans from the right side of combine().
    pub modifier_extra_spans: Vec<Vec<SourceSpan>>,
}

impl HapContext {
    /// Create a context with a source span.
    pub fn with_span(span: SourceSpan) -> Self {
        Self {
            source_span: Some(span),
            source_extra_spans: Vec::new(),
            modifier_spans: Vec::new(),
            modifier_extra_spans: Vec::new(),
        }
    }


    /// Combine two contexts (e.g., when combining haps in applicative operations).
    pub fn combine(&self, other: &HapContext) -> HapContext {
        let mut combined = self.clone();
        // Add the other's source span as a modifier span (positional indexing for app_left)
        if let Some(span) = &other.source_span {
            combined.modifier_spans.push(span.clone());
        }
        // Carry the other's source_extra_spans alongside the modifier entry just added
        combined
            .modifier_extra_spans
            .push(other.source_extra_spans.clone());
        // Carry over other's modifier_spans and their parallel extras
        combined
            .modifier_spans
            .extend(other.modifier_spans.iter().cloned());
        combined
            .modifier_extra_spans
            .extend(other.modifier_extra_spans.iter().cloned());
        combined
    }

    /// Collect every span (source + modifier) as a tuple list for JSON
    /// serialisation.
    pub fn get_all_span_tuples(&self) -> Vec<(usize, usize)> {
        self.source_span
            .iter()
            .chain(self.source_extra_spans.iter())
            .chain(self.modifier_spans.iter())
            .chain(self.modifier_extra_spans.iter().flatten())
            .map(|s| s.to_tuple())
            .collect()
    }
}

/// An event (happening) with temporal extent and value.
///
/// - `whole`: The full logical extent of the event (None for continuous signals)
/// - `part`: The portion of the event visible in the current query
/// - `value`: The event's value
/// - `context`: Metadata including source spans for highlighting
#[derive(Clone, Debug)]
pub struct Hap<T> {
    /// Full logical extent of the event (None for continuous signals).
    pub whole: Option<TimeSpan>,
    /// Portion visible in the current query.
    pub part: TimeSpan,
    /// The event's value.
    pub value: T,
    /// Context (source spans, etc.).
    pub context: HapContext,
}

impl<T: Clone> Hap<T> {
    /// Create a new hap.
    pub fn new(whole: Option<TimeSpan>, part: TimeSpan, value: T) -> Self {
        Self {
            whole,
            part,
            value,
            context: HapContext::default(),
        }
    }

    /// True if this hap includes its onset (start of whole == start of part).
    pub fn has_onset(&self) -> bool {
        match &self.whole {
            Some(whole) => whole.begin == self.part.begin,
            None => false,
        }
    }

    // ===== f64 Fast-Path Methods for DSP =====

    /// Get part begin time as f64.
    #[inline]
    pub fn part_begin_f64(&self) -> f64 {
        self.part.begin_f64()
    }

    /// Get part end time as f64.
    #[inline]
    pub fn part_end_f64(&self) -> f64 {
        self.part.end_f64()
    }

    /// Get whole begin time as f64 (or part begin if no whole).
    #[inline]
    pub fn whole_begin_f64(&self) -> f64 {
        self.whole
            .as_ref()
            .map_or_else(|| self.part.begin_f64(), |w| w.begin_f64())
    }

    /// Get whole end time as f64 (or part end if no whole).
    #[inline]
    pub fn whole_end_f64(&self) -> f64 {
        self.whole
            .as_ref()
            .map_or_else(|| self.part.end_f64(), |w| w.end_f64())
    }

    /// Check if time t is within the part span [begin, end).
    #[inline]
    pub fn part_contains_f64(&self, t: f64) -> bool {
        t >= self.part_begin_f64() && t < self.part_end_f64()
    }

    /// Convert to a DSP-friendly cached representation.
    pub fn to_dsp_hap(&self) -> DspHap<T> {
        DspHap {
            whole_begin: self.whole_begin_f64(),
            whole_end: self.whole_end_f64(),
            part_begin: self.part_begin_f64(),
            part_end: self.part_end_f64(),
            value: self.value.clone(),
            context: self.context.clone(),
            has_whole: self.whole.is_some(),
        }
    }
}

/// Pre-computed f64 bounds for DSP contexts.
/// Avoids repeated BigRational→f64 conversion in sample-rate loops.
#[derive(Clone, Debug)]
pub struct DspHap<T> {
    /// Whole span begin (or part begin if continuous).
    pub whole_begin: f64,
    /// Whole span end (or part end if continuous).
    pub whole_end: f64,
    /// Part span begin.
    pub part_begin: f64,
    /// Part span end.
    pub part_end: f64,
    /// The event's value.
    pub value: T,
    /// Context (source spans, etc.).
    pub context: HapContext,
    /// Whether this hap has a whole span (discrete vs continuous).
    pub has_whole: bool,
}

impl<T: Clone> DspHap<T> {
    /// Check if time t is within the part span [begin, end).
    #[inline]
    pub fn part_contains(&self, t: f64) -> bool {
        t >= self.part_begin && t < self.part_end
    }

    /// True if this hap includes its onset (start of whole == start of part).
    /// A fragment (hap that started in a previous cycle) will return false.
    #[inline]
    pub fn has_onset(&self) -> bool {
        self.has_whole && (self.whole_begin - self.part_begin).abs() < 1e-9
    }

    /// Get all source spans as tuples for reporting to frontend.
    pub fn get_active_spans(&self) -> Vec<(usize, usize)> {
        self.context.get_all_span_tuples()
    }
}

// ============================================================================
// Arena-allocated variants for the zero-alloc `query_into` API.
// ============================================================================

/// Tree-shaped, bumpalo-allocated context. Replaces the previous flat
/// representation (4× BumpVec) to make `combine_in` O(1): a `Combined` node
/// just stores two references to its children. Span ordering for extraction
/// is recovered by a depth-first walk, matching `HapContext::combine`'s
/// flat order:
///
/// ```text
/// combine(L, R) →
///   source = L.source
///   modifier_spans = L.modifier_spans
///                 ++ [R.source]
///                 ++ R.modifier_spans
/// ```
///
/// Equivalent walk for `Combined { primary: L, modifier: R }`:
/// 1. Recurse into L. Emit L's source as pattern 0, then L's modifier-chain
///    as patterns 1, 2, ...
/// 2. Then visit R. Emit R's source as the next pattern index, then R's
///    modifier-chain in turn.
///
/// `Stripped` collapses a sub-tree's modifier chain into the source side,
/// implementing `strip_modifier_spans` in O(1).
///
/// Variance: all references are immutable, so `ArenaHapContext<'b>` is
/// covariant in `'b`, allowing a `&'static ArenaHapContext<'static>::Empty`
/// to be reused everywhere `&'b ArenaHapContext<'b>` is expected.
#[derive(Debug)]
pub enum ArenaHapContext<'b> {
    Empty,
    Leaf(SourceSpan),
    Combined {
        primary: &'b ArenaHapContext<'b>,
        modifier: &'b ArenaHapContext<'b>,
    },
    /// Wraps a context so that every span (including those that would be
    /// emitted as modifier_spans by `Combined`) appears on the source side.
    /// Used by `Pattern::strip_modifier_spans`.
    Stripped(&'b ArenaHapContext<'b>),
}

/// Static `Empty` context. Used by leaf constructors that emit value-only
/// haps (e.g. `pure(value)`) and by combinators that need a no-op context.
/// Reusable across queries — `ArenaHapContext<'static>` coerces to
/// `&'b ArenaHapContext<'b>` via covariance.
static EMPTY_CTX: ArenaHapContext<'static> = ArenaHapContext::Empty;

impl<'b> ArenaHapContext<'b> {
    /// Reference to the static empty context. Cheaper than allocating an
    /// `Empty` in the arena.
    #[inline]
    pub fn empty_ref() -> &'static ArenaHapContext<'static> {
        &EMPTY_CTX
    }

    /// Allocate a `Leaf` carrying a single source span.
    pub fn with_span_in(span: SourceSpan, bump: &'b Bump) -> &'b ArenaHapContext<'b> {
        bump.alloc(ArenaHapContext::Leaf(span))
    }

    /// Combine two contexts. O(1): stores two references in a new `Combined`
    /// node. Span order is recovered by tree walk on extraction.
    ///
    /// Right is treated as a chain of modifier patterns appended to left.
    #[inline]
    pub fn combine_in(
        left: &'b ArenaHapContext<'b>,
        right: &'b ArenaHapContext<'b>,
        bump: &'b Bump,
    ) -> &'b ArenaHapContext<'b> {
        // Fast paths: skip allocation when one side carries no info.
        if matches!(right, ArenaHapContext::Empty) {
            return left;
        }
        if matches!(left, ArenaHapContext::Empty) {
            return right;
        }
        bump.alloc(ArenaHapContext::Combined {
            primary: left,
            modifier: right,
        })
    }

    /// Wrap so all spans (including modifier-side) appear as source on extract.
    #[inline]
    pub fn strip_in(
        ctx: &'b ArenaHapContext<'b>,
        bump: &'b Bump,
    ) -> &'b ArenaHapContext<'b> {
        if matches!(ctx, ArenaHapContext::Empty) {
            return ctx;
        }
        bump.alloc(ArenaHapContext::Stripped(ctx))
    }

    /// Walk the tree depth-first, calling `emit(pattern_idx, span)` for every
    /// span. Pattern 0 = source side of the root primary chain; each
    /// `Combined` modifier increments the pattern index.
    ///
    /// A `Stripped` node folds its modifier-side spans back into the source
    /// pattern_idx for the rest of the subtree.
    pub fn walk<F: FnMut(u8, &SourceSpan)>(&self, emit: &mut F) {
        // Pattern index counter — incremented when we step into a modifier
        // subtree at the root level.
        let mut next_pattern_idx: u8 = 0;
        self.walk_inner(0, false, &mut next_pattern_idx, emit);
    }

    fn walk_inner<F: FnMut(u8, &SourceSpan)>(
        &self,
        pattern_idx: u8,
        stripped: bool,
        next_pattern_idx: &mut u8,
        emit: &mut F,
    ) {
        match self {
            ArenaHapContext::Empty => {}
            ArenaHapContext::Leaf(span) => {
                if pattern_idx >= *next_pattern_idx {
                    *next_pattern_idx = pattern_idx + 1;
                }
                emit(pattern_idx, span);
            }
            ArenaHapContext::Combined { primary, modifier } => {
                // Visit primary in current pattern slot.
                primary.walk_inner(pattern_idx, stripped, next_pattern_idx, emit);
                if stripped {
                    // In a stripped subtree the modifier-side spans become
                    // additional source spans at the same pattern_idx.
                    modifier.walk_inner(pattern_idx, true, next_pattern_idx, emit);
                } else {
                    let next = *next_pattern_idx;
                    modifier.walk_inner(next, false, next_pattern_idx, emit);
                }
            }
            ArenaHapContext::Stripped(inner) => {
                inner.walk_inner(pattern_idx, true, next_pattern_idx, emit);
            }
        }
    }

    /// Materialise into an owned `HapContext` by walking the tree and
    /// grouping spans by pattern index.
    pub fn to_owned(&self) -> HapContext {
        let mut owned = HapContext::default();
        let mut current_idx: i32 = -1;
        let mut current_extras: Vec<SourceSpan> = Vec::new();
        self.walk(&mut |idx, span| {
            if idx as i32 != current_idx {
                flush_owned_extras(
                    &mut owned,
                    current_idx,
                    std::mem::take(&mut current_extras),
                );
                current_idx = idx as i32;
            }
            current_extras.push(span.clone());
        });
        flush_owned_extras(&mut owned, current_idx, current_extras);
        owned
    }

}

fn flush_owned_extras(owned: &mut HapContext, pattern_idx: i32, spans: Vec<SourceSpan>) {
    if spans.is_empty() {
        return;
    }
    if pattern_idx == 0 {
        let mut it = spans.into_iter();
        if owned.source_span.is_none() {
            owned.source_span = it.next();
        }
        owned.source_extra_spans.extend(it);
    } else {
        // For pattern_idx >= 1, first span is the modifier_span, rest are extras.
        let mut it = spans.into_iter();
        let head = it.next().unwrap();
        owned.modifier_spans.push(head);
        let extras: Vec<SourceSpan> = it.collect();
        owned.modifier_extra_spans.push(extras);
    }
}

/// Bumpalo-arena version of [`Hap`]. Stored in a `BumpVec` for zero-alloc
/// intermediate buffers. The `value` type stays owned (often `Clone + Send`),
/// the context is a *reference* into the arena — making `combine_in` O(1)
/// regardless of accumulated context depth.
#[derive(Debug)]
pub struct ArenaHap<'b, T> {
    pub whole: Option<TimeSpan>,
    pub part: TimeSpan,
    pub value: T,
    pub context: &'b ArenaHapContext<'b>,
}

impl<'b, T: Clone> ArenaHap<'b, T> {
    /// True if this hap includes its onset (whole begin == part begin).
    pub fn has_onset(&self) -> bool {
        match &self.whole {
            Some(whole) => whole.begin == self.part.begin,
            None => false,
        }
    }

    /// Whole span if present, otherwise the part.
    pub fn whole_or_part(&self) -> &TimeSpan {
        self.whole.as_ref().unwrap_or(&self.part)
    }

    /// Materialise into an owned `Hap` for callers that need ownership.
    pub fn to_owned(&self) -> Hap<T> {
        Hap {
            whole: self.whole.clone(),
            part: self.part.clone(),
            value: self.value.clone(),
            context: self.context.to_owned(),
        }
    }

    /// f64 part-begin (matches `Hap::part_begin_f64`).
    #[inline]
    pub fn part_begin_f64(&self) -> f64 {
        self.part.begin_f64()
    }

    /// f64 part-end.
    #[inline]
    pub fn part_end_f64(&self) -> f64 {
        self.part.end_f64()
    }

    /// f64 whole-begin (falls back to part begin if continuous).
    #[inline]
    pub fn whole_begin_f64(&self) -> f64 {
        self.whole
            .as_ref()
            .map_or_else(|| self.part.begin_f64(), |w| w.begin_f64())
    }

    /// f64 whole-end.
    #[inline]
    pub fn whole_end_f64(&self) -> f64 {
        self.whole
            .as_ref()
            .map_or_else(|| self.part.end_f64(), |w| w.end_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_system::Fraction;

    #[test]
    fn test_hap_has_onset() {
        let whole = TimeSpan::new(Fraction::from_integer(0), Fraction::from_integer(1));
        let part = TimeSpan::new(Fraction::from_integer(0), Fraction::new(1, 2));

        let hap = Hap::new(Some(whole), part, 42);
        assert!(hap.has_onset());
    }

    #[test]
    fn test_hap_no_onset() {
        let whole = TimeSpan::new(Fraction::from_integer(0), Fraction::from_integer(1));
        let part = TimeSpan::new(Fraction::new(1, 2), Fraction::from_integer(1));

        let hap = Hap::new(Some(whole), part, 42);
        assert!(!hap.has_onset());
    }

    #[test]
    fn test_hap_continuous() {
        let part = TimeSpan::new(Fraction::from_integer(0), Fraction::from_integer(1));
        let hap = Hap::new(None, part, 42);

        assert!(!hap.has_onset());
        assert!(hap.whole.is_none());
    }

    #[test]
    fn test_context_combine() {
        let mut ctx1 = HapContext::with_span(SourceSpan::new(0, 5));
        ctx1.modifier_spans.push(SourceSpan::new(10, 15));

        let ctx2 = HapContext::with_span(SourceSpan::new(20, 25));

        let combined = ctx1.combine(&ctx2);

        let spans: Vec<_> = combined.get_all_span_tuples();
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0], (0, 5)); // Original source span
        assert_eq!(spans[1], (10, 15)); // Original modifier span
        assert_eq!(spans[2], (20, 25)); // Combined from ctx2
    }

    // ===== Fast-Path / DSP Tests =====

    #[test]
    fn test_hap_f64_accessors() {
        let whole = TimeSpan::new(Fraction::new(1, 4), Fraction::new(3, 4));
        let part = TimeSpan::new(Fraction::new(1, 4), Fraction::new(1, 2));
        let hap = Hap::new(Some(whole), part, 42);

        assert!((hap.part_begin_f64() - 0.25).abs() < 1e-10);
        assert!((hap.part_end_f64() - 0.5).abs() < 1e-10);
        assert!((hap.whole_begin_f64() - 0.25).abs() < 1e-10);
        assert!((hap.whole_end_f64() - 0.75).abs() < 1e-10);
    }

    #[test]
    fn test_hap_f64_continuous_fallback() {
        // Continuous hap (no whole span) should use part values for whole accessors
        let part = TimeSpan::new(Fraction::new(1, 3), Fraction::new(2, 3));
        let hap: Hap<i32> = Hap::new(None, part, 42);

        assert!((hap.whole_begin_f64() - hap.part_begin_f64()).abs() < 1e-10);
        assert!((hap.whole_end_f64() - hap.part_end_f64()).abs() < 1e-10);
    }

    #[test]
    fn test_hap_part_contains_f64() {
        let whole = TimeSpan::new(Fraction::from_integer(0), Fraction::from_integer(1));
        let part = TimeSpan::new(Fraction::new(1, 4), Fraction::new(3, 4));
        let hap = Hap::new(Some(whole), part, 42);

        // Inside part range
        assert!(hap.part_contains_f64(0.25)); // start (inclusive)
        assert!(hap.part_contains_f64(0.5));
        assert!(hap.part_contains_f64(0.7499));

        // Outside part range
        assert!(!hap.part_contains_f64(0.0));
        assert!(!hap.part_contains_f64(0.24));
        assert!(!hap.part_contains_f64(0.75)); // end (exclusive)
        assert!(!hap.part_contains_f64(1.0));
    }

    #[test]
    fn test_to_dsp_hap_discrete() {
        let whole = TimeSpan::new(Fraction::new(1, 4), Fraction::new(3, 4));
        let part = TimeSpan::new(Fraction::new(1, 4), Fraction::new(1, 2));
        let hap = Hap::new(Some(whole), part, 42);
        let dsp = hap.to_dsp_hap();

        assert!((dsp.part_begin - 0.25).abs() < 1e-10);
        assert!((dsp.part_end - 0.5).abs() < 1e-10);
        assert!((dsp.whole_begin - 0.25).abs() < 1e-10);
        assert!((dsp.whole_end - 0.75).abs() < 1e-10);
        assert_eq!(dsp.value, 42);
        assert!(dsp.has_whole);
    }

    #[test]
    fn test_to_dsp_hap_continuous() {
        let part = TimeSpan::new(Fraction::new(1, 3), Fraction::new(2, 3));
        let hap: Hap<i32> = Hap::new(None, part, 99);
        let dsp = hap.to_dsp_hap();

        // For continuous, whole should equal part
        assert!((dsp.whole_begin - dsp.part_begin).abs() < 1e-10);
        assert!((dsp.whole_end - dsp.part_end).abs() < 1e-10);
        assert_eq!(dsp.value, 99);
        assert!(!dsp.has_whole);
    }

    #[test]
    fn test_dsp_hap_part_contains() {
        let whole = TimeSpan::new(Fraction::from_integer(0), Fraction::from_integer(1));
        let part = TimeSpan::new(Fraction::new(1, 4), Fraction::new(3, 4));
        let dsp = Hap::new(Some(whole), part, 42).to_dsp_hap();

        assert!(dsp.part_contains(0.25));
        assert!(dsp.part_contains(0.5));
        assert!(!dsp.part_contains(0.24));
        assert!(!dsp.part_contains(0.75));
    }

    #[test]
    fn test_context_extra_spans_survive_combine() {
        // Simulate what happens after strip_modifier_spans:
        // pattern A has source_extra_spans from internal modifiers
        let mut ctx_a = HapContext::with_span(SourceSpan::new(0, 1));
        ctx_a.source_extra_spans.push(SourceSpan::new(5, 6)); // from *<4 6>

        // pattern B has its own source_extra_spans
        let mut ctx_b = HapContext::with_span(SourceSpan::new(10, 11));
        ctx_b.source_extra_spans.push(SourceSpan::new(15, 16));

        // combine simulates app_left merging
        let combined = ctx_a.combine(&ctx_b);

        // Pattern 0: source_span + source_extra_spans
        assert_eq!(combined.source_span.as_ref().unwrap().to_tuple(), (0, 1));
        assert_eq!(combined.source_extra_spans.len(), 1);
        assert_eq!(combined.source_extra_spans[0].to_tuple(), (5, 6));

        // Pattern 1: modifier_spans[0] = B's source, modifier_extra_spans[0] = B's extras
        assert_eq!(combined.modifier_spans.len(), 1);
        assert_eq!(combined.modifier_spans[0].to_tuple(), (10, 11));
        assert_eq!(combined.modifier_extra_spans.len(), 1);
        assert_eq!(combined.modifier_extra_spans[0].len(), 1);
        assert_eq!(combined.modifier_extra_spans[0][0].to_tuple(), (15, 16));
    }

    #[test]
    fn test_dsp_hap_preserves_context() {
        let whole = TimeSpan::new(Fraction::from_integer(0), Fraction::from_integer(1));
        let part = whole.clone();

        let mut ctx = HapContext::with_span(SourceSpan::new(10, 20));
        ctx.modifier_spans.push(SourceSpan::new(30, 40));

        let hap = Hap {
            whole: Some(whole),
            part,
            value: 42,
            context: ctx,
        };
        let dsp = hap.to_dsp_hap();

        let spans = dsp.get_active_spans();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0], (10, 20));
        assert_eq!(spans[1], (30, 40));
    }
}
