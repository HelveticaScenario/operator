//! Filters must stay bounded when their frequency is modulated hard: a ±5 V
//! input may ring up to the filter's fixed-frequency resonant peak, never far
//! past it.

use std::collections::HashMap;

use modular_core::patch::Patch;
use modular_core::types::{ModuleSpec, PatchGraph};
use serde_json::{Value, json};

mod common;
use common::from_graph;

const SAMPLE_RATE: f32 = 48_000.0;
const FRAMES: usize = 24_000;

fn cable(module: &str) -> Value {
    json!({ "type": "cable", "module": module, "port": "output", "channel": 0 })
}

fn filter_params(module_type: &str, freq: Value) -> Value {
    match module_type {
        "$bpf" => json!({ "input": cable("src"), "center": freq, "resonance": 4.5 }),
        "$xover" => json!({ "input": cable("src"), "lowMidFreq": freq }),
        _ => json!({ "input": cable("src"), "cutoff": freq, "resonance": 4.5 }),
    }
}

/// Peak |output| over every output port of the filter.
fn peak(module_type: &str, freq: Value, modulator: Option<(&str, Value)>) -> f32 {
    let mut modules = vec![
        ("src", "$sine", json!({ "freq": -1.25 })),
        ("filt", module_type, filter_params(module_type, freq)),
    ];
    if let Some((t, p)) = modulator {
        modules.push(("mod", t, p));
    }
    let graph = PatchGraph {
        modules: modules
            .into_iter()
            .map(|(id, t, params)| ModuleSpec {
                id: id.into(),
                module_type: t.into(),
                id_is_explicit: None,
                skip_state_transfer: None,
                params,
            })
            .collect(),
        module_id_remaps: None,
        scopes: vec![],
        vu_meters: vec![],
    };
    let patch: Patch = from_graph(&graph, SAMPLE_RATE, 1, &HashMap::new()).unwrap();
    let ports: &[&str] = if module_type == "$xover" {
        &["low", "mid", "high"]
    } else {
        &["output"]
    };
    let filt = &patch.sampleables["filt"];
    let mut peak = 0.0f32;
    for _ in 0..FRAMES {
        for m in patch.sampleables.values() {
            m.start_block();
        }
        for m in patch.sampleables.values() {
            m.ensure_processed();
        }
        for port in ports {
            let v = filt.get_value_at(port, 0, 0);
            peak = if v.is_finite() {
                peak.max(v.abs())
            } else {
                f32::INFINITY
            };
        }
    }
    peak
}

#[test]
fn filters_stay_bounded_under_frequency_modulation() {
    for module_type in ["$lpf", "$hpf", "$bpf", "$xover"] {
        let ceiling = [-5.0f32, -3.0, -1.0, 1.0, 3.0, 5.0]
            .into_iter()
            .map(|v| peak(module_type, json!(v), None))
            .fold(5.0f32, f32::max);
        for (name, modulator) in [
            ("stepped", ("$pulse", json!({ "freq": -2.0 }))),
            ("audio-rate", ("$sine", json!({ "freq": 0.0 }))),
        ] {
            let p = peak(module_type, cable("mod"), Some(modulator));
            assert!(
                p <= ceiling * 1.5,
                "{module_type} {name} modulation peaked at {p} V (fixed-frequency ceiling {ceiling} V)"
            );
        }
    }
}
