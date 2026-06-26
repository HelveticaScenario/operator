//! Dev-only runtime detector for heap allocation/deallocation on the audio thread.
//!
//! Gated behind the `alloc-detector` Cargo feature (off by default). `yarn start`
//! never compiles it in — the default build installs no global allocator and is
//! byte-identical. `yarn start:alloc` or `yarn build-native-alloc` builds with `--features=alloc-detector`,
//! which installs [`AudioAllocDetector`] as the process `#[global_allocator]`.
//!
//! How it works: every allocation/deallocation in the native addon runs the real
//! `std::alloc::System` call **first**, then — only while the current thread is
//! inside an [`AudioThreadScope`] (entered at the top of the cpal callback) —
//! records a fixed-size `Copy` event into a pre-allocated lock-free SPSC ring. A
//! dedicated background thread drains the ring and writes rate-limited warnings to
//! stderr, naming the offending module via the profiler's currently-running frame.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

/// Capacity of the SPSC event ring. Sized to cover several logger drain intervals
/// of a module allocating once per sample before overflow — and overflow is
/// counted (`DROPPED_EVENTS`), not fatal.
const RING_CAPACITY: usize = 8192;

/// Max module-id bytes carried per event. Longer ids are truncated for the
/// diagnostic line; the alloc count and size stay accurate. Must match the buffer
/// size of [`modular_core::profiling::current_module_id_into`].
const MODULE_ID_CAP: usize = 64;

/// A fixed-size, `Copy`, `Drop`-free allocation event — safe to push to the ring
/// and to drop on the audio thread (dropping it frees nothing).
#[derive(Clone, Copy)]
struct AllocEvent {
    module_id: [u8; MODULE_ID_CAP],
    module_id_len: u8,
    size: u32,
    is_dealloc: bool,
}

thread_local! {
    /// Set while the current thread is inside the cpal audio callback. This is the
    /// hot fast-path: a single `Cell::get` that short-circuits recording on every
    /// non-audio thread. `const` init keeps the TLS access allocation-free.
    static ON_AUDIO: Cell<bool> = const { Cell::new(false) };
    /// Re-entrancy backstop: set while `record_event` runs, so an allocation made
    /// by the recording path itself (there should be none) cannot recurse.
    static IN_DETECTOR: Cell<bool> = const { Cell::new(false) };
}

/// Running totals of audio-thread allocator traffic, plus events dropped because
/// the ring was full. These survive ring overflow, so the logger can always report
/// true magnitude even when individual events were dropped.
static AUDIO_ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static AUDIO_DEALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static AUDIO_ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);

/// Producer half of the ring. `rtrb`'s `Producer` is single-producer and not
/// `Sync`, so all access must be exclusive. Exclusivity is enforced at runtime by
/// `PRODUCER_BUSY` (below), *not* by assuming a single audio thread: during a
/// device or sample-rate switch the new cpal output stream starts before the old
/// one is dropped, so two callback threads (both with `ON_AUDIO` set) can be
/// briefly live at once. Wrapped in `UnsafeCell` so it can live in a `static`.
struct ProducerCell(UnsafeCell<Option<Producer<AllocEvent>>>);

// SAFETY: the inner `Producer` is written once during `init_detector` (before any
// `ON_AUDIO` is set) and thereafter accessed only by the thread that wins the
// `PRODUCER_BUSY` claim, so there is exactly one accessor at a time. The
// Acquire/Release pairing on that claim publishes one accessor's writes to the
// next, keeping the producer's non-atomic cached state coherent. No two threads
// ever touch it concurrently, so the lack of built-in synchronization is sound.
unsafe impl Sync for ProducerCell {}

static RING_PRODUCER: ProducerCell = ProducerCell(UnsafeCell::new(None));

/// Single-claim token guarding `RING_PRODUCER`. A thread must win this CAS before
/// touching the producer; a thread that loses (another audio thread is mid-push
/// during a stream swap) drops its event rather than racing the SPSC ring. Lock-
/// free and non-blocking — never spins or waits on the audio thread.
static PRODUCER_BUSY: AtomicBool = AtomicBool::new(false);

/// True once the ring is built and the producer published. Until then the audio
/// thread only bumps the totals (no ring to push to yet).
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Guards one-time construction of the ring + logger thread.
static INIT: Once = Once::new();

