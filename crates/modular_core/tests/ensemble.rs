//! `$ensemble`: pitch, quantization, spread, the split outputs, and an
//! allocation-free audio path across its routing and warp options.

mod block_common;

use block_common::*;
use modular_core::Patch;
use modular_core::types::ProcessingMode;
use serde_json::{Value, json};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const C4_HZ: f32 = 261.625_58;

fn semis(n: f32) -> f32 {
    n / 12.0
}

fn build_ensemble(params: Value) -> Patch {
    build(
        ProcessingMode::Sample,
        ProcessingMode::Sample,
        "$ensemble",
        params,
    )
}

/// One second of each of `ports` on `channel`, rendered together.
fn render_ports<const N: usize>(patch: &Patch, ports: [&str; N], channel: usize) -> [Vec<f32>; N] {
    let mut out = std::array::from_fn(|_| Vec::new());
    let blocks = SAMPLE_RATE as usize / BLOCK_SIZE;
    for _ in 0..blocks {
        for module in patch.sampleables.values() {
            module.start_block();
        }
        for module in patch.sampleables.values() {
            module.ensure_processed_to(BLOCK_SIZE);
        }
        for index in 0..BLOCK_SIZE {
            for (samples, port) in out.iter_mut().zip(ports) {
                samples.push(patch.sampleables["dut"].get_value_at(port, channel, index));
            }
        }
    }
    out
}

fn render_port(patch: &Patch, port: &str, channel: usize) -> Vec<f32> {
    let [samples] = render_ports(patch, [port], channel);
    samples
}

/// Frequency from rising zero crossings, skipping the onset.
fn frequency(samples: &[f32]) -> f32 {
    let tail = &samples[samples.len() / 4..];
    let crossings = tail
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    crossings as f32 * SAMPLE_RATE / tail.len() as f32
}

fn assert_hz(got: f32, expected: f32) {
    assert!(
        (got - expected).abs() < expected * 0.01 + 4.0,
        "got {got} Hz, expected {expected} Hz"
    );
}

#[test]
fn single_voice_plays_its_root() {
    let patch = build_ensemble(json!({ "pitch": 0.0, "count": 1 }));
    assert_hz(frequency(&render_port(&patch, "output", 0)), C4_HZ);
}

#[test]
fn root_quantizes_to_the_scale() {
    // Hard steps: a root between C and D lands on the nearer note.
    let c_major = [
        0.0,
        semis(2.0),
        semis(4.0),
        semis(5.0),
        semis(7.0),
        semis(9.0),
        semis(11.0),
    ];
    for (root, expected) in [(semis(0.9), 0.0), (semis(1.2), 2.0), (semis(6.6), 7.0)] {
        let patch = build_ensemble(json!({
            "pitch": 0.0, "root": root, "scale": c_major, "count": 1, "crossfade": 0.0,
        }));
        let hz = frequency(&render_port(&patch, "output", 0));
        assert_hz(hz, C4_HZ * 2f32.powf(expected / 12.0));
    }
}

#[test]
fn pitch_transposes_after_quantization() {
    let patch = build_ensemble(json!({
        "pitch": semis(1.0), "scale": [0.0, semis(7.0)], "count": 1,
    }));
    assert_hz(
        frequency(&render_port(&patch, "output", 0)),
        C4_HZ * 2f32.powf(1.0 / 12.0),
    );
}

#[test]
fn the_scale_is_relative_to_its_first_note() {
    // A minor from A3: three semitones up is C4.
    let a3 = semis(-3.0);
    let a_minor: Vec<f32> = [9.0, 11.0, 12.0, 14.0, 16.0, 17.0, 19.0]
        .iter()
        .map(|&n| semis(n))
        .collect();
    let patch = build_ensemble(json!({
        "pitch": a3, "root": semis(3.0), "scale": a_minor, "count": 1, "crossfade": 0.0,
    }));
    assert_hz(frequency(&render_port(&patch, "output", 0)), C4_HZ);
}

#[test]
fn free_presets_repeat_at_their_period() {
    // A Bohlen–Pierce major triad an octave and a half up sits a tritave plus
    // its first interval up: 19.02 + 8.78 semitones.
    let patch = build_ensemble(json!({
        "pitch": semis(-24.0), "root": semis(27.0), "scale": "enosc.bp-major",
        "count": 1, "crossfade": 0.0,
    }));
    assert_hz(
        frequency(&render_port(&patch, "output", 0)),
        C4_HZ * 2f32.powf((-24.0 + 19.019_55 + 8.778_254) / 12.0),
    );
}

