//! Dev-only runtime detector for heap allocation/deallocation on the audio thread.
//!
//! Gated behind the `alloc-detector` Cargo feature (off by default). `yarn start`
//! never compiles it in — the default build installs no global allocator, and
//! [`panic_on_alloc`] is an inlined pass-through, so the binary is byte-identical.
//! `yarn start:alloc` or `yarn build-native-alloc` builds with
//! `--features=alloc-detector`, which installs [`AudioAllocDetector`] as the
//! process `#[global_allocator]`.
//!
//! How it works: [`panic_on_alloc`] runs its closure inside a no-alloc region
//! (the audio DSP work). While that region is active, [`AudioAllocDetector`]
//! captures a backtrace at the site of the **first** heap (de)allocation made on
//! that thread. Once the region exits — where allocating, and therefore panicking,
//! is safe again — `panic_on_alloc` panics with that allocation-site backtrace.
//! The audio callback's `catch_unwind` catches the panic, which flips the
//! poisoned-thread flag and drives the existing emit-silence-and-restart path; the
//! `panic_log` hook and the panic payload record the backtrace pointing at the
//! offending code.
//!
//! All state is thread-local, so two cpal callback threads briefly overlapping
//! during a device/sample-rate switch each capture into their own slot with no
//! synchronization.

#[cfg(feature = "alloc-detector")]
mod imp {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::backtrace::Backtrace;
    use std::cell::{Cell, RefCell};

    thread_local! {
        /// Nonzero while this thread is inside a [`panic_on_alloc`] region. The
        /// allocator only records while it is set, so only the audio DSP is policed.
        /// `const` init keeps the TLS access allocation-free.
        static FORBID_DEPTH: Cell<u32> = const { Cell::new(0) };
        /// Re-entrancy backstop: set while capturing a backtrace (which itself
        /// allocates), so the capture's own allocations are not recorded.
        static CAPTURING: Cell<bool> = const { Cell::new(false) };
        /// The first forbidden (de)allocation seen in the current region, captured
        /// at its site. [`panic_on_alloc`] takes it once the region exits.
        static CAPTURED: RefCell<Option<Captured>> = const { RefCell::new(None) };
    }

    /// A forbidden allocator event captured at its site.
    struct Captured {
        backtrace: Backtrace,
        size: usize,
        is_dealloc: bool,
    }

    /// Record one allocator event, called from the [`AudioAllocDetector`] methods
    /// **after** the real `System` call. Outside a no-alloc region it is a single
    /// `Cell::get`; the first event inside one captures a site backtrace.
    #[inline]
    fn record(size: usize, is_dealloc: bool) {
        if FORBID_DEPTH.with(|d| d.get()) == 0 {
            return;
        }
        if CAPTURING.with(|c| c.get()) {
            return;
        }
        CAPTURING.with(|c| c.set(true));
        CAPTURED.with(|slot| {
            // Drop the shared borrow before `force_capture` so the capture's own
            // (re-entrant, `CAPTURING`-skipped) allocations can never collide with
            // the exclusive borrow taken below.
            let empty = slot.borrow().is_none();
            if empty {
                let backtrace = Backtrace::force_capture();
                *slot.borrow_mut() = Some(Captured {
                    backtrace,
                    size,
                    is_dealloc,
                });
            }
        });
        CAPTURING.with(|c| c.set(false));
    }

    /// Bumps [`FORBID_DEPTH`] for the lifetime of the no-alloc region, restoring it
    /// on drop so a panic mid-region still exits the region cleanly.
    struct ForbidGuard;

    impl ForbidGuard {
        #[inline]
        fn enter() -> ForbidGuard {
            FORBID_DEPTH.with(|d| d.set(d.get() + 1));
            ForbidGuard
        }
    }

    impl Drop for ForbidGuard {
        #[inline]
        fn drop(&mut self) {
            FORBID_DEPTH.with(|d| d.set(d.get() - 1));
        }
    }

    /// Run `f` forbidding heap (de)allocation on this thread. The first violation
    /// is captured with a site backtrace; this then panics with it **after** the
    /// region exits (`FORBID_DEPTH` back to 0), where allocating to format the
    /// message and unwinding are both safe. The audio callback's `catch_unwind`
    /// catches the panic and drives the restart path.
    pub fn panic_on_alloc<T>(f: impl FnOnce() -> T) -> T {
        CAPTURED.with(|slot| *slot.borrow_mut() = None);
        let ret = {
            let _guard = ForbidGuard::enter();
            f()
        };
        if let Some(c) = CAPTURED.with(|slot| slot.borrow_mut().take()) {
            let kind = if c.is_dealloc {
                "deallocation"
            } else {
                "allocation"
            };
            panic!(
                "audio-thread heap {kind} of {n} bytes inside the no-alloc region. \
                Allocation-site backtrace:\n{bt}",
                n = c.size,
                bt = c.backtrace,
            );
        }
        ret
    }

    /// The global allocator installed by the `alloc-detector` feature build. Every
    /// method delegates to `std::alloc::System` (so allocation behavior is
    /// unchanged) and then records the event via [`record`].
    pub struct AudioAllocDetector;

    // SAFETY: every method performs the real `System` operation first and returns
    // its result unchanged; `record` only reads/writes thread-locals and (on the
    // first violation in a region) captures a backtrace, never affecting the
    // returned pointer. The detector never returns null for a successful System
    // allocation and never frees early.
    unsafe impl GlobalAlloc for AudioAllocDetector {
        #[inline]
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc(layout) };
            if !ptr.is_null() {
                record(layout.size(), false);
            }
            ptr
        }

        #[inline]
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) };
            record(layout.size(), true);
        }

        #[inline]
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc_zeroed(layout) };
            if !ptr.is_null() {
                record(layout.size(), false);
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
                record(new_size, false);
            }
            new_ptr
        }
    }
}

#[cfg(feature = "alloc-detector")]
pub use imp::{AudioAllocDetector, panic_on_alloc};

/// Pass-through when the detector is not compiled in: runs `f` directly, so the
/// audio callback's wrapping is zero-cost in the default build.
#[cfg(not(feature = "alloc-detector"))]
#[inline(always)]
pub fn panic_on_alloc<T>(f: impl FnOnce() -> T) -> T {
    f()
}
