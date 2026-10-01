//! Whole-patch benchmarks driven through the generated module wrappers, the
//! way the audio callback runs them: `start_block` on every module, then
//! `ensure_processed_to(block_size)` in producer-before-consumer order. Unlike
//! the per-module benches, these include cable reads, block-buffer copies and
//! wrapper dispatch, so they track engine overhead as well as DSP cost.
//!
//! Each case runs at several channel counts; polyphonic cables connect one
//! cable per channel.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use modular_core::Patch;
use modular_core::dsp::get_params_deserializers;
use modular_core::params::DeserializedParams;
use modular_core::types::ProcessingMode;
use serde_json::{Value, json};

const SR: f32 = 48000.0;
const BLOCK: usize = 64;
const CHANNELS: [usize; 3] = [1, 8, 16];

fn poly(base: f32, channels: usize) -> Value {
    Value::Array(
        (0..channels)
            .map(|ch| json!(base + ch as f32 * 0.07))
            .collect(),
    )
}

fn poly_cable(module: &str, channels: usize) -> Value {
    Value::Array(
        (0..channels)
            .map(|ch| json!({ "type": "cable", "module": module, "port": "output", "channel": ch }))
            .collect(),
    )
}

/// A connected all-Block patch plus its processing order (`modules` must
/// already list producers before consumers).
struct BenchPatch {
    patch: Patch,
    order: Vec<String>,
}

impl BenchPatch {
    fn new(modules: Vec<(String, &str, Value)>) -> Self {
        let deserializers = get_params_deserializers();
        let order = modules.iter().map(|(id, _, _)| id.clone()).collect();
        let mut patch = Patch::new();
        patch
            .insert_modules(
                modules.into_iter().map(|(id, module_type, params)| {
                    let cached = deserializers[module_type](params).expect("deserialize");
                    (
                        id,
                        module_type.to_string(),
                        DeserializedParams {
                            params: cached.params,
                            channel_count: cached.channel_count,
                        },
                        ProcessingMode::Block,
                    )
                }),
                SR,
                BLOCK,
            )
            .expect("insert");
        patch.connect_all();
        Self { patch, order }
    }

    fn process_block(&self) {
        for module in self.patch.sampleables.values() {
            module.start_block();
        }
        for id in &self.order {
            self.patch.sampleables[id].ensure_processed_to(BLOCK);
        }
    }
}

/// `$saw` into a chain of 32 `$lpf`s at a constant cutoff.
fn lpf_chain(channels: usize) -> BenchPatch {
    let mut modules = vec![(
        "src".to_string(),
        "$saw",
        json!({ "freq": poly(0.0, channels) }),
    )];
    for i in 0..32 {
        let prev = if i == 0 {
            "src".to_string()
        } else {
            format!("f{}", i - 1)
        };
        modules.push((
            format!("f{i}"),
            "$lpf",
            json!({ "input": poly_cable(&prev, channels), "cutoff": 2.0, "resonance": 1.0 }),
        ));
    }
    BenchPatch::new(modules)
}

/// 8 copies of gate → `$adsr` → (`$saw` → `$lpf` with envelope cutoff →
/// envelope VCA) → `$mix`.
fn voices(channels: usize) -> BenchPatch {
    let mut modules = Vec::new();
    for v in 0..8 {
        let id = |name: &str| format!("{name}{v}");
        modules.push((
            id("gate"),
            "$pulse",
            json!({ "freq": poly(-4.0, channels) }),
        ));
        modules.push((
            id("env"),
            "$adsr",
            json!({
                "gate": poly_cable(&id("gate"), channels),
                "attack": 0.01, "decay": 0.2, "sustain": 2.0, "release": 0.3,
            }),
        ));
        modules.push((id("osc"), "$saw", json!({ "freq": poly(-1.0, channels) })));
        modules.push((
            id("lpf"),
            "$lpf",
            json!({
                "input": poly_cable(&id("osc"), channels),
                "cutoff": poly_cable(&id("env"), channels),
                "resonance": 1.5,
            }),
        ));
        modules.push((
            id("vca"),
            "$scaleAndShift",
            json!({
                "input": poly_cable(&id("lpf"), channels),
                "scale": poly_cable(&id("env"), channels),
            }),
        ));
        modules.push((
            id("mix"),
            "$mix",
            json!({ "inputs": [poly_cable(&id("vca"), channels)] }),
        ));
    }
    BenchPatch::new(modules)
}

fn bench_patches(c: &mut Criterion) {
    let cases: [(&str, fn(usize) -> BenchPatch); 2] =
        [("lpf_chain_32", lpf_chain), ("voices_8", voices)];
    for (name, build) in cases {
        let mut group = c.benchmark_group(name);
        for channels in CHANNELS {
            let patch = build(channels);
            group.bench_with_input(BenchmarkId::from_parameter(channels), &patch, |b, patch| {
                b.iter(|| patch.process_block())
            });
        }
        group.finish();
    }
}

criterion_group!(benches, bench_patches);
criterion_main!(benches);