#[test]
fn spread_stacks_voices_up_the_scale() {
    // Spread 14/3 is 7 semitones per voice: C then G.
    let patch = build_ensemble(json!({ "pitch": 0.0, "count": 2, "spread": 14.0 / 3.0 }));
    let [root, other] = render_ports(&patch, ["root", "other"], 0);
    assert_hz(frequency(&root), C4_HZ);
    assert_hz(frequency(&other), C4_HZ * 2f32.powf(7.0 / 12.0));
}

#[test]
fn split_outputs_sum_to_the_mix() {
    let patch = build_ensemble(json!({
        "pitch": -1.0, "count": 5, "spread": 2.0, "detune": 2.0, "balance": 2.0,
        "crossFm": 1.0, "fold": 1.0,
    }));
    let [output, odd, even, low, high, root, other] = render_ports(
        &patch,
        ["output", "odd", "even", "low", "high", "root", "other"],
        0,
    );
    // The mix gain for 5 voices at balance 2, where each voice is 2^(4 · 0.4³)
    // louder than the one below. The root output's gain is a fixed 0.7 and the
    // other output's is 0.6 times the mix gain.
    let ratio = 2f32.powf(4.0 * 0.4f32.powi(3));
    let amplitude_sum: f32 = (0..5).map(|i| ratio.powi(i)).sum();
    let mix_gain = (0.5 + (0.5 + 0.007_874_016) / ratio) / amplitude_sum;
    let peak = output.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.1, "mix is silent");
    for i in 0..output.len() {
        assert!(
            (odd[i] + even[i] - output[i]).abs() < 1e-3,
            "odd + even at {i}"
        );
        assert!(
            (low[i] + high[i] - output[i]).abs() < 1e-3,
            "low + high at {i}"
        );
        let rebuilt = root[i] / 0.7 * mix_gain + other[i] / 0.6;
        assert!((rebuilt - output[i]).abs() < 1e-3, "root + other at {i}");
    }
}

#[test]
fn widest_input_sets_the_channel_count() {
    let patch = build_ensemble(json!({
        "pitch": [0.0, 1.0], "spread": [1.0, 2.0, 3.0], "scale": [0.0, 0.1, 0.2, 0.3],
    }));
    let module = &patch.sampleables["dut"];
    assert_eq!(module.port_channels("output"), 3);
}

#[test]
fn outputs_stay_bounded_across_options() {
    for mode in ["up", "chain", "down"] {
        for phase in [
            json!({ "type": "pulsar", "amount": 3.0 }),
            json!({ "type": "feedback", "amount": 5.0 }),
            json!({ "type": "pipe",
                    "first": { "type": "crush", "amount": 2.0 },
                    "second": { "type": "feedback", "amount": 2.0 } }),
        ] {
            let patch = build_ensemble(json!({
                "pitch": poly_cable("src", CHANNELS), "count": 16, "spread": 5.0,
                "detune": 5.0, "crossFm": 5.0, "crossFmMode": mode, "phase": phase,
                "fold": 5.0, "cheby": 5.0, "segment": 5.0, "effectOrder": "scf",
                "balance": -5.0,
            }));
            for sample in render_port(&patch, "output", 1) {
                assert!(
                    sample.is_finite() && sample.abs() < 12.0,
                    "{mode}: {sample}"
                );
            }
        }
    }
}

#[test]
fn rejects_an_unknown_effect_order() {
    let deserializers = modular_core::dsp::get_params_deserializers();
    assert!(deserializers["$ensemble"](json!({ "pitch": 0.0, "effectOrder": "ffc" })).is_err());
}

#[test]
fn allocation_free() {
    for params in [
        json!({ "pitch": 0.0 }),
        json!({
            "pitch": poly_cable("src", CHANNELS), "scale": [0.0, 0.25, 0.5],
            "spread": cable("lfo", 0), "detune": 3.0, "crossFm": cable("lfo", 1),
            "crossFmMode": "chain", "count": 16,
            "phase": { "type": "pipe",
                       "first": { "type": "feedback", "amount": cable("lfo", 2) },
                       "second": { "type": "pulsar", "amount": 2.0 } },
            "fold": 2.0, "cheby": 2.0, "segment": cable("lfo", 3),
        }),
        json!({ "pitch": cable("src", 0), "scale": poly_cable("lfo", CHANNELS), "crossFmMode": "down", "crossFm": 2.0 }),
    ] {
        assert_eq!(allocations_while_rendering("$ensemble", params), 0);
    }
}