/// RAII guard marking the current thread as the audio thread for the detector's
/// duration. Entered at the top of the cpal callback; dropped when the callback
/// returns (including on a caught unwind, since it is bound outside `catch_unwind`).
pub struct AudioThreadScope;

impl AudioThreadScope {
    /// Mark the current thread as the audio thread. Lazily builds the ring and
    /// spawns the logger thread on first use (a one-off cost on the first callback,
    /// acceptable for an opt-in diagnostic build), then sets the `ON_AUDIO` flag.
    #[inline]
    pub fn enter() -> AudioThreadScope {
        // Runs before `ON_AUDIO` is set, so the allocations it makes (ring buffer,
        // logger thread) are not recorded.
        INIT.call_once(init_detector);
        ON_AUDIO.with(|c| c.set(true));
        AudioThreadScope
    }
}

impl Drop for AudioThreadScope {
    #[inline]
    fn drop(&mut self) {
        ON_AUDIO.with(|c| c.set(false));
    }
}

/// One-time setup, run on the audio thread on the first callback: build the ring,
/// publish the producer, and hand the consumer to a background logger thread.
fn init_detector() {
    let (producer, consumer) = RingBuffer::new(RING_CAPACITY);
    // SAFETY: still single-threaded with respect to the producer — no audio
    // callback is recording yet (this runs before `ON_AUDIO` is ever set), and the
    // logger thread (which only touches the consumer) is spawned below.
    unsafe {
        *RING_PRODUCER.0.get() = Some(producer);
    }
    // Publish the producer before flipping the flag so the audio thread never sees
    // `INITIALIZED == true` alongside a `None` producer.
    INITIALIZED.store(true, Ordering::Release);
    spawn_logger(consumer);
}

/// Record one allocator event. Called from the `GlobalAlloc` impl **after** the
/// real `System` call. Returns immediately on any non-audio thread.
#[inline]
fn record_event(size: usize, is_dealloc: bool) {
    // Fast path: nothing to do unless we're on the audio thread.
    if !ON_AUDIO.with(|c| c.get()) {
        return;
    }
    // Re-entrancy backstop (the body below should never allocate, but be safe).
    if IN_DETECTOR.with(|c| c.get()) {
        return;
    }
    IN_DETECTOR.with(|c| c.set(true));

    if is_dealloc {
        AUDIO_DEALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    } else {
        AUDIO_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        AUDIO_ALLOC_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    }

    if INITIALIZED.load(Ordering::Acquire) {
        let mut module_id = [0u8; MODULE_ID_CAP];
        let module_id_len = modular_core::profiling::current_module_id_into(&mut module_id);
        let event = AllocEvent {
            module_id,
            module_id_len,
            size: size.min(u32::MAX as usize) as u32,
            is_dealloc,
        };
        // Claim exclusive access to the single SPSC producer before touching it.
        // Non-blocking: if another audio thread holds the claim (possible for a few
        // callbacks during a device/sample-rate switch, when the new stream starts
        // before the old one is dropped), drop this event rather than race the ring.
        if PRODUCER_BUSY
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            // SAFETY: the CAS guarantees this thread is the only one accessing the
            // producer for the duration of the push (see `ProducerCell`); `push`
            // into the pre-allocated ring never allocates.
            unsafe {
                if let Some(producer) = (*RING_PRODUCER.0.get()).as_mut()
                    && producer.push(event).is_err()
                {
                    DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
                }
            }
            PRODUCER_BUSY.store(false, Ordering::Release);
        } else {
            DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
        }
    }

    IN_DETECTOR.with(|c| c.set(false));
}

/// Spawn the background logger thread that owns the ring consumer. It force-enables
/// module profiling so the audio thread populates frames (and allocations can be
/// attributed) even with the profiling UI closed.
fn spawn_logger(consumer: Consumer<AllocEvent>) {
    let spawned = std::thread::Builder::new()
        .name("alloc-detector-log".to_string())
        .spawn(move || {
            // Force module profiling on independently of the UI enable refcount and
            // the device-switch reset (both drive the shared `ENABLED` flag), so the
            // audio thread always pushes frames and allocations can be attributed to
            // the running module. `refresh_enabled` ORs this in every callback.
            modular_core::profiling::set_force_enabled(true);
            logger_loop(consumer);
        });
    if spawned.is_err() {
        eprintln!("[alloc-detector] failed to spawn logger thread; attribution disabled");
    }
}

