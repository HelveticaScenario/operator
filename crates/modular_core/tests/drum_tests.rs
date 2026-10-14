//! Behavioural tests for the drum voices, built through the public module API.
//!
//! A constant 5 V trigger strikes each drum on its first sample.

use modular_core::dsp::{get_constructors, get_params_deserializers};
use modular_core::params::DeserializedParams;
use modular_core::types::{ProcessingMode, Sampleable};
use serde_json::{Value, json};

const SR: f32 = 48_000.0;

fn make(module_type: &str, params: Value) -> Box<dyn Sampleable> {
    let cached = get_params_deserializers()[module_type](params)
        .unwrap_or_else(|e| panic!("params for {module_type}: {e}"));
    get_constructors()[module_type](
        &"drum".to_string(),
        SR,
        DeserializedParams {
            params: cached.params,
            channel_count: cached.channel_count,
        },
        1,
        ProcessingMode::Block,
    )
    .unwrap_or_else(|e| panic!("construct {module_type}: {e}"))
}

/// Render `secs` of output channel `ch` with `params` plus a held 5 V trigger.
fn render_channel(module_type: &str, mut params: Value, secs: f32, ch: usize) -> Vec<f32> {
    params["trigger"] = json!(5.0);
    let m = make(module_type, params);
    (0..(secs * SR) as usize)
        .map(|_| {
            m.start_block();
            m.ensure_processed();
            m.get_value_at("output", ch, 0)
        })
        .collect()
}

fn render(module_type: &str, params: Value, secs: f32) -> Vec<f32> {
    render_channel(module_type, params, secs, 0)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Mean frequency from upward zero crossings.
fn crossing_hz(x: &[f32]) -> f32 {
    let ups: Vec<usize> = (1..x.len())
        .filter(|&i| x[i - 1] <= 0.0 && x[i] > 0.0)
        .collect();
    (ups.len() - 1) as f32 * SR / (ups[ups.len() - 1] - ups[0]) as f32
}

/// Rough share of energy above `hz`: the power through a one-pole high-pass
/// at `hz` relative to the whole signal. The gentle slope lets some energy
/// below `hz` count, so thresholds leave room for it.
fn energy_above(x: &[f32], hz: f32) -> f32 {
    let a = (-std::f32::consts::TAU * hz / SR).exp();
    let (mut prev_in, mut prev_out) = (0.0f32, 0.0f32);
    let mut high = 0.0;
    for &v in x {
        let out = a * (prev_out + v - prev_in);
        prev_in = v;
        prev_out = out;
        high += out * out;
    }
    high / x.iter().map(|v| v * v).sum::<f32>()
}

/// Share of energy above `hz` through four cascaded one-pole high-passes,
/// steep enough that a loud low body does not leak into it.
fn steep_energy_above(x: &[f32], hz: f32) -> f32 {
    let a = (-std::f32::consts::TAU * hz / SR).exp();
    let mut y = x.to_vec();
    for _ in 0..4 {
        let (mut prev_in, mut prev_out) = (0.0f32, 0.0f32);
        for v in y.iter_mut() {
            let out = a * (prev_out + *v - prev_in);
            prev_in = *v;
            prev_out = out;
            *v = out;
        }
    }
    y.iter().map(|v| v * v).sum::<f32>() / x.iter().map(|v| v * v).sum::<f32>()
}

/// Peak frequency of `x` by a direct DFT scan over `lo..hi` in 1 Hz steps.
fn spectral_peak(x: &[f32], lo: f32, hi: f32) -> f32 {
    let mut best = (lo, 0.0f32);
    let mut f = lo;
    while f < hi {
        let w = std::f32::consts::TAU * f / SR;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, v) in x.iter().enumerate() {
            re += v * (w * i as f32).cos();
            im += v * (w * i as f32).sin();
        }
        let mag = re * re + im * im;
        if mag > best.1 {
            best = (f, mag);
        }
        f += 1.0;
    }
    best.0
}

const DRUMS: [(&str, f32); 7] = [
    ("$tom", 0.5),
    ("$snare", 0.3),
    ("$hihat", 0.2),
    ("$cymbal", 0.5),
    ("$clang", 0.2),
    ("$fmDrum", 0.4),
    ("$clap", 0.3),
];

#[test]
fn every_drum_is_silent_until_struck() {
    for (name, _) in DRUMS {
        let m = make(name, json!({ "trigger": 0.0 }));
        for _ in 0..4800 {
            m.start_block();
            m.ensure_processed();
            assert_eq!(m.get_value_at("output", 0, 0), 0.0, "{name}");
        }
    }
}

#[test]
fn every_drum_sounds_in_range_and_dies_away() {
    for (name, decay) in DRUMS {
        let out = render(name, json!({ "decay": decay }), decay * 4.0 + 0.3);
        assert!(
            out.iter().all(|v| v.is_finite()),
            "{name} produced a non-finite sample"
        );
        let p = peak(&out);
        assert!((0.5..=8.0).contains(&p), "{name} peak {p} V");
        let tail = rms(&out[out.len() - 480..]);
        assert!(
            tail < p * 1e-3,
            "{name} still at {tail} V after {}s",
            decay * 4.0 + 0.3
        );
    }
}

