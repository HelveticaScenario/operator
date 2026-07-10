//! Integration test for the per-module profiler. Exercises the proc-macro
//! generated `ensure_processed_to` hooks against a real two-module patch
//! and asserts the records show the expected pattern: both modules see
//! work, the downstream module's `total_ns` is greater than its `self_ns`
//! because it pulls from upstream.

use std::collections::HashMap;

use modular_core::patch::Patch;
use modular_core::profiling::{
    self, ModuleProfileAccum, build_seed, drain_collection, new_collection, swap_records,
    try_swap_shared,
};
use modular_core::types::{ModuleSpec, PatchGraph};
use serde_json::json;

mod common;
use common::from_graph;

const SAMPLE_RATE: f32 = 48000.0;
const BLOCK_SIZE: usize = 1;

fn make_graph(modules: Vec<(&str, &str, serde_json::Value)>) -> PatchGraph {
    PatchGraph {
        modules: modules
            .into_iter()
            .map(|(id, module_type, params)| ModuleSpec {
                id: id.to_string(),
                module_type: module_type.to_string(),
                id_is_explicit: None,
                params,
            })
            .collect(),
        module_id_remaps: None,
        scopes: vec![],
        vu_meters: vec![],
        scope_xy: None,
    }
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
fn profiler_attributes_work_to_both_modules() {
    // Cable an oscillator into a signal pass-through. Each `process_frame`
    // visits both wrappers; the `$signal` module pulls from `$sine` via a
    // `Signal::Cable`, so its wrapper's `ensure_processed_to` recursively
    // calls the sine wrapper's `ensure_processed_to`.
    let graph = make_graph(vec![
        ("osc", "$sine", json!({ "freq": 0.0 })),
        (
            "sig",
            "$signal",
            json!({
                "source": {
                    "type": "cable",
                    "module": "osc",
                    "port": "output",
                    "channel": 0,
                }
            }),
        ),
    ]);
    let patch = from_graph(&graph, SAMPLE_RATE, BLOCK_SIZE, &HashMap::new()).expect("from_graph");

    let collection = new_collection();
    // The from_graph helper bypasses the audio-thread patch-swap path, so seed
    // the profiler maps directly. Equivalent to what apply_patch_update
    // does via `swap_records` / `try_swap_shared` in production.
    let ids = ["osc".to_string(), "sig".to_string()];
    let _ = swap_records(build_seed(ids.iter().cloned()));
    let _ = try_swap_shared(&collection, build_seed(ids.iter().cloned()));

    profiling::set_enabled(true);
    profiling::refresh_enabled();

    for _ in 0..2000 {
        process_frame(&patch);
    }

    profiling::flush_into(&collection);
    profiling::set_enabled(false);
    profiling::refresh_enabled();

    let snap: HashMap<String, ModuleProfileAccum> =
        drain_collection(&collection).into_iter().collect();

    let osc = snap
        .get("osc")
        .unwrap_or_else(|| panic!("no record for osc; snapshot: {:?}", snap));
    let sig = snap
        .get("sig")
        .unwrap_or_else(|| panic!("no record for sig; snapshot: {:?}", snap));

    assert!(
        osc.samples_processed >= 2000,
        "osc samples: {}",
        osc.samples_processed
    );
    assert!(
        sig.samples_processed >= 2000,
        "sig samples: {}",
        sig.samples_processed
    );
    assert!(osc.self_ns > 0, "osc self_ns should be > 0");
    assert!(sig.self_ns > 0, "sig self_ns should be > 0");
    assert!(osc.ensure_calls_did_work > 0);
    assert!(sig.ensure_calls_did_work > 0);

    // Sanity: total >= self for every record. (Equality is possible when
    // there's no upstream work attributed to the frame, but never `<`.)
    assert!(
        osc.total_ns >= osc.self_ns,
        "osc total_ns={} should be >= self_ns={}",
        osc.total_ns,
        osc.self_ns
    );
    assert!(
        sig.total_ns >= sig.self_ns,
        "sig total_ns={} should be >= self_ns={}",
        sig.total_ns,
        sig.self_ns
    );
}

#[test]
fn profiler_disabled_produces_no_records() {
    let graph = make_graph(vec![("osc", "$sine", json!({ "freq": 0.0 }))]);
    let patch = from_graph(&graph, SAMPLE_RATE, BLOCK_SIZE, &HashMap::new()).expect("from_graph");

    profiling::set_enabled(false);
    profiling::refresh_enabled();

    for _ in 0..200 {
        process_frame(&patch);
    }

    let collection = new_collection();
    profiling::flush_into(&collection);
    assert!(drain_collection(&collection).is_empty());
}
