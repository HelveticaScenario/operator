//! Per-module audio-thread profiler.
//!
//! Stack-based exclusive-time accounting hooked into the proc-macro
//! generated `ensure_processed_to`. Self-time = time inside a module's own
//! `update()` loop, excluding time recursively spent inside other modules'
//! `ensure_processed_to` (cable pulls). Total-time = full elapsed
//! including recursive pulls. Total minus self is upstream pull cost.
//!
//! Hot-path design:
//!
//! - Cable-pull cache-hit short-circuit (`index >= target`) is *not*
//!   instrumented. That branch fires every `Signal::Cable::get_value()`
//!   resolution inside an `update()` and dominates call counts; touching
//!   the profiler there would dwarf the work being measured.
//! - When profiling is off, the only cost is one `Cell<bool>` TLS read in
//!   `push_frame` (early-return; `pop_frame` is not called by the
//!   generated wrapper when `push_frame` returns `false`).
//! - Sampling: `set_sample_rate(N)` profiles 1-of-N audio callbacks.
//!   Reduces per-callback cost linearly while still giving the UI a
//!   representative average over its 1 Hz poll window.
//!
//! # Allocation-free audio-thread contract
//!
//! The audio thread never allocates inside the profiler:
//!
//! - The TLS `records` map and the shared cross-thread map are
//!   pre-allocated on the main thread at patch-swap time, with one entry
//!   per desired module id. [`swap_records`] and [`try_swap_shared`] swap
//!   them in via `mem::replace`; the old maps are returned and routed
//!   through the command-queue garbage path for drop on the main thread.
//! - [`pop_frame`] writes through `get_mut` only. A pop with an id
//!   absent from the records map is dropped silently — possible during
//!   the window between `set_enabled(true)` and the first subsequent
//!   patch swap that seeds the map.
//! - [`flush_into`] iterates `iter_mut` and merges through `get_mut`
//!   into the shared map; locals are zeroed in place (mode preserved).
//!   The shared map must contain every key the local map contains;
//!   main-thread seeding guarantees this.
//! - [`drain_collection`] (main-thread reader) iterates + clones keys +
//!   zeros values in place. The audio thread keeps its bucket capacity
//!   across UI drains.
//! - The call stack is a fixed inline `[MaybeUninit<Frame>; STACK_CAPACITY]`
//!   array, never heap-backed, so the lazy thread-local initializer that
//!   first touches it on the audio thread allocates nothing. Pushes beyond
//!   the cap return `false`, so the inline stack never overflows.

use crate::types::ProcessingMode;
use parking_lot::Mutex;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

/// Maximum profiled call-stack depth. Real graphs nest a few levels
/// (root → mix → osc/fx); 32 leaves headroom for pathological patches.
/// Pushes past this are dropped so the fixed inline stack never overflows.
pub const STACK_CAPACITY: usize = 32;

/// Snapshot of one module's cumulative stats since the last UI drain.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModuleProfileAccum {
    pub self_ns: u64,
    pub total_ns: u64,
    pub ensure_calls_did_work: u32,
    pub samples_processed: u32,
    pub mode: ProcessingMode,
}

impl ModuleProfileAccum {
    fn add_assign(&mut self, other: &ModuleProfileAccum) {
        self.self_ns = self.self_ns.saturating_add(other.self_ns);
        self.total_ns = self.total_ns.saturating_add(other.total_ns);
        self.ensure_calls_did_work = self
            .ensure_calls_did_work
            .saturating_add(other.ensure_calls_did_work);
        self.samples_processed = self.samples_processed.saturating_add(other.samples_processed);
        // Last-write-wins on mode. The same id can legitimately switch
        // mode across patch swaps when graph_analysis reclassifies its
        // SCC membership.
        self.mode = other.mode;
    }

    fn zero_counters(&mut self) {
        self.self_ns = 0;
        self.total_ns = 0;
        self.ensure_calls_did_work = 0;
        self.samples_processed = 0;
    }
}

pub type ModuleProfileCollection = Arc<Mutex<HashMap<String, ModuleProfileAccum>>>;

