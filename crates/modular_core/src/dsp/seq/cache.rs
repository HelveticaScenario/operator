//! Shared cycle-cache primitives for the pattern-based sequencer modules.
//!
//! `Seq` bakes every cycle of its ribbon loop window `[offset, offset+length)`
//! once at parse time on the main thread, then loops that window forever — the
//! audio thread never re-evaluates the pattern.
//!
//! Each cycle's data lives in a [`CycleStorage`] holding two parallel
//! pre-sized `Vec`s: one of scalar haps `H`, one of span entries `S`.
//! Voices reference a hap by `(cached_cycle, hap_index)` plus
//! `span_offset/span_len` into the span arena, so voice state contains no
//! `Arc` or reference into storage.

use crate::pattern_system::{ArenaHap, Pattern};

/// Initial per-slot `haps` Vec capacity used when baking a cycle. A floor so
/// the bake-time `push` loop rarely reallocates; the main-thread bake may grow
/// it past this for dense cycles.
pub(crate) const MIN_HAPS_CAP_HINT: usize = 16;

/// Initial span_arena sizing per cached cycle (`MIN_HAPS_CAP_HINT * this`).
pub(crate) const SPANS_RESERVE_PER_HAP: usize = 4;

/// Flat span entry tagged with the source pattern it belongs to. Used by
/// `Seq`'s chained `$p.s` (`SpPattern`) payloads, which need per-source
/// highlighting when one runtime hap was produced from multiple input
/// pattern strings.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlatSpan {
    pub pattern_idx: u32,
    pub start: u32,
    pub end: u32,
}

/// Per-cycle storage: parallel hap + span arena. Pre-allocated so the
/// audio thread can `push` into both vectors alloc-free as long as the
/// pre-sized capacity holds.
#[derive(Clone, Debug)]
pub(crate) struct CycleStorage<H, S> {
    pub haps: Vec<H>,
    pub span_arena: Vec<S>,
}

impl<H, S> Default for CycleStorage<H, S> {
    fn default() -> Self {
        Self {
            haps: Vec::new(),
            span_arena: Vec::new(),
        }
    }
}

impl<H, S> CycleStorage<H, S> {
    pub(crate) fn with_capacity(hap_cap: usize, arena_cap: usize) -> Self {
        Self {
            haps: Vec::with_capacity(hap_cap),
            span_arena: Vec::with_capacity(arena_cap),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.haps.clear();
        self.span_arena.clear();
    }
}

/// Look up `cycle`'s storage in the baked ribbon window. `cached` holds the
/// haps for cycles `[base, base+cached.len())` where `base = floor(offset)`; a
/// `cycle` below `base` or past the end of the window has no storage.
pub(crate) fn get_cycle_storage<H, S>(
    cycle: i64,
    base: i64,
    cached: &[CycleStorage<H, S>],
) -> Option<&CycleStorage<H, S>> {
    if cycle < base {
        None
    } else {
        cached.get((cycle - base) as usize)
    }
}

/// Fill `storage` with the pattern's haps for `cycle`. Uses the supplied
/// bumpalo arena for intermediate allocations so the heap is untouched on
/// the common path. The caller supplies a `convert` closure that pushes
/// one `H` (and any number of `S` spans) per `ArenaHap<T>`.
pub(crate) fn populate_cycle_storage<T, H, S, F>(
    pattern: &Pattern<T>,
    cycle: i64,
    bump: &mut bumpalo::Bump,
    storage: &mut CycleStorage<H, S>,
    mut convert: F,
) where
    T: Clone + Send + Sync + 'static,
    F: FnMut(&ArenaHap<'_, T>, &mut Vec<H>, &mut Vec<S>),
{
    bump.reset();
    let mut arena_haps: bumpalo::collections::Vec<'_, ArenaHap<'_, T>> =
        bumpalo::collections::Vec::new_in(bump);
    pattern.query_cycle_all_into(cycle, bump, &mut arena_haps);
    storage.reset();
    for hap in &arena_haps {
        convert(hap, &mut storage.haps, &mut storage.span_arena);
    }
}