/// Drums whose accent only scales their level, with the level of the softest
/// accent.
const LEVEL_ONLY_ACCENT: [(&str, f32, f32); 4] = [
    ("$hihat", 0.2, 0.0),
    ("$cymbal", 0.5, 0.155),
    ("$clang", 0.2, 0.08),
    ("$fmDrum", 0.4, 0.087),
];

#[test]
fn accent_scales_level_from_each_drums_floor() {
    for (name, decay, floor) in LEVEL_ONLY_ACCENT {
        let full = render(name, json!({ "decay": decay }), 0.1);
        let half = render(name, json!({ "decay": decay, "accent": 2.5 }), 0.1);
        let ratio = peak(&half) / peak(&full);
        let expected = floor + (1.0 - floor) * 0.5;
        assert!(
            (ratio - expected).abs() < 0.01,
            "{name} accent ratio {ratio}, expected {expected}"
        );
    }
}

#[test]
fn soft_accents_dull_the_tom_and_snare() {
    let tom = json!({ "freq": 1.0, "bend": 0.0, "color": 5.0 });
    for (name, params) in [("$tom", tom), ("$snare", json!({}))] {
        let full = render(name, params.clone(), 0.3);
        let mut soft_params = params;
        soft_params["accent"] = json!(0.5);
        let soft = render(name, soft_params, 0.3);
        assert!(rms(&soft) < 0.5 * rms(&full), "{name} soft hit too loud");
        let (bright_full, bright_soft) = (
            steep_energy_above(&full, 2000.0),
            steep_energy_above(&soft, 2000.0),
        );
        assert!(
            bright_soft < 0.7 * bright_full,
            "{name} soft hit not darker: {bright_soft} vs {bright_full}"
        );
    }
}

#[test]
fn clap_has_no_accent() {
    assert!(get_params_deserializers()["$clap"](json!({ "trigger": 0.0, "accent": 5.0 })).is_err());
}

#[test]
fn tom_glides_down_to_its_pitch() {
    let out = render(
        "$tom",
        json!({ "freq": -1.0, "bend": 1.0, "tone": 0.0 }),
        1.2,
    );
    let early = crossing_hz(&out[..2400]);
    let late = crossing_hz(&out[38_400..57_600]);
    let target = 261.6256 / 2.0;
    assert!(early > target * 1.6, "early {early} Hz");
    assert!((late - target).abs() < target * 0.02, "late {late} Hz");
}

#[test]
fn snare_body_sits_at_its_pitch() {
    let out = render(
        "$snare",
        json!({ "freq": -1.0, "tone": 0.0, "color": 0.0 }),
        0.3,
    );
    let hz = crossing_hz(&out[2400..9600]);
    assert!((hz - 130.81).abs() < 3.0, "{hz} Hz");
}

#[test]
fn snare_wires_follow_tone() {
    let dry = render("$snare", json!({ "tone": 0.0 }), 0.2);
    let wet = render("$snare", json!({ "tone": 5.0 }), 0.2);
    assert!(energy_above(&wet, 3000.0) > 0.3);
    assert!(energy_above(&dry, 3000.0) < 0.05);
}

#[test]
fn hihat_is_bright_and_decay_sets_its_length() {
    let closed = render("$hihat", json!({ "decay": 0.05 }), 0.3);
    let open = render("$hihat", json!({ "decay": 0.8 }), 0.3);
    let bright = energy_above(&open, 3000.0);
    assert!(bright > 0.5, "{bright}");
    assert!(rms(&open[9600..14_400]) > 10.0 * rms(&closed[9600..14_400]));
}

#[test]
fn cymbal_energy_is_mostly_high() {
    let out = render("$cymbal", json!({}), 0.5);
    let high = energy_above(&out, 2000.0);
    assert!(high > 0.7, "{high}");
}

#[test]
fn clang_upper_square_sits_above_its_pitch() {
    let out = render("$clang", json!({ "freq": 0.0, "low": 0.0 }), 0.2);
    let hz = spectral_peak(&out[480..4800], 300.0, 500.0);
    assert!((hz - 261.6256 * 1.535).abs() < 4.0, "{hz} Hz");
}

#[test]
fn fm_drum_without_modulation_is_a_sine_at_its_pitch() {
    let out = render(
        "$fmDrum",
        json!({ "freq": 0.0, "feedback": 0.0, "amount": 0.0, "tone": 5.0 }),
        0.2,
    );
    let hz = crossing_hz(&out[480..9600]);
    assert!((hz - 261.6256).abs() < 1.0, "{hz} Hz");
    let high = energy_above(&out[480..9600], 2000.0);
    assert!(high < 0.03, "{high}");
}

#[test]
fn clap_spread_widens_the_stereo_image() {
    let mono_l = render_channel("$clap", json!({ "spread": 0.0 }), 0.2, 0);
    let mono_r = render_channel("$clap", json!({ "spread": 0.0 }), 0.2, 1);
    assert_eq!(mono_l, mono_r);
    let wide_l = render_channel("$clap", json!({ "spread": 5.0 }), 0.2, 0);
    let wide_r = render_channel("$clap", json!({ "spread": 5.0 }), 0.2, 1);
    assert!(
        wide_l
            .iter()
            .zip(&wide_r)
            .any(|(l, r)| (l - r).abs() > 0.01)
    );
}