pub fn new_collection() -> ModuleProfileCollection {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Build a pre-seeded records map sized for `ids`, with a default
/// `ModuleProfileAccum` per id. Allocates on the caller thread (always
/// the main thread). The result is handed to the audio thread via the
/// command queue and swapped into TLS / the shared map via
/// [`swap_records`] / [`try_swap_shared`].
pub fn build_seed<I, S>(ids: I) -> HashMap<String, ModuleProfileAccum>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let iter = ids.into_iter();
    let (lo, hi) = iter.size_hint();
    let cap = hi.unwrap_or(lo);
    let mut map = HashMap::with_capacity(cap);
    for id in iter {
        map.insert(id.into(), ModuleProfileAccum::default());
    }
    map
}

/// Global enable flag flipped from the main thread. Audio thread mirrors
/// this into TLS once per callback via `refresh_enabled`.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Sample 1-of-N audio callbacks when enabled. 1 = every callback.
static SAMPLE_RATE: AtomicU32 = AtomicU32::new(1);

/// Monotonic audio-callback counter, advanced by `refresh_enabled`.
static CALLBACK_COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn set_sample_rate(rate: u32) {
    SAMPLE_RATE.store(rate.max(1), Ordering::Relaxed);
}

struct Frame {
    /// Pointer + len back into the wrapper's owned `id: String`.
    ///
    /// # Safety contract
    ///
    /// Caller (`push_frame`) borrows from `&self.id` on a wrapper that is
    /// guaranteed alive across the matching `pop_frame` call because the
    /// wrapper is the one executing `ensure_processed_to`. The wrapper's
    /// `id: String` is **write-once** — set in the generated constructor
    /// and never reassigned, mutated, or replaced. If a future change adds
    /// an id setter, reallocates the `String` (`push_str`, `clear`, etc.),
    /// or hands out `&mut self.id`, this becomes UB.
    id_ptr: *const u8,
    id_len: usize,
    mode: ProcessingMode,
    total_start: Instant,
    last_resume: Instant,
    self_accum_ns: u64,
}

struct Profiler {
    /// Fixed inline call-stack. Never heap-backed, so the lazy thread-local
    /// initializer that first touches `PROFILER` on the audio thread does no
    /// allocation. `depth` is the live length; slots `[0, depth)` hold
    /// initialized frames. `Frame` is plain data with no `Drop`, so
    /// unwinding the stack is just resetting `depth`.
    stack: [MaybeUninit<Frame>; STACK_CAPACITY],
    depth: usize,
    records: HashMap<String, ModuleProfileAccum>,
}

impl Profiler {
    fn new() -> Self {
        Self {
            stack: [const { MaybeUninit::uninit() }; STACK_CAPACITY],
            depth: 0,
            // `HashMap::new` does not allocate until the first insert, so
            // this initializer stays allocation-free; the records map is
            // seeded from the main thread via `swap_records`.
            records: HashMap::new(),
        }
    }
}

thread_local! {
    /// Cheap on/off check on the hot path. `Cell::get` is a TLS read + load;
    /// no `RefCell` borrow.
    static ENABLED_TLS: Cell<bool> = const { Cell::new(false) };
    static PROFILER: RefCell<Profiler> = RefCell::new(Profiler::new());
}

/// Refresh the thread-local enable mirror. Called once per audio callback
/// at the top, before any module processing. Honours the sample-rate knob.
pub fn refresh_enabled() {
    let global_on = ENABLED.load(Ordering::Relaxed);
    let on = if global_on {
        let rate = SAMPLE_RATE.load(Ordering::Relaxed).max(1);
        let count = CALLBACK_COUNTER.fetch_add(1, Ordering::Relaxed);
        count.is_multiple_of(rate)
    } else {
        false
    };
    ENABLED_TLS.with(|c| c.set(on));
    // Stack residue clearance for the unwind case: a panic inside
    // `inner.update` skips the wrapper's matching `pop_frame`, leaving
    // stale frames and a stale `last_resume` on the parent. Resetting
    // `depth` to 0 gives the next active window a clean start; `Frame` has
    // no `Drop`, so the abandoned inline slots are simply overwritten by
    // later pushes. No-op when push/pop are balanced.
    PROFILER.with(|p| {
        p.borrow_mut().depth = 0;
    });
}

