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
use modular_core::types::{ProcessingMode, SampleBuffer, WavData};
use serde_json::{Value, json};
use std::sync::Arc;

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

fn poly_cable(module: &str, port: &str, channels: usize) -> Value {
    Value::Array(
        (0..channels)
            .map(|ch| json!({ "type": "cable", "module": module, "port": port, "channel": ch }))
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
        patch.wav_data.insert("noise".into(), noise_wav());
        patch.wav_data.insert("table".into(), table_wav());
        for module in patch.sampleables.values() {
            module.prepare_resources(&patch.wav_data);
        }
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
            json!({ "input": poly_cable(&prev, "output", channels), "cutoff": 2.0, "resonance": 1.0 }),
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
                "gate": poly_cable(&id("gate"), "output", channels),
                "attack": 0.01, "decay": 0.2, "sustain": 2.0, "release": 0.3,
            }),
        ));
        modules.push((id("osc"), "$saw", json!({ "freq": poly(-1.0, channels) })));
        modules.push((
            id("lpf"),
            "$lpf",
            json!({
                "input": poly_cable(&id("osc"), "output", channels),
                "cutoff": poly_cable(&id("env"), "output", channels),
                "resonance": 1.5,
            }),
        ));
        modules.push((
            id("vca"),
            "$scaleAndShift",
            json!({
                "input": poly_cable(&id("lpf"), "output", channels),
                "scale": poly_cable(&id("env"), "output", channels),
            }),
        ));
        modules.push((
            id("mix"),
            "$mix",
            json!({ "inputs": [poly_cable(&id("vca"), "output", channels)] }),
        ));
    }
    BenchPatch::new(modules)
}

/// `$saw` into a chain of 8 `$comp`s with constant settings.
fn comp_chain(channels: usize) -> BenchPatch {
    let mut modules = vec![(
        "src".to_string(),
        "$saw",
        json!({ "freq": poly(0.0, channels) }),
    )];
    for i in 0..8 {
        let prev = if i == 0 {
            "src".to_string()
        } else {
            format!("c{}", i - 1)
        };
        modules.push((
            format!("c{i}"),
            "$comp",
            json!({
                "input": poly_cable(&prev, if i == 0 { "output" } else { "sample" }, channels),
                "threshold": 2.0, "ratio": 4.0, "attack": 0.01, "release": 0.1,
                "makeup": 1.0, "inputGain": 0.5, "outputGain": -0.5,
            }),
        ));
    }
    BenchPatch::new(modules)
}

/// Two seconds of deterministic white noise at 48 kHz, for `$grains`.
fn noise_wav() -> Arc<WavData> {
    let mut state = 1u32;
    let samples = (0..96_000)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / 8_388_608.0 - 1.0
        })
        .collect();
    Arc::new(WavData::new(
        SampleBuffer::from_samples(vec![samples], 48_000.0),
        None,
    ))
}

/// A 64-frame wavetable of 2048-sample cycles, for `$wavetable`.
fn table_wav() -> Arc<WavData> {
    const SIZE: usize = 2048;
    let samples = (0..64)
        .flat_map(|frame| {
            (0..SIZE).map(move |i| {
                let phase = std::f32::consts::TAU * i as f32 / SIZE as f32;
                (phase * (1.0 + frame as f32 * 0.1)).sin() * 0.8
            })
        })
        .collect();
    Arc::new(WavData::new(
        SampleBuffer::from_samples(vec![samples], 48_000.0),
        Some(SIZE),
    ))
}

/// 4 `$grains` clouds (≈ 2.5, 10, 30 and 60 active grains per channel), each
/// gated by its own slow pulse.
fn grains(channels: usize) -> BenchPatch {
    let mut modules = Vec::new();
    for (i, (density, length)) in [(2.5, 1.0), (10.0, 1.0), (10.0, 3.0), (20.0, 3.0)]
        .iter()
        .enumerate()
    {
        let gate = format!("gate{i}");
        modules.push((
            gate.clone(),
            "$pulse",
            json!({ "freq": poly(-6.0, channels), "width": 4.5 }),
        ));
        modules.push((
            format!("grains{i}"),
            "$grains",
            json!({
                "wav": { "type": "wav_ref", "path": "noise", "channels": 1 },
                "pitch": poly(0.0, channels),
                "gate": poly_cable(&gate, "output", channels),
                "density": density,
                "length": length,
            }),
        ));
    }
    BenchPatch::new(modules)
}

/// 8 `$wavetable` oscillators: half at a fixed position, half scanned by an LFO.
fn wavetables(channels: usize) -> BenchPatch {
    let mut modules = vec![(
        "lfo".to_string(),
        "$sine",
        json!({ "freq": poly(-5.0, channels) }),
    )];
    for i in 0..8 {
        let position = if i % 2 == 0 {
            json!(2.0)
        } else {
            poly_cable("lfo", "output", channels)
        };
        modules.push((
            format!("wt{i}"),
            "$wavetable",
            json!({
                "wav": { "type": "wav_ref", "path": "table", "channels": 1 },
                "pitch": poly(i as f32 * 0.3 - 1.0, channels),
                "position": position,
            }),
        ));
    }
    BenchPatch::new(modules)
}

fn bench_patches(c: &mut Criterion) {
    let cases: [(&str, fn(usize) -> BenchPatch); 5] = [
        ("lpf_chain_32", lpf_chain),
        ("voices_8", voices),
        ("comp_chain_8", comp_chain),
        ("grains_4", grains),
        ("wavetable_8", wavetables),
    ];
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
