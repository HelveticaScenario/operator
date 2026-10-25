//! Shared helpers for the `$tr808.*` and `$tr909.*` behavioural tests: building
//! a voice, rendering it, and measuring what it renders.
//!
//! Each test file uses the parts it needs, so items unused by a given test
//! binary are expected.
#![allow(dead_code)]

use modular_core::Patch;
use modular_core::dsp::utils::rng::TEST_SEED_BASE;
use modular_core::dsp::{get_constructors, get_params_deserializers};
use modular_core::params::DeserializedParams;
use modular_core::types::{ProcessingMode, Sampleable};
use serde_json::{Value, json};

pub const SR: f32 = 48_000.0;

/// Every module seeds its noise from this, so each run draws the same noise.
pub const SEED: u64 = 0x5eed;

/// V/Oct for `hz`, 0 V at C4.
pub fn voct(hz: f32) -> f32 {
    (hz / 261.625_58).log2()
}

pub fn make(module_type: &str, params: Value, sample_rate: f32) -> Box<dyn Sampleable> {
    TEST_SEED_BASE.set(Some(SEED));
    let cached = get_params_deserializers()[module_type](params)
        .unwrap_or_else(|e| panic!("params for {module_type}: {e}"));
    get_constructors()[module_type](
        &"drum".to_string(),
        sample_rate,
        DeserializedParams {
            params: cached.params,
            channel_count: cached.channel_count,
        },
        1,
        ProcessingMode::Block,
    )
    .unwrap_or_else(|e| panic!("construct {module_type}: {e}"))
}

/// Render `secs` of `output` at `sample_rate` with `params`.
pub fn render_from(
    module_type: &str,
    params: Value,
    output: &str,
    secs: f32,
    sample_rate: f32,
) -> Vec<f32> {
    let m = make(module_type, params, sample_rate);
    (0..(secs * sample_rate) as usize)
        .map(|_| {
            m.start_block();
            m.ensure_processed();
            m.get_value_at(output, 0, 0)
        })
        .collect()
}

/// Render `secs` of output at `sample_rate` with `params` plus a held 5 V trigger.
pub fn render_at(module_type: &str, mut params: Value, secs: f32, sample_rate: f32) -> Vec<f32> {
    params["trigger"] = json!(5.0);
    render_from(module_type, params, "output", secs, sample_rate)
}

pub fn render(module_type: &str, params: Value, secs: f32) -> Vec<f32> {
    render_at(module_type, params, secs, SR)
}