/// Called on entry to a wrapper's `ensure_processed_to`, after cache-hit
/// and re-entry guards. Returns `true` if profiling was active for this
/// frame; the generated wrapper conditions its matching `pop_frame` call
/// on the return value so disabled callbacks pay only one TLS bool read.
#[inline]
pub fn push_frame(id: &str, mode: ProcessingMode) -> bool {
    if !ENABLED_TLS.with(|c| c.get()) {
        return false;
    }
    let now = Instant::now();
    PROFILER.with(|p| {
        let mut prof = p.borrow_mut();
        // Bounds check: pushes past `STACK_CAPACITY` are dropped so the
        // inline stack never overflows. Caller treats `false` as "frame
        // not profiled" and skips its matching `pop_frame`.
        if prof.depth >= STACK_CAPACITY {
            return false;
        }
        if prof.depth > 0 {
            let parent_idx = prof.depth - 1;
            // SAFETY: `depth > 0` so slot `depth - 1` holds a live frame
            // (strict LIFO; every slot below `depth` was written by a
            // matching `push_frame` and not yet read by `pop_frame`).
            let parent = unsafe { prof.stack[parent_idx].assume_init_mut() };
            parent.self_accum_ns = parent
                .self_accum_ns
                .saturating_add((now - parent.last_resume).as_nanos() as u64);
        }
        let depth = prof.depth;
        prof.stack[depth].write(Frame {
            id_ptr: id.as_ptr(),
            id_len: id.len(),
            mode,
            total_start: now,
            last_resume: now,
            self_accum_ns: 0,
        });
        prof.depth += 1;
        true
    })
}

/// Called on exit from a wrapper's `ensure_processed_to`, only when the
/// matching `push_frame` returned `true`.
#[inline]
pub fn pop_frame(samples_processed: u32) {
    let now = Instant::now();
    PROFILER.with(|p| {
        let mut prof = p.borrow_mut();
        if prof.depth == 0 {
            return;
        }
        prof.depth -= 1;
        let depth = prof.depth;
        // SAFETY: slot `depth` was initialized by the matching `push_frame`
        // and not yet read (strict LIFO). `Frame` has no `Drop`, so moving
        // it out leaves an inert slot to be overwritten by the next push.
        let frame = unsafe { prof.stack[depth].assume_init_read() };
        let self_ns = frame
            .self_accum_ns
            .saturating_add((now - frame.last_resume).as_nanos() as u64);
        let total_ns = (now - frame.total_start).as_nanos() as u64;

        // SAFETY: pointer came from a `&str` borrowed from the wrapper's
        // owned `id: String`, alive for the strict-LIFO span of this frame.
        let id = unsafe {
            let slice = std::slice::from_raw_parts(frame.id_ptr, frame.id_len);
            std::str::from_utf8_unchecked(slice)
        };

        // get_mut only. Missing entries drop the sample. The records
        // map is seeded from the main thread via `swap_records` at each
        // patch swap; ids absent from the seed (e.g. reserved ids the
        // caller omitted) record nothing.
        if let Some(rec) = prof.records.get_mut(id) {
            rec.self_ns = rec.self_ns.saturating_add(self_ns);
            rec.total_ns = rec.total_ns.saturating_add(total_ns);
            rec.ensure_calls_did_work = rec.ensure_calls_did_work.saturating_add(1);
            rec.samples_processed = rec.samples_processed.saturating_add(samples_processed);
            rec.mode = frame.mode;
        }

        if prof.depth > 0 {
            let parent_idx = prof.depth - 1;
            // SAFETY: `depth > 0` so slot `depth - 1` holds a live frame.
            let parent = unsafe { prof.stack[parent_idx].assume_init_mut() };
            parent.last_resume = now;
        }
    });
}

