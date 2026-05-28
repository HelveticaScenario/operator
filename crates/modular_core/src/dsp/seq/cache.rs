//! Shared cycle-cache primitives for the pattern-based sequencer modules.
//!
//! Both `Seq` and `IntervalSeq` cache pre-computed hap data in two tiers:
//! the param cache (cycles `0..PARAM_CACHE_CYCLES`, built at parse time on
//! the main thread) and the audio-thread module cache (cycles past the
//! param cache horizon, up to `MAX_MODULE_CYCLES` slots, populated lazily).
//!
//! Each cycle's data lives in a [`CycleStorage`] holding two parallel
//! pre-sized `Vec`s: one of scalar haps `H`, one of span entries `S`.
//! Voices reference a hap by `(cached_cycle, hap_index)` plus
//! `span_offset/span_len` into the span arena, so voice state contains no
//! `Arc` or reference into storage.

use crate::pattern_system::{ArenaHap, Pattern};

/// Number of cycles pre-computed at parse time on the main thread. Cycles
/// beyond this fall through to the per-module audio-thread cache.
pub(crate) const PARAM_CACHE_CYCLES: usize = 1024;

/// Maximum number of cycles cached by the audio thread past the param
/// cache. Slots are pre-allocated, so this is a memory/latency tradeoff.
pub(crate) const MAX_MODULE_CYCLES: usize = 64;

/// Heuristic floor for `max_haps_per_cycle`. Keeps the per-slot Vec from
/// reallocating on an occasional cycle whose hap count exceeds the
/// param-time observed maximum.
pub(crate) const MIN_HAPS_CAP_HINT: usize = 16;

/// Heuristic floor for `max_spans_per_cycle`.
pub(crate) const MIN_SPANS_CAP_HINT: usize = 32;

/// Initial span_arena sizing per cached cycle.
pub(crate) const SPANS_RESERVE_PER_HAP: usize = 4;

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

/// Pre-allocate `module_cache` to [`MAX_MODULE_CYCLES`] slots, each sized
/// to the supplied capacity hints. Call from the main thread on patch
/// update so the audio thread never reallocates the cache.
pub(crate) fn rebuild_module_cache<H, S>(
    module_cache: &mut Vec<CycleStorage<H, S>>,
    module_cache_populated: &mut Vec<bool>,
    hap_cap: usize,
    span_cap: usize,
) {
    module_cache.clear();
    module_cache.reserve_exact(MAX_MODULE_CYCLES);
    for _ in 0..MAX_MODULE_CYCLES {
        module_cache.push(CycleStorage::with_capacity(hap_cap, span_cap));
    }
    module_cache_populated.clear();
    module_cache_populated.resize(MAX_MODULE_CYCLES, false);
}

/// Clear every slot's content but keep their allocated capacities. Reset
/// the populated flags. Voices keep their cached scalar copy so any
/// sounding note can still be released by its `whole_end` afterwards.
pub(crate) fn invalidate_module_cache<H, S>(
    module_cache: &mut [CycleStorage<H, S>],
    module_cache_populated: &mut [bool],
) {
    for slot in module_cache.iter_mut() {
        slot.reset();
    }
    for p in module_cache_populated.iter_mut() {
        *p = false;
    }
}

/// Look up `cycle`'s storage. Cycles in `0..PARAM_CACHE_CYCLES` come from
/// the param cache; later cycles come from the audio-thread module cache
/// (if a slot has been populated for that cycle).
pub(crate) fn get_cycle_storage<'a, H, S>(
    cycle: i64,
    param_cache: &'a [CycleStorage<H, S>],
    module_cache: &'a [CycleStorage<H, S>],
    module_cache_populated: &[bool],
) -> Option<&'a CycleStorage<H, S>> {
    if cycle < PARAM_CACHE_CYCLES as i64 {
        param_cache.get(cycle as usize)
    } else {
        let module_idx = (cycle - PARAM_CACHE_CYCLES as i64) as usize;
        if module_idx < module_cache.len() && module_cache_populated[module_idx] {
            Some(&module_cache[module_idx])
        } else {
            None
        }
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