/// Per-offender accumulation between reports.
#[derive(Default)]
struct Offender {
    /// Events seen since the last report line for this key.
    pending: u64,
    /// Most recent allocation size for this key (matches the plan's per-line size).
    last_size: u32,
}

/// Snapshot of the running totals, for change-detection on the summary line.
#[derive(Clone, Copy, Default, PartialEq)]
struct Totals {
    allocs: u64,
    deallocs: u64,
    bytes: u64,
    dropped: u64,
}

/// Drain → dedup/rate-limit → stderr. Runs forever on the logger thread; all heap
/// use here (`HashMap`, `String`, `format!`) is fine because this is not the audio
/// thread (the `ON_AUDIO` gate is false here, so these allocations are ignored).
fn logger_loop(mut consumer: Consumer<AllocEvent>) {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    let drain_interval = Duration::from_millis(200);
    let report_interval = Duration::from_millis(1000);

    let mut offenders: HashMap<([u8; MODULE_ID_CAP], u8, bool), Offender> = HashMap::new();
    let mut last_report = Instant::now();
    let mut last_totals = Totals::default();

    loop {
        while let Ok(event) = consumer.pop() {
            let key = (event.module_id, event.module_id_len, event.is_dealloc);
            let offender = offenders.entry(key).or_default();
            offender.pending += 1;
            offender.last_size = event.size;
        }

        if last_report.elapsed() >= report_interval {
            for ((id_bytes, id_len, is_dealloc), offender) in offenders.iter_mut() {
                if offender.pending == 0 {
                    continue;
                }
                let kind = if *is_dealloc { "DEALLOC" } else { "ALLOC" };
                if *id_len == 0 {
                    eprintln!(
                        "[alloc-detector] AUDIO-THREAD {kind} in module \"<unknown>\" \
                         (no active profiler frame — allocation outside any module/scope, \
                         or profiling not yet enabled) — {} bytes (×{} since last report).",
                        offender.last_size, offender.pending,
                    );
                } else {
                    let name = String::from_utf8_lossy(&id_bytes[..*id_len as usize]);
                    eprintln!(
                        "[alloc-detector] AUDIO-THREAD {kind} in module \"{name}\" — {} bytes \
                         (×{} since last report). Move allocation out of process()/update() into \
                         init()/on_patch_update() (see CLAUDE.md lifecycle rules).",
                        offender.last_size, offender.pending,
                    );
                }
                offender.pending = 0;
            }

            let totals = Totals {
                allocs: AUDIO_ALLOC_COUNT.load(Ordering::Relaxed),
                deallocs: AUDIO_DEALLOC_COUNT.load(Ordering::Relaxed),
                bytes: AUDIO_ALLOC_BYTES.load(Ordering::Relaxed),
                dropped: DROPPED_EVENTS.load(Ordering::Relaxed),
            };
            if totals != last_totals {
                eprintln!(
                    "[alloc-detector] totals: {} allocs / {} deallocs / {} bytes on audio thread \
                     ({} events dropped, ring full)",
                    totals.allocs, totals.deallocs, totals.bytes, totals.dropped,
                );
                last_totals = totals;
            }

            // Drop fully-reported entries (all have pending == 0 here) so a long
            // session with many distinct module ids can't grow the map without bound.
            offenders.retain(|_, o| o.pending != 0);

            last_report = Instant::now();
        }

        std::thread::sleep(drain_interval);
    }
}

/// The global allocator installed by the `alloc-detector` feature build. Every
/// method delegates to `std::alloc::System` (so allocation behavior is unchanged)
/// and then records the event via [`record_event`].
pub struct AudioAllocDetector;

// SAFETY: every method performs the real `System` operation first and returns its
// result unchanged; `record_event` only reads thread-locals / atomics and pushes
// to a pre-allocated ring, never affecting the returned pointer. The detector never
// returns null for a successful System allocation and never frees early.
unsafe impl GlobalAlloc for AudioAllocDetector {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_event(layout.size(), false);
        }
        ptr
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        record_event(layout.size(), true);
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_event(layout.size(), false);
        }
        ptr
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // Delegate to System's realloc (may resize in place) rather than the
        // default alloc+copy+dealloc, so the addon's allocation behavior is
        // unperturbed. A realloc still touches the allocator, so flag it.
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            record_event(new_size, false);
        }
        new_ptr
    }
}