/// Drain the audio-thread-local records into the shared cross-thread
/// map. `try_lock` — contention drops the merge for this callback.
///
/// Allocation-free: iterates `iter_mut` over the local map, merges each
/// entry into the matching pre-seeded slot in `dst` via `get_mut`, and
/// zeros the local counters in place. Local entries whose key is absent
/// from `dst` are skipped; the main-thread seed populates both maps with
/// the same key set.
pub fn flush_into(dst: &ModuleProfileCollection) {
    PROFILER.with(|p| {
        let mut prof = p.borrow_mut();
        if prof.records.is_empty() {
            return;
        }
        let Some(mut guard) = dst.try_lock() else {
            return;
        };
        for (id, acc) in prof.records.iter_mut() {
            if acc.ensure_calls_did_work == 0 {
                continue;
            }
            if let Some(existing) = guard.get_mut(id) {
                existing.add_assign(acc);
            }
            acc.zero_counters();
        }
    });
}

/// Drain and return the shared cross-thread snapshot. Caller (UI) gets
/// the accumulated stats since its last drain.
///
/// Iterates + clones keys + zeros values in place, preserving the
/// shared map's bucket allocation for the audio thread's `flush_into`.
/// Runs on the main thread and blocks on `lock()`, holding the lock across
/// the per-key clones. The audio-thread writers ([`flush_into`] and
/// [`try_swap_shared`]) both use `try_lock` and skip/defer on contention,
/// so this blocking acquisition never stalls the audio callback.
pub fn drain_collection(src: &ModuleProfileCollection) -> Vec<(String, ModuleProfileAccum)> {
    let mut guard = src.lock();
    let mut out = Vec::with_capacity(guard.len());
    for (id, acc) in guard.iter_mut() {
        if acc.ensure_calls_did_work == 0 {
            // Reset mode-only entries still get zeroed; keep them out
            // of the snapshot so the UI doesn't see no-op rows.
            continue;
        }
        out.push((id.clone(), *acc));
        acc.zero_counters();
    }
    out
}

/// Replace the audio thread's TLS records map with `new`. Returns the
/// previous map for routing through the garbage queue. The swap is two
/// pointer writes; allocation lives in `new` (built on the main thread)
/// and in the dropped return value (also dropped on the main thread).
/// Subsequent `pop_frame` calls neither allocate nor rehash as long as
/// every active wrapper's id is present in `new`.
pub fn swap_records(
    new: HashMap<String, ModuleProfileAccum>,
) -> HashMap<String, ModuleProfileAccum> {
    PROFILER.with(|p| {
        let mut prof = p.borrow_mut();
        std::mem::replace(&mut prof.records, new)
    })
}

