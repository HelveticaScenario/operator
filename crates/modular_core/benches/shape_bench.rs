//! Benchmarks for the scalar `$unstable.shape.*` waveshaper modules.
//!
//! `bench_shape` measures each module's heaviest mode at several channel counts:
//! per-block time scales roughly linearly with channels, since each channel is
//! processed independently in `f32`.

use criterion::{Criterion, criterion_group, criterion_main};
use modular_core::dsp::{get_constructors, get_params_deserializers};
use modular_core::params::DeserializedParams;
use modular_core::types::{ProcessingMode, Sampleable};
use serde_json::json;
use std::hint::black_box;

const SR: f32 = 48000.0;
const BLOCK: usize = 128;

/// Build a shape module forced to `channels` channels (the input constant cycles
/// across them; we only care about the per-channel work, not the input values).
fn make(module: &str, params: serde_json::Value, channels: usize) -> Box<dyn Sampleable> {
    let cached = get_params_deserializers()[module](params).expect("deserialize");
    let deserialized = DeserializedParams {
        params: cached.params,
        channel_count: channels,
    };
    get_constructors()[module](
        &"bench".to_string(),
        SR,
        deserialized,
        BLOCK,
        ProcessingMode::Block,
    )
    .expect("construct")
}

fn process_block(module: &dyn Sampleable) {
    module.start_block();
    module.ensure_processed();
    black_box(module.get_value_at("output", 0, 0));
}

fn bench_shape(c: &mut Criterion) {
    // Heaviest mode of each module, across channel counts.
    let cases: &[(&str, serde_json::Value)] = &[
        (
            "$unstable.shape.saturate",
            json!({ "input": 1.5, "mode": "asymmetric", "drive": 2.5 }),
        ),
        (
            "$unstable.shape.harmonic",
            json!({ "input": 1.5, "mode": "add12345", "drive": 2.5 }),
        ),
        (
            "$unstable.shape.rectify",
            json!({ "input": 1.5, "mode": "soft", "drive": 2.5 }),
        ),
        (
            "$unstable.shape.fold",
            json!({ "input": 1.5, "mode": "westcoast", "drive": 2.5 }),
        ),
        (
            "$unstable.shape.fuzz",
            json!({ "input": 1.5, "mode": "center", "drive": 2.5 }),
        ),
        (
            "$unstable.shape.trigonometric",
            json!({ "input": 1.5, "mode": "cyc10", "drive": 2.5 }),
        ),
        (
            "$unstable.shape.digital",
            json!({ "input": 1.5, "drive": 2.5 }),
        ),
        (
            "$unstable.shape.sine",
            json!({ "input": 1.5, "drive": 2.5 }),
        ),
    ];

    for (module, params) in cases {
        let mut g = c.benchmark_group(module.strip_prefix('$').unwrap_or(module));
        for &channels in &[1usize, 4, 16, 64] {
            let m = make(module, params.clone(), channels);
            g.bench_function(format!("n{channels}"), |b| {
                b.iter(|| process_block(m.as_ref()))
            });
        }
        g.finish();
    }
}

criterion_group!(benches, bench_shape);
criterion_main!(benches);
