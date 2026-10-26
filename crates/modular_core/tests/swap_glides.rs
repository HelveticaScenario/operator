//! Patch swaps glide a param only when its source changed: a cable from the
//! same upstream (even a renamed one) keeps reaching the module unsmoothed,
//! while an edited constant glides from the value it replaced.

mod common;

use std::collections::HashMap;

use common::from_graph;
use modular_core::patch::Patch;
use modular_core::types::{ModuleSpec, PatchGraph, PreviousIds, SameIds};
use serde_json::{Value, json};

const SAMPLE_RATE: f32 = 48000.0;

fn graph(modules: Vec<(&str, &str, Value)>) -> PatchGraph {
    PatchGraph {
        modules: modules
            .into_iter()
            .map(|(id, module_type, params)| ModuleSpec {
                id: id.to_string(),
                module_type: module_type.to_string(),
                id_is_explicit: None,
                skip_state_transfer: None,
                params,
            })
            .collect(),
        module_id_remaps: None,
        scopes: vec![],
        vu_meters: vec![],
    }
}

fn cable(module: &str) -> Value {
    json!({ "type": "cable", "module": module, "port": "output", "channel": 0 })
}

/// A `$remap` whose output equals `out_max` (input sits at `in_max`), so the
/// output shows exactly what the module reads from `out_max`, while `out_min`
/// can change without moving it.
fn remap(out_min: f32, out_max: Value) -> Value {
    json!({ "input": 1.0, "inMin": 0.0, "inMax": 1.0, "outMin": out_min, "outMax": out_max })
}

/// A ~1 kHz sine: audio-rate, so it moves a lot between consecutive samples.
fn sine() -> Value {
    json!({ "freq": 2.0 })
}

fn build(g: &PatchGraph) -> Patch {
    from_graph(g, SAMPLE_RATE, 1, &HashMap::new()).expect("from_graph failed")
}

fn step(patch: &Patch) {
    for module in patch.sampleables.values() {
        module.start_block();
    }
    for module in patch.sampleables.values() {
        module.ensure_processed();
    }
}

fn out(patch: &Patch, id: &str) -> f32 {
    patch.sampleables[id].get_value_at("output", 0, 0)
}

/// Swap `old` for a patch built from `g`, as the audio thread does.
fn swap(old: &Patch, g: &PatchGraph, ids: &dyn PreviousIds) -> Patch {
    let new = build(g);
    for (id, module) in &new.sampleables {
        if let Some(prev) = ids.previous(id).and_then(|p| old.sampleables.get(p)) {
            module.transfer_state_from(prev.as_ref(), ids);
        }
    }
    new.connect_all();
    new
}

/// Runs `patch` for `frames` samples, asserting `remap` tracks the sine
/// within `tolerance` on every one.
fn assert_tracks(patch: &Patch, sine_id: &str, frames: usize, tolerance: f32) {
    for frame in 0..frames {
        step(patch);
        let (s, r) = (out(patch, sine_id), out(patch, "remap"));
        assert!(
            (s - r).abs() <= tolerance,
            "frame {frame}: remap {r} strays from its source {s}"
        );
    }
}

#[test]
fn unchanged_cable_passes_through_every_swap() {
    let g = graph(vec![
        ("lfo", "$sine", sine()),
        ("remap", "$remap", remap(0.0, cable("lfo"))),
    ]);
    let mut patch = build(&g);
    patch.connect_all();
    assert_tracks(&patch, "lfo", 100, 0.0);
    for _ in 0..20 {
        patch = swap(&patch, &g, &SameIds);
        assert_tracks(&patch, "lfo", 37, 0.0);
    }
}

#[test]
fn editing_another_param_leaves_a_cable_unsmoothed() {
    let before = graph(vec![
        ("lfo", "$sine", sine()),
        ("remap", "$remap", remap(0.0, cable("lfo"))),
    ]);
    let after = graph(vec![
        ("lfo", "$sine", sine()),
        ("remap", "$remap", remap(2.0, cable("lfo"))),
    ]);
    let patch = build(&before);
    patch.connect_all();
    assert_tracks(&patch, "lfo", 100, 0.0);
    let patch = swap(&patch, &after, &SameIds);
    assert_tracks(&patch, "lfo", 200, 1e-5);
}

/// The id a module had before a rename: `lfo2` was `lfo`.
struct Renamed;

impl PreviousIds for Renamed {
    fn previous<'a>(&'a self, id: &'a str) -> Option<&'a str> {
        Some(if id == "lfo2" { "lfo" } else { id })
    }
}

#[test]
fn renamed_upstream_counts_as_the_same_source() {
    let before = graph(vec![
        ("lfo", "$sine", sine()),
        ("remap", "$remap", remap(0.0, cable("lfo"))),
    ]);
    let after = graph(vec![
        ("lfo2", "$sine", sine()),
        ("remap", "$remap", remap(0.0, cable("lfo2"))),
    ]);
    let patch = build(&before);
    patch.connect_all();
    assert_tracks(&patch, "lfo", 100, 0.0);
    let patch = swap(&patch, &after, &Renamed);
    assert_tracks(&patch, "lfo2", 200, 0.0);
}

#[test]
fn rewiring_to_another_source_glides() {
    let before = graph(vec![
        ("lfo", "$sine", sine()),
        ("other", "$signal", json!({ "value": 3.0 })),
        ("remap", "$remap", remap(0.0, cable("lfo"))),
    ]);
    let after = graph(vec![
        ("lfo", "$sine", sine()),
        ("other", "$signal", json!({ "value": 3.0 })),
        ("remap", "$remap", remap(0.0, cable("other"))),
    ]);
    let patch = build(&before);
    patch.connect_all();
    for _ in 0..100 {
        step(&patch);
    }
    let last = out(&patch, "remap");
    let patch = swap(&patch, &after, &SameIds);
    step(&patch);
    assert_eq!(
        out(&patch, "remap"),
        last,
        "the swap sample holds the old value"
    );
    for _ in 0..4800 {
        step(&patch);
    }
    assert!(
        (out(&patch, "remap") - 3.0).abs() < 1e-4,
        "settles on the new source"
    );
}

#[test]
fn edited_constant_glides_from_the_old_value() {
    let before = graph(vec![("remap", "$remap", remap(0.0, json!(0.0)))]);
    let after = graph(vec![("remap", "$remap", remap(0.0, json!(4.0)))]);
    let patch = build(&before);
    patch.connect_all();
    step(&patch);
    let patch = swap(&patch, &after, &SameIds);
    let mut prev = 0.0;
    for frame in 0..4800 {
        step(&patch);
        let y = out(&patch, "remap");
        assert!(
            y >= prev && y - prev < 0.1,
            "frame {frame}: {prev} → {y} is not a glide"
        );
        prev = y;
    }
    assert!((prev - 4.0).abs() < 1e-4);
}

#[test]
fn block_path_hands_an_edit_to_the_glide() {
    let before = graph(vec![("sig", "$signal", json!({ "value": 0.0 }))]);
    let after = graph(vec![("sig", "$signal", json!({ "value": 4.0 }))]);
    let patch = build(&before);
    patch.connect_all();
    for _ in 0..100 {
        step(&patch);
    }
    let patch = swap(&patch, &after, &SameIds);
    let mut prev = 0.0;
    for frame in 0..4800 {
        step(&patch);
        let y = out(&patch, "sig");
        assert!(
            y >= prev && y - prev < 0.1,
            "frame {frame}: {prev} → {y} is not a glide"
        );
        prev = y;
    }
    assert_eq!(prev, 4.0, "settles exactly on the new value");
}
