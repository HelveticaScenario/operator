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

use crate::types::ProcessingMode;
use parking_lot::Mutex;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

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
    fn merge(&mut self, other: &ModuleProfileAccum) {
        self.self_ns = self.self_ns.saturating_add(other.self_ns);
        self.total_ns = self.total_ns.saturating_add(other.total_ns);
        self.ensure_calls_did_work = self
            .ensure_calls_did_work
            .saturating_add(other.ensure_calls_did_work);
        self.samples_processed = self.samples_processed.saturating_add(other.samples_processed);
        // Mode is write-once per module instance (assigned at construction
        // by graph_analysis). Merges across drain windows must observe the
        // same mode for the same id.
        debug_assert_eq!(self.mode, other.mode);
    }
}

pub type ModuleProfileCollection = Arc<Mutex<HashMap<String, ModuleProfileAccum>>>;

pub fn new_collection() -> ModuleProfileCollection {
    Arc::new(Mutex::new(HashMap::new()))
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
    stack: Vec<Frame>,
    records: HashMap<String, ModuleProfileAccum>,
}

impl Profiler {
    fn new() -> Self {
        Self {
            stack: Vec::with_capacity(32),
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
    // Defensive: a panic inside `inner.update` unwinds past the wrapper's
    // matching `pop_frame`, leaving stale frames on the stack and a stale
    // `last_resume` on the parent. The audio panic guard catches the
    // unwind; this clears the residue on the next callback so the next
    // active window starts clean. No-op when push/pop balanced.
    PROFILER.with(|p| {
        let mut prof = p.borrow_mut();
        if !prof.stack.is_empty() {
            prof.stack.clear();
        }
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
        if let Some(parent) = prof.stack.last_mut() {
            parent.self_accum_ns = parent
                .self_accum_ns
                .saturating_add((now - parent.last_resume).as_nanos() as u64);
        }
        prof.stack.push(Frame {
            id_ptr: id.as_ptr(),
            id_len: id.len(),
            mode,
            total_start: now,
            last_resume: now,
            self_accum_ns: 0,
        });
    });
    true
}

/// Called on exit from a wrapper's `ensure_processed_to`, only when the
/// matching `push_frame` returned `true`.
#[inline]
pub fn pop_frame(samples_processed: u32) {
    let now = Instant::now();
    PROFILER.with(|p| {
        let mut prof = p.borrow_mut();
        let Some(frame) = prof.stack.pop() else {
            return;
        };
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

        if let Some(rec) = prof.records.get_mut(id) {
            rec.self_ns = rec.self_ns.saturating_add(self_ns);
            rec.total_ns = rec.total_ns.saturating_add(total_ns);
            rec.ensure_calls_did_work = rec.ensure_calls_did_work.saturating_add(1);
            rec.samples_processed = rec.samples_processed.saturating_add(samples_processed);
            rec.mode = frame.mode;
        } else {
            prof.records.insert(
                id.to_string(),
                ModuleProfileAccum {
                    self_ns,
                    total_ns,
                    ensure_calls_did_work: 1,
                    samples_processed,
                    mode: frame.mode,
                },
            );
        }

        if let Some(parent) = prof.stack.last_mut() {
            parent.last_resume = now;
        }
    });
}

/// Drain the audio-thread-local records into the shared cross-thread map.
/// Uses `try_lock` — drops the merge on contention rather than blocking.
pub fn flush_into(dst: &ModuleProfileCollection) {
    PROFILER.with(|p| {
        let mut prof = p.borrow_mut();
        if prof.records.is_empty() {
            return;
        }
        let Some(mut guard) = dst.try_lock() else {
            return;
        };
        for (id, acc) in prof.records.drain() {
            guard
                .entry(id)
                .and_modify(|existing| existing.merge(&acc))
                .or_insert(acc);
        }
    });
}

/// Drain and return the shared cross-thread snapshot. Caller (UI) gets
/// the accumulated stats since its last drain.
///
/// Uses the blocking `lock()` rather than `try_lock`. The audio thread
/// holds the lock only for the duration of a `mem::take`-equivalent drain
/// in `flush_into`, so contention is bounded by a single short critical
/// section. A blocking acquire here is preferable to a try-and-drop because
/// the UI poll cadence (1 Hz) cannot afford to silently miss a window.
pub fn drain_collection(src: &ModuleProfileCollection) -> Vec<(String, ModuleProfileAccum)> {
    let mut guard = src.lock();
    let map = std::mem::take(&mut *guard);
    map.into_iter().collect()
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
            prof.stack.clear();
            prof.records.clear();
        });
    }

    #[test]
    fn push_pop_accumulates_self_time() {
        let _g = TEST_LOCK.lock();
        reset();
        set_enabled(true);
        refresh_enabled();

        let active = push_frame("a", ProcessingMode::Sample);
        assert!(active);
        std::thread::sleep(std::time::Duration::from_millis(2));
        pop_frame(10);

        let collection = new_collection();
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
}
