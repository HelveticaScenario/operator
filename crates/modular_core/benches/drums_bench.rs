//! Per-voice cost of the `$tr808.*` and `$tr909.*` drums, each struck by a
//! `$pulse` clock so its circuit spends the run in a mix of attacks, tails
//! and rest, the way a pattern plays it.
//!
//! Prints the best of several runs as nanoseconds per sample per channel,
//! including the clock, which costs a few nanoseconds.
//! `cargo bench --bench drums_bench [-- <filter> [channels,…]]`.

use std::time::Instant;

use modular_core::Patch;
use modular_core::dsp::get_params_deserializers;
use modular_core::params::DeserializedParams;
use modular_core::types::ProcessingMode;
use serde_json::{Value, json};

const SR: f32 = 48_000.0;
const BLOCK: usize = 64;
const SECS: f32 = 4.0;
const RUNS: usize = 7;

/// Every `$tr808.*` and `$tr909.*` voice.
fn voices() -> Vec<String> {
    modular_core::dsp::drums::schemas()
        .into_iter()
        .map(|s| s.name)
        .collect()
}

/// A clock per channel at about 2 Hz, detuned so channels strike apart, with
/// the accent swept across channels.
fn patch(voice: &str, channels: usize) -> Patch {
    let clock = Value::Array(
        (0..channels)
            .map(|ch| json!(-7.0 + ch as f32 * 0.13))
            .collect(),
    );
    let trigger = Value::Array(
        (0..channels)
            .map(
                |ch| json!({ "type": "cable", "module": "clock", "port": "output", "channel": ch }),
            )
            .collect(),
    );
    let accent = Value::Array(
        (0..channels)
            .map(|ch| json!(5.0 * ch as f32 / channels.max(2) as f32))
            .collect(),
    );
    let deserializers = get_params_deserializers();
    let modules = [
        ("clock", "$pulse", json!({ "freq": clock })),
        (
            "drum",
            voice,
            if voice == "$tr808.hiHat" {
                json!({ "closed": trigger, "open": trigger, "accent": accent })
            } else {
                json!({ "trigger": trigger, "accent": accent })
            },
        ),
    ];
    let mut patch = Patch::new();
    patch
        .insert_modules(
            modules.into_iter().map(|(id, module_type, params)| {
                let cached = deserializers[module_type](params).expect("deserialize");
                (
                    id.to_string(),
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
    for module in patch.sampleables.values() {
        module.on_patch_update();
    }
    patch
}

fn run(patch: &Patch) -> f64 {
    let blocks = (SECS * SR) as usize / BLOCK;
    let start = Instant::now();
    for _ in 0..blocks {
        for module in patch.sampleables.values() {
            module.start_block();
        }
        patch.sampleables["clock"].ensure_processed_to(BLOCK);
        patch.sampleables["drum"].ensure_processed_to(BLOCK);
    }
    start.elapsed().as_secs_f64()
}

fn main() {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();
    let filter = args.first().cloned().unwrap_or_default();
    let channels: Vec<usize> = match args.get(1) {
        Some(list) => list
            .split(',')
            .map(|n| n.parse().expect("channels"))
            .collect(),
        None => vec![1, 4],
    };
    let samples = (SECS * SR) as usize / BLOCK * BLOCK;
    print!("{:<20}", "voice");
    for ch in &channels {
        print!("{:>14}", format!("ns/s/ch @{ch}"));
    }
    println!();
    let mut totals = vec![0.0; channels.len()];
    for voice in voices().iter().filter(|v| v.contains(filter.as_str())) {
        print!("{voice:<20}");
        for (i, &ch) in channels.iter().enumerate() {
            let p = patch(voice, ch);
            let best = (0..RUNS).map(|_| run(&p)).fold(f64::INFINITY, f64::min);
            let ns = best * 1e9 / samples as f64 / ch as f64;
            totals[i] += ns;
            print!("{ns:>14.1}");
        }
        println!();
    }
    print!("{:<20}", "total");
    for t in totals {
        print!("{t:>14.1}");
    }
    println!();
}
