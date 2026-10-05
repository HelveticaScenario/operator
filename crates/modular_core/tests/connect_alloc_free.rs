//! `Patch::connect_all` runs inside the audio callback on every patch apply,
//! so the whole call — cable resolution and `on_patch_update` — must never
//! touch the heap. `#[default_connection]` inputs (e.g. the ROOT_CLOCK
//! playhead normalled into `$cycle`/`$track`) are filled at module
//! construction on the main thread, so a patch that leaves them disconnected
//! still connects allocation-free.
//!
//! Lives in its own test binary: the counting `#[global_allocator]` is
//! process-wide.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use modular_core::dsp::get_params_deserializers;
use modular_core::params::DeserializedParams;
use modular_core::patch::Patch;
use modular_core::types::ProcessingMode;
use serde_json::json;

struct CountingAllocator;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static DEALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNTING.load(Ordering::Relaxed) {
            DEALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Deserialize and construct the given modules into an unconnected `Patch`,
/// mirroring what the app's main thread does before handing the patch to the
/// audio thread (which then calls `connect_all`).
fn build_unconnected_patch(modules: Vec<(&str, &str, serde_json::Value)>) -> Patch {
    let deserializers = get_params_deserializers();
    let mut to_insert = Vec::with_capacity(modules.len());
    for (id, module_type, params) in modules {
        let cached = deserializers
            .get(module_type)
            .unwrap_or_else(|| panic!("no params deserializer for '{module_type}'"))(
            params
        )
        .unwrap_or_else(|e| panic!("params deserialization for '{module_type}' failed: {e}"));
        to_insert.push((
            id.to_string(),
            module_type.to_string(),
            DeserializedParams {
                params: cached.params,
                channel_count: cached.channel_count,
            },
            ProcessingMode::Block,
        ));
    }
    let mut patch = Patch::new();
    patch
        .insert_modules(to_insert, 48000.0, 1)
        .expect("insert_modules failed");
    patch.rebuild_message_listeners();
    patch
}

fn process_frame(patch: &Patch) {
    for module in patch.sampleables.values() {
        module.start_block();
    }
    for module in patch.sampleables.values() {
        module.ensure_processed();
    }
}

#[test]
fn connect_all_with_default_connections_is_allocation_free() {
    // Neither $track nor $cycle gets an explicit playhead, exactly like the
    // DSL emits them, so both rely on their #[default_connection] to
    // ROOT_CLOCK's playhead output.
    let pattern = modular_core::dsp::seq::seq_value::ParsedPatternPayload::parse_for_test("c4");
    let patch = build_unconnected_patch(vec![
        (
            "ROOT_CLOCK",
            "_clock",
            json!({ "tempo": 240.0, "numerator": 4, "denominator": 4 }),
        ),
        (
            "track",
            "$track",
            json!({ "keyframes": [[0.0, 0.0], [10.0, 1.0]] }),
        ),
        (
            "seq",
            "$cycle",
            json!({ "pattern": serde_json::to_value(&pattern).unwrap() }),
        ),
    ]);

    ALLOCS.store(0, Ordering::SeqCst);
    DEALLOCS.store(0, Ordering::SeqCst);
    COUNTING.store(true, Ordering::SeqCst);
    patch.connect_all();
    COUNTING.store(false, Ordering::SeqCst);

    let allocs = ALLOCS.load(Ordering::SeqCst);
    let deallocs = DEALLOCS.load(Ordering::SeqCst);
    assert_eq!(
        (allocs, deallocs),
        (0, 0),
        "connect_all must not touch the heap ({allocs} allocs / {deallocs} deallocs)"
    );

    // The defaults must still be live: with ROOT_CLOCK's playhead ramping
    // 0 → 1 over one bar (240 BPM 4/4 = 48000 samples at 48 kHz), the track's
    // 0 → 10 keyframe ramp follows it. A disconnected playhead would pin the
    // output at the first keyframe (0.0).
    let track = patch.sampleables.get("track").unwrap();
    let mut early = 0.0f32;
    let mut late = 0.0f32;
    for frame in 1..=20_000 {
        process_frame(&patch);
        match frame {
            4_000 => early = track.get_value_at("output", 0, 0),
            20_000 => late = track.get_value_at("output", 0, 0),
            _ => {}
        }
    }
    assert!(
        late > early && late > 0.5,
        "track output should follow the default ROOT_CLOCK playhead \
         (early={early}, late={late})"
    );
}