/// Render `secs` of `output` with `params` and a `$pulse` clock on each
/// `(port, hz)`, and return it with the sample index of each edge of the
/// first clock.
pub fn render_clocks(
    module_type: &str,
    mut params: Value,
    clocks: &[(&str, f32)],
    output: &str,
    secs: f32,
) -> (Vec<f32>, Vec<usize>) {
    const BLOCK: usize = 64;
    let mut modules = Vec::new();
    for (k, &(port, hz)) in clocks.iter().enumerate() {
        let id = format!("clock{k}");
        params[port] = json!({ "type": "cable", "module": id, "port": "output", "channel": 0 });
        modules.push((id, "$pulse", json!({ "freq": voct(hz) })));
    }
    modules.push(("drum".to_string(), module_type, params));
    let deserializers = get_params_deserializers();
    TEST_SEED_BASE.set(Some(SEED));
    let mut patch = Patch::new();
    patch
        .insert_modules(
            modules.into_iter().map(|(id, module_type, params)| {
                let cached = deserializers[module_type](params).unwrap();
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
        .unwrap();
    patch.connect_all();
    let ids: Vec<String> = (0..clocks.len()).map(|k| format!("clock{k}")).collect();
    let drum = &patch.sampleables["drum"];
    let (mut x, mut strikes, mut prev) = (Vec::new(), Vec::new(), 0.0);
    for _ in 0..(secs * SR) as usize / BLOCK {
        for id in &ids {
            patch.sampleables[id].start_block();
        }
        drum.start_block();
        for id in &ids {
            patch.sampleables[id].ensure_processed_to(BLOCK);
        }
        drum.ensure_processed_to(BLOCK);
        let first = &patch.sampleables[&ids[0]];
        for i in 0..BLOCK {
            let gate = first.get_value_at("output", 0, i);
            if prev < 1.0 && gate >= 1.0 {
                strikes.push(x.len());
            }
            prev = gate;
            x.push(drum.get_value_at(output, 0, i));
        }
    }
    (x, strikes)
}

/// Asserts a voice with `params`, none of which strike it, keeps each of
/// `outputs` at exactly 0 V.
pub fn assert_silent_until_struck(module_type: &str, params: Value, outputs: &[&str]) {
    let m = make(module_type, params, SR);
    for _ in 0..4800 {
        m.start_block();
        m.ensure_processed();
        for output in outputs {
            assert_eq!(m.get_value_at(output, 0, 0), 0.0, "{module_type} {output}");
        }
    }
}

pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

/// Root-mean-square level of `x`.
pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Largest change between neighbouring samples.
pub fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0, |m, w| m.max((w[1] - w[0]).abs()))
}

/// Mean frequency from upward zero crossings between `from` and `to` seconds.
pub fn crossing_hz(x: &[f32], from: f32, to: f32, sample_rate: f32) -> f32 {
    let ups: Vec<usize> = ((from * sample_rate) as usize..(to * sample_rate) as usize)
        .filter(|&i| x[i - 1] <= 0.0 && x[i] > 0.0)
        .collect();
    (ups.len() - 1) as f32 * sample_rate / (ups[ups.len() - 1] - ups[0]) as f32
}

/// Seconds to fall 60 dB, from a log-linear fit of 25 ms block peaks lying
/// between −20 and −60 dB of the hit's peak.
pub fn t60(x: &[f32], sample_rate: f32) -> f32 {
    let block = (0.025 * sample_rate) as usize;
    let top = peak(x);
    let points: Vec<(f32, f32)> = x
        .chunks(block)
        .enumerate()
        .map(|(i, c)| {
            let t = (i as f32 + 0.5) * block as f32 / sample_rate;
            (t, 20.0 * (peak(c) / top).log10())
        })
        .filter(|&(t, db)| t > 0.05 && db < -20.0 && db > -60.0)
        .collect();
    let n = points.len() as f32;
    let (st, sd) = points
        .iter()
        .fold((0.0, 0.0), |(st, sd), &(t, d)| (st + t, sd + d));
    let (mt, md) = (st / n, sd / n);
    let (num, den) = points.iter().fold((0.0, 0.0), |(num, den), &(t, d)| {
        (num + (t - mt) * (d - md), den + (t - mt) * (t - mt))
    });
    -60.0 / (num / den)
}

/// Power of `x` at `hz` (Goertzel), normalised by length.
pub fn power_at(x: &[f32], hz: f32, sample_rate: f32) -> f32 {
    let w = std::f32::consts::TAU * hz / sample_rate;
    let (c, mut s1, mut s2) = (2.0 * w.cos(), 0.0f32, 0.0f32);
    for &v in x {
        let s0 = v + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - c * s1 * s2) / (x.len() * x.len()) as f32
}

/// Share of `x`'s energy that `filter` keeps, run over it four times.
fn energy_through(x: &[f32], mut filter: impl FnMut(&mut [f32])) -> f32 {
    let mut y = x.to_vec();
    for _ in 0..4 {
        filter(&mut y);
    }
    y.iter().map(|v| v * v).sum::<f32>() / x.iter().map(|v| v * v).sum::<f32>()
}

/// Share of energy below `hz` through four cascaded one-pole low-passes.
pub fn energy_below(x: &[f32], hz: f32) -> f32 {
    let a = (-std::f32::consts::TAU * hz / SR).exp();
    energy_through(x, |y| {
        let mut state = 0.0f32;
        for v in y.iter_mut() {
            state = *v + a * (state - *v);
            *v = state;
        }
    })
}

/// Share of energy above `hz` through four cascaded one-pole high-passes.
pub fn energy_above(x: &[f32], hz: f32) -> f32 {
    let a = (-std::f32::consts::TAU * hz / SR).exp();
    energy_through(x, |y| {
        let (mut prev_in, mut prev_out) = (0.0f32, 0.0f32);
        for v in y.iter_mut() {
            let out = a * (prev_out + *v - prev_in);
            prev_in = *v;
            prev_out = out;
            *v = out;
        }
    })
}