/// Try to replace the shared cross-thread map's contents with `new`.
///
/// Non-blocking: this runs on the audio thread, so it must never wait on
/// the shared `Mutex`. On success returns `Ok(previous_contents)` for
/// routing through the garbage queue. On lock contention with the
/// main-thread [`drain_collection`] it returns `Err(new)` unchanged, so the
/// caller can retry on a later callback instead of blocking the audio
/// thread.
pub fn try_swap_shared(
    dst: &ModuleProfileCollection,
    new: HashMap<String, ModuleProfileAccum>,
) -> Result<HashMap<String, ModuleProfileAccum>, HashMap<String, ModuleProfileAccum>> {
    match dst.try_lock() {
        Some(mut guard) => Ok(std::mem::replace(&mut *guard, new)),
        None => Err(new),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Profiler state lives in globals (ENABLED, SAMPLE_RATE, CALLBACK_COUNTER)
    // and a thread_local. Cargo runs tests on multiple threads by default,
    // so we serialize through this lock to keep them deterministic.
    static TEST_LOCK: Mutex<()> = parking_lot::const_mutex(());

    fn reset() {
        set_enabled(false);
        set_sample_rate(1);
        CALLBACK_COUNTER.store(0, Ordering::Relaxed);
        refresh_enabled();
        PROFILER.with(|p| {
            let mut prof = p.borrow_mut();
            prof.depth = 0;
            prof.records.clear();
        });
    }

    fn seed(ids: &[&str]) {
        let _ = swap_records(build_seed(ids.iter().map(|s| s.to_string())));
    }

    #[test]
    fn push_pop_accumulates_self_time() {
        let _g = TEST_LOCK.lock();
        reset();
        seed(&["a"]);
        set_enabled(true);
        refresh_enabled();

        let active = push_frame("a", ProcessingMode::Sample);
        assert!(active);
        std::thread::sleep(std::time::Duration::from_millis(2));
        pop_frame(10);

        let collection = new_collection();
        // Seed shared map too so flush_into has a slot to write to.
        let _ = try_swap_shared(&collection, build_seed(["a".to_string()]));
        flush_into(&collection);

        let snap = drain_collection(&collection);
        let (id, acc) = snap.into_iter().next().unwrap();
        assert_eq!(id, "a");
        assert!(acc.self_ns >= 1_000_000, "self_ns: {}", acc.self_ns);
        assert!(acc.total_ns >= acc.self_ns);
        assert_eq!(acc.ensure_calls_did_work, 1);
        assert_eq!(acc.samples_processed, 10);
        assert_eq!(acc.mode, ProcessingMode::Sample);
    }

    #[test]
    fn nested_frames_split_self_and_total() {
        let _g = TEST_LOCK.lock();
        reset();
        seed(&["parent", "child"]);
        set_enabled(true);
        refresh_enabled();

        push_frame("parent", ProcessingMode::Block);
        std::thread::sleep(std::time::Duration::from_millis(2));
        push_frame("child", ProcessingMode::Block);
        std::thread::sleep(std::time::Duration::from_millis(4));
        pop_frame(4);
        std::thread::sleep(std::time::Duration::from_millis(2));
        pop_frame(4);

        let collection = new_collection();
        let _ = try_swap_shared(
            &collection,
            build_seed(["parent".to_string(), "child".to_string()]),
        );
        flush_into(&collection);

        let snap: HashMap<String, ModuleProfileAccum> = drain_collection(&collection)
            .into_iter()
            .collect();
        let parent = snap.get("parent").unwrap();
        let child = snap.get("child").unwrap();

        assert!(child.total_ns >= 3_000_000);
        assert!(parent.self_ns >= 3_000_000);
        assert!(parent.total_ns >= child.total_ns);
    }

    #[test]
    fn disabled_push_returns_false() {
        let _g = TEST_LOCK.lock();
        reset();
        assert!(!push_frame("x", ProcessingMode::Block));

        let collection = new_collection();
        flush_into(&collection);
        assert!(drain_collection(&collection).is_empty());
    }

    #[test]
    fn sampling_skips_callbacks() {
        let _g = TEST_LOCK.lock();
        reset();
        seed(&["m"]);
        set_enabled(true);
        set_sample_rate(3);

        let mut active_count = 0;
        for _ in 0..9 {
            refresh_enabled();
            if push_frame("m", ProcessingMode::Block) {
                active_count += 1;
                pop_frame(1);
            }
        }
        // 9 callbacks at rate 3 = 3 active (counters 0, 3, 6).
        assert_eq!(active_count, 3);
    }

    #[test]
    fn unseeded_id_is_dropped_not_allocated() {
        let _g = TEST_LOCK.lock();
        reset();
        // Seed only "a"; push under "b" should not insert into records.
        seed(&["a"]);
        set_enabled(true);
        refresh_enabled();
        assert!(push_frame("b", ProcessingMode::Block));
        pop_frame(1);

        PROFILER.with(|p| {
            let prof = p.borrow();
            assert!(prof.records.contains_key("a"));
            assert!(!prof.records.contains_key("b"));
        });
    }

    #[test]
    fn stack_capacity_bounds_pushes() {
        let _g = TEST_LOCK.lock();
        reset();
        // Build a seed covering every id we'll push so pop_frame has slots.
        let ids: Vec<String> = (0..STACK_CAPACITY + 4).map(|i| format!("m{i}")).collect();
        let _ = swap_records(build_seed(ids.iter().cloned()));
        set_enabled(true);
        refresh_enabled();

        let mut pushed = 0usize;
        for id in &ids {
            if push_frame(id, ProcessingMode::Block) {
                pushed += 1;
            }
        }
        assert_eq!(pushed, STACK_CAPACITY);

        // The inline stack is full at the cap; pushes past it were dropped
        // (`push_frame` returned false) rather than overflowing.
        PROFILER.with(|p| {
            assert_eq!(p.borrow().depth, STACK_CAPACITY);
        });

        // Drain the stack: only the pushes that succeeded matter.
        for _ in 0..pushed {
            pop_frame(1);
        }
    }
}
