//! Behavioural tests for the TR-808 voices, built through the public module
//! API and checked against the published circuit analysis of each voice.
//!
//! A constant 5 V trigger strikes each drum on its first sample.

use modular_core::Patch;
use modular_core::dsp::get_params_deserializers;
use modular_core::dsp::utils::rng::TEST_SEED_BASE;
use modular_core::params::DeserializedParams;
use modular_core::types::ProcessingMode;
use serde_json::{Value, json};

mod drum_common;

use drum_common::*;

/// Render `secs` of output with `params`, struck by a `$pulse` clock at
/// `hz`, and return it with the sample index of each strike.
fn render_retriggered(
    module_type: &str,
    params: Value,
    hz: f32,
    secs: f32,
) -> (Vec<f32>, Vec<usize>) {
    render_clocks(module_type, params, &[("trigger", hz)], "output", secs)
}

/// Render `secs` of `output` with `params` and a `$pulse` clock on each
/// `(port, hz)`, and return it with the sample index of each edge of the
/// first clock.
fn render_clocks(
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

/// Peak level between consecutive strikes.
fn hit_peaks(x: &[f32], strikes: &[usize]) -> Vec<f32> {
    strikes.windows(2).map(|w| peak(&x[w[0]..w[1]])).collect()
}

/// The median peak on `output` of hits struck from rest by a 4 Hz clock on
/// `port`, which holds still where the noise in a voice makes each hit's
/// peak differ.
fn median_peak(module_type: &str, mut params: Value, port: &str, output: &str) -> f32 {
    params["restart"] = json!(true);
    let (x, strikes) = render_clocks(module_type, params, &[(port, 4.0)], output, 6.0);
    let mut peaks = hit_peaks(&x, &strikes);
    peaks.sort_by(f32::total_cmp);
    peaks[peaks.len() / 2]
}

/// [`median_peak`] of a voice struck on its `trigger`.
fn median_hit_peak(module_type: &str, params: Value) -> f32 {
    median_peak(module_type, params, "trigger", "output")
}

const BD: &str = "$tr808.kick";
/// The stock bridged-T's centre frequency at rest.
const BD_REST_HZ: f32 = 49.45;

/// The kick's ring time, seconds, for its decay control at `volts`: the
/// stock knob's ≈0.09–3.8 s, the same ratio for every volt.
fn kick_decay(volts: f32) -> f32 {
    0.09 * (3.8f32 / 0.09).powf(volts / 5.0)
}

/// The ring time, seconds, of a drum ringing for `stock` at 2.5 V, for its
/// decay control at `volts`: three times shorter or longer at either end.
fn decay_around(volts: f32, stock: f32) -> f32 {
    stock * 3f32.powf((volts - 2.5) / 2.5)
}

#[test]
fn bd_rings_at_the_bridged_t_frequency() {
    let x = render(BD, json!({}), 1.0);
    let hz = crossing_hz(&x, 0.3, 0.8, SR);
    assert!((hz - BD_REST_HZ).abs() < 0.3, "{hz}");
}

#[test]
fn bd_first_trough_follows_the_raised_attack_frequency() {
    // The envelope generator raises the bridged-T to ≈130 Hz for the
    // attack, so the first trough lands near a quarter of that period after
    // the 1 ms pulse rather than a quarter of a 49.5 Hz period.
    let x = render(BD, json!({}), 0.02);
    let trough = (0..x.len()).min_by(|&a, &b| x[a].total_cmp(&x[b])).unwrap();
    let ms = trough as f32 / SR * 1000.0;
    assert!((3.0..4.5).contains(&ms), "{ms}");
}

#[test]
fn bd_pitch_sighs_down_to_rest() {
    let x = render(BD, json!({}), 1.0);
    let early = crossing_hz(&x, 0.005, 0.08, SR);
    let late = crossing_hz(&x, 0.3, 0.8, SR);
    assert!(early > late + 3.0, "early {early} late {late}");
}

#[test]
fn bd_accent_sighs_longer() {
    let soft = render(BD, json!({ "accent": 0.0, "decay": 3.5 }), 0.3);
    let hard = render(BD, json!({ "accent": 5.0, "decay": 3.5 }), 0.3);
    let soft_hz = crossing_hz(&soft, 0.08, 0.15, SR);
    let hard_hz = crossing_hz(&hard, 0.08, 0.15, SR);
    assert!(hard_hz > soft_hz + 1.0, "soft {soft_hz} hard {hard_hz}");
}

#[test]
fn bd_accent_doubles_the_level() {
    let soft = peak(&render(BD, json!({ "accent": 0.0 }), 0.1));
    let hard = peak(&render(BD, json!({ "accent": 5.0 }), 0.1));
    assert!((soft - 5.0).abs() < 0.3, "{soft}");
    assert!((hard - 10.0).abs() < 0.6, "{hard}");
}

#[test]
fn bd_rings_down_at_any_pitch() {
    for freq in [-3.5f32, -1.0, 0.0, 1.0, 2.0, 3.0] {
        let x = render(BD, json!({ "freq": freq, "tone": 5.0, "accent": 5.0 }), 1.5);
        let tail = peak(&x[(1.4 * SR) as usize..]);
        assert!(tail < 0.01, "freq {freq}: {tail}");
    }
}

#[test]
fn bd_decay_sets_the_time_to_minus_60_db() {
    for volts in [1.5, 2.5, 4.5] {
        let decay = kick_decay(volts);
        let x = render(BD, json!({ "decay": volts }), decay * 1.5 + 0.2);
        let measured = t60(&x, SR);
        assert!(
            (measured / decay - 1.0).abs() < 0.05,
            "decay {volts}: {measured}"
        );
    }
}

#[test]
fn bd_decay_stops_at_the_circuit_minimum() {
    // With the feedback buffer at zero gain the bridged-T alone sets the
    // shortest ring, ≈0.09 s at stock pitch.
    let x = render(BD, json!({ "decay": 0.0 }), 0.3);
    let measured = t60(&x, SR);
    assert!((0.07..0.11).contains(&measured), "{measured}");
}

#[test]
fn bd_freq_scales_the_ring_and_keeps_the_decay() {
    for (voct, hz) in [(-1.4f32, 99.14f32), (-3.2, 28.47)] {
        let x = render(BD, json!({ "freq": voct, "decay": 3.5 }), 1.5);
        let measured = crossing_hz(&x, 0.4, 1.0, SR);
        assert!((measured / hz - 1.0).abs() < 0.01, "{voct}: {measured}");
        let decay = t60(&x, SR);
        assert!(
            (decay / kick_decay(3.5) - 1.0).abs() < 0.05,
            "{voct}: {decay}"
        );
    }
}

#[test]
fn bd_tone_lets_through_the_click() {
    let dark = render(BD, json!({ "tone": 0.0 }), 0.05);
    let bright = render(BD, json!({ "tone": 5.0 }), 0.05);
    let (d, b) = (energy_above(&dark, 1000.0), energy_above(&bright, 1000.0));
    assert!(b > 10.0 * d, "dark {d} bright {b}");
}

#[test]
fn bd_settles_to_silence() {
    let x = render(BD, json!({ "decay": 1.5 }), 2.0);
    assert!(x[(1.9 * SR) as usize..].iter().all(|&v| v == 0.0));
}

#[test]
fn bd_matches_across_sample_rates() {
    for sample_rate in [44_100.0, 96_000.0] {
        let x = render_at(BD, json!({ "decay": 3.5 }), 1.0, sample_rate);
        let hz = crossing_hz(&x, 0.3, 0.8, sample_rate);
        assert!((hz - BD_REST_HZ).abs() < 0.3, "{sample_rate}: {hz}");
        let decay = t60(&x, sample_rate);
        assert!(
            (decay / kick_decay(3.5) - 1.0).abs() < 0.05,
            "{sample_rate}: {decay}"
        );
    }
}

#[test]
fn bd_retriggers_add_to_the_ring() {
    // Eighth notes at 120 bpm with the longest decay: each strike lands on a ring
    // still within a few dB of its peak, so hit levels differ.
    let (x, strikes) = render_retriggered(BD, json!({ "decay": 5.0 }), 4.0, 4.0);
    let peaks = hit_peaks(&x, &strikes);
    let (lo, hi) = peaks
        .iter()
        .fold((f32::MAX, 0.0f32), |(lo, hi), &p| (lo.min(p), hi.max(p)));
    assert!(hi > 1.5 * lo, "{peaks:?}");
}

#[test]
fn bd_restart_makes_every_hit_alike() {
    let (x, strikes) = render_retriggered(BD, json!({ "decay": 5.0, "restart": true }), 4.0, 4.0);
    let single = peak(&render(BD, json!({ "decay": 5.0 }), 0.25));
    for p in hit_peaks(&x, &strikes) {
        assert!((p / single - 1.0).abs() < 0.02, "{p} vs {single}");
    }
}

#[test]
fn bd_restart_fades_without_a_click() {
    // Steps at a restarting strike stay within those of a hit from rest.
    let (x, _) = render_retriggered(BD, json!({ "decay": 5.0, "restart": true }), 4.0, 2.0);
    let single = render(BD, json!({ "decay": 5.0 }), 0.25);
    assert!(
        max_step(&x) < 1.1 * max_step(&single),
        "{} vs {}",
        max_step(&x),
        max_step(&single)
    );
}

/// Each tom and conga with its stock resting pitch, Hz.
const TOMS: [(&str, f32); 3] = [
    ("$tr808.lowTom", 86.55),
    ("$tr808.midTom", 134.27),
    ("$tr808.highTom", 182.24),
];
const CONGAS: [(&str, f32); 3] = [
    ("$tr808.lowConga", 191.91),
    ("$tr808.midConga", 276.39),
    ("$tr808.highConga", 404.59),
];

fn toms_and_congas() -> impl Iterator<Item = (&'static str, f32)> {
    TOMS.into_iter().chain(CONGAS)
}

#[test]
fn toms_ring_at_their_bridged_t_frequency() {
    // Window early enough that the toms' noise tail has not taken over.
    for (voice, hz) in toms_and_congas() {
        let x = render(voice, json!({}), 0.3);
        let measured = crossing_hz(&x, 0.05, 0.2, SR);
        assert!((measured / hz - 1.0).abs() < 0.01, "{voice}: {measured}");
    }
}

#[test]
fn toms_accent_doubles_the_level() {
    for (voice, _) in toms_and_congas() {
        let soft = peak(&render(voice, json!({ "accent": 0.0 }), 0.1));
        let hard = peak(&render(voice, json!({ "accent": 5.0 }), 0.1));
        assert!((soft - 5.0).abs() < 0.3, "{voice}: {soft}");
        assert!((9.5..12.5).contains(&hard), "{voice}: {hard}");
    }
}

#[test]
fn toms_accent_starts_sharper() {
    // Harder hits drive the tuning diodes further into conduction, raising
    // the ring's frequency until it settles.
    for (voice, _) in TOMS {
        let soft = crossing_hz(
            &render(voice, json!({ "accent": 0.0 }), 0.1),
            0.005,
            0.05,
            SR,
        );
        let hard = crossing_hz(
            &render(voice, json!({ "accent": 5.0 }), 0.1),
            0.005,
            0.05,
            SR,
        );
        assert!(hard > soft * 1.015, "{voice}: soft {soft} hard {hard}");
    }
}

#[test]
fn congas_decay_sets_the_time_to_minus_60_db() {
    for ((voice, _), stock) in CONGAS.into_iter().zip([0.634, 0.306, 0.306]) {
        for volts in [0.0, 2.5, 5.0] {
            let decay = decay_around(volts, stock);
            let x = render(voice, json!({ "decay": volts }), decay * 1.5 + 0.2);
            let measured = t60(&x, SR);
            assert!(
                (measured / decay - 1.0).abs() < 0.05,
                "{voice} {volts}: {measured}"
            );
        }
    }
}

#[test]
fn toms_noise_outlasts_the_ring() {
    // With the ring cut short, the toms' gated noise still sounds while the
    // congas, which ground it, have fallen silent.
    for (voice, _) in toms_and_congas() {
        let x = render(voice, json!({ "decay": 0.0 }), 0.4);
        let tail = peak(&x[(0.3 * SR) as usize..]);
        if TOMS.iter().any(|(t, _)| *t == voice) {
            assert!(tail > 0.005, "{voice}: {tail}");
        } else {
            assert!(tail < 0.002, "{voice}: {tail}");
        }
    }
}

#[test]
fn congas_freq_scales_the_ring() {
    for (voice, hz) in CONGAS {
        let x = render(voice, json!({ "freq": voct(hz) + 0.5 }), 0.5);
        let measured = crossing_hz(&x, 0.1, 0.4, SR);
        assert!(
            (measured / (hz * 2f32.sqrt()) - 1.0).abs() < 0.01,
            "{voice}: {measured}"
        );
    }
}

#[test]
fn toms_ring_down_at_any_pitch() {
    for (voice, hz) in toms_and_congas() {
        let stock = voct(hz);
        for offset in [-2.5f32, 0.0, 1.5, 3.0] {
            let x = render(voice, json!({ "freq": stock + offset, "accent": 5.0 }), 3.0);
            let tail = peak(&x[(2.8 * SR) as usize..]);
            assert!(tail < 0.01, "{voice} {offset}: {tail}");
        }
    }
}

#[test]
fn toms_settle_to_silence() {
    for (voice, _) in toms_and_congas() {
        let x = render(voice, json!({ "decay": 0.0 }), 4.0);
        assert!(
            x[(3.9 * SR) as usize..].iter().all(|&v| v == 0.0),
            "{voice}"
        );
    }
}

#[test]
fn toms_match_across_sample_rates() {
    for (voice, hz) in toms_and_congas() {
        let at48 = peak(&render(voice, json!({}), 0.1));
        for sample_rate in [44_100.0, 96_000.0] {
            let x = render_at(voice, json!({}), 0.3, sample_rate);
            let measured = crossing_hz(&x, 0.05, 0.2, sample_rate);
            assert!(
                (measured / hz - 1.0).abs() < 0.01,
                "{voice} {sample_rate}: {measured}"
            );
            assert!(
                (peak(&x) / at48 - 1.0).abs() < 0.05,
                "{voice} {sample_rate}: {}",
                peak(&x)
            );
        }
    }
}

#[test]
fn low_conga_restart_makes_every_hit_alike() {
    let (x, strikes) = render_retriggered(
        "$tr808.lowConga",
        json!({ "decay": 5.0, "restart": true }),
        4.0,
        3.0,
    );
    let single = peak(&render("$tr808.lowConga", json!({ "decay": 5.0 }), 0.25));
    for p in hit_peaks(&x, &strikes) {
        assert!((p / single - 1.0).abs() < 0.02, "{p} vs {single}");
    }
}

const SD: &str = "$tr808.snare";
/// The snare's two resonators as built, Hz.
const SD_LOW_HZ: f32 = 249.6;
const SD_HIGH_HZ: f32 = 499.0;

#[test]
fn sd_tone_crossfades_the_resonators() {
    let low = render(SD, json!({ "tone": 0.0, "snappy": 0.0 }), 0.1);
    let hz = crossing_hz(&low, 0.01, 0.08, SR);
    assert!((hz / SD_LOW_HZ - 1.0).abs() < 0.01, "low {hz}");
    let high = render(SD, json!({ "tone": 5.0, "snappy": 0.0 }), 0.1);
    let hz = crossing_hz(&high, 0.002, 0.012, SR);
    assert!((hz / SD_HIGH_HZ - 1.0).abs() < 0.03, "high {hz}");
}

#[test]
fn sd_accent_doubles_the_level() {
    let soft = peak(&render(SD, json!({ "accent": 0.0 }), 0.1));
    let hard = peak(&render(SD, json!({ "accent": 5.0 }), 0.1));
    assert!((soft - 5.0).abs() < 0.4, "{soft}");
    assert!((9.0..12.0).contains(&hard), "{hard}");
}

#[test]
fn sd_snappy_adds_noise() {
    let dry = render(SD, json!({ "snappy": 0.0 }), 0.1);
    let wet = render(SD, json!({ "snappy": 5.0 }), 0.1);
    let (d, w) = (energy_above(&dry, 3000.0), energy_above(&wet, 3000.0));
    assert!(w > 20.0 * d, "dry {d} wet {w}");
}

#[test]
fn sd_freq_scales_both_resonators() {
    let x = render(
        SD,
        json!({ "tone": 0.0, "snappy": 0.0, "freq": voct(SD_LOW_HZ) + 1.0 }),
        0.1,
    );
    let hz = crossing_hz(&x, 0.01, 0.06, SR);
    assert!((hz / (2.0 * SD_LOW_HZ) - 1.0).abs() < 0.01, "{hz}");
}

#[test]
fn sd_low_resonator_rings_its_stock_decay() {
    // R197's loss through both arms: 2·R·C / 2 → T60 = ln(1000)·820 kΩ·27 nF.
    let x = render(SD, json!({ "tone": 0.0, "snappy": 0.0 }), 0.4);
    let measured = t60(&x, SR);
    assert!((measured / 0.1529 - 1.0).abs() < 0.08, "{measured}");
}

#[test]
fn sd_settles_to_silence() {
    let x = render(SD, json!({ "accent": 5.0, "snappy": 5.0 }), 4.0);
    assert!(x[(3.9 * SR) as usize..].iter().all(|&v| v == 0.0));
}

#[test]
fn sd_matches_across_sample_rates() {
    let at48 = peak(&render(SD, json!({ "snappy": 0.0 }), 0.1));
    for sample_rate in [44_100.0, 96_000.0] {
        let x = render_at(SD, json!({ "tone": 0.0, "snappy": 0.0 }), 0.1, sample_rate);
        let hz = crossing_hz(&x, 0.01, 0.08, sample_rate);
        assert!((hz / SD_LOW_HZ - 1.0).abs() < 0.01, "{sample_rate}: {hz}");
        let p = peak(&render_at(SD, json!({ "snappy": 0.0 }), 0.1, sample_rate));
        assert!(
            (p / at48 - 1.0).abs() < 0.05,
            "{sample_rate}: {p} vs {at48}"
        );
    }
}

const RS: &str = "$tr808.rimShot";
const CL: &str = "$tr808.claves";
/// The claves' resonator as built, Hz.
const CL_HZ: f32 = 2529.0;

#[test]
fn rs_accent_raises_the_level() {
    let soft = peak(&render(RS, json!({ "accent": 0.0 }), 0.05));
    let hard = peak(&render(RS, json!({ "accent": 5.0 }), 0.05));
    assert!((soft - 5.0).abs() < 0.3, "{soft}");
    assert!((8.0..11.0).contains(&hard), "{hard}");
}

#[test]
fn rs_is_a_short_bright_crack() {
    let x = render(RS, json!({}), 0.1);
    let after = peak(&x[(0.03 * SR) as usize..]);
    assert!(after < 0.01 * peak(&x), "{after}");
    // Most of it sits above the Sallen-Key high-pass's 590 Hz corner.
    let bright = energy_above(&x, 500.0);
    assert!(bright > 0.5, "{bright}");
}

#[test]
fn rs_settles_to_silence() {
    let x = render(RS, json!({ "accent": 5.0 }), 3.0);
    assert!(x[(2.9 * SR) as usize..].iter().all(|&v| v == 0.0));
}

#[test]
fn rs_matches_across_sample_rates() {
    let at48 = peak(&render(RS, json!({}), 0.05));
    for sample_rate in [44_100.0, 96_000.0] {
        let p = peak(&render_at(RS, json!({}), 0.05, sample_rate));
        assert!((p / at48 - 1.0).abs() < 0.1, "{sample_rate}: {p} vs {at48}");
    }
}

#[test]
fn cl_rings_at_its_resonance() {
    let x = render(CL, json!({}), 0.1);
    let hz = crossing_hz(&x, 0.002, 0.05, SR);
    assert!((hz / CL_HZ - 1.0).abs() < 0.01, "{hz}");
}

#[test]
fn cl_accent_raises_the_level() {
    let soft = peak(&render(CL, json!({ "accent": 0.0 }), 0.05));
    let hard = peak(&render(CL, json!({ "accent": 5.0 }), 0.05));
    assert!((soft - 5.0).abs() < 0.3, "{soft}");
    assert!((9.0..13.5).contains(&hard), "{hard}");
}

#[test]
fn cl_decay_sets_the_time_to_minus_60_db() {
    for volts in [2.5, 5.0] {
        let decay = decay_around(volts, 0.075);
        let x = render(CL, json!({ "decay": volts }), decay * 1.5 + 0.2);
        let measured = t60(&x, SR);
        assert!((measured / decay - 1.0).abs() < 0.05, "{volts}: {measured}");
    }
}

#[test]
fn cl_freq_scales_the_ring() {
    let x = render(CL, json!({ "freq": voct(CL_HZ) - 1.0 }), 0.1);
    let hz = crossing_hz(&x, 0.002, 0.05, SR);
    assert!((hz / (0.5 * CL_HZ) - 1.0).abs() < 0.01, "{hz}");
}

#[test]
fn cl_settles_to_silence() {
    let x = render(CL, json!({}), 2.0);
    assert!(x[(1.9 * SR) as usize..].iter().all(|&v| v == 0.0));
}

const CB: &str = "$tr808.cowbell";

#[test]
fn cb_sounds_its_two_oscillators() {
    let x = render(CB, json!({}), 0.2);
    let between = power_at(&x, 670.0, SR);
    for hz in [540.0, 800.0] {
        let p = power_at(&x, hz, SR);
        assert!(p > 20.0 * between, "{hz}: {p} vs {between}");
    }
}

#[test]
fn cb_freq_scales_both_oscillators() {
    let x = render(CB, json!({ "freq": voct(1080.0) }), 0.2);
    let between = power_at(&x, 1340.0, SR);
    for hz in [1080.0, 1600.0] {
        let p = power_at(&x, hz, SR);
        assert!(p > 20.0 * between, "{hz}: {p} vs {between}");
    }
}

#[test]
fn cb_drops_fast_then_rings_on() {
    // R82 and C34 pull the envelope down hard over the first milliseconds;
    // C9 then drains slowly through the VCAs.
    let x = render(CB, json!({}), 0.3);
    let at = |t: f32| peak(&x[(t * SR) as usize..((t + 0.005) * SR) as usize]);
    let early = at(0.0) / at(0.02);
    let late = at(0.1) / at(0.12);
    assert!(early > 2.5, "{early}");
    assert!(late < 1.5, "{late}");
}

#[test]
fn cb_accent_raises_the_level() {
    let soft = peak(&render(CB, json!({ "accent": 0.0 }), 0.05));
    let hard = peak(&render(CB, json!({ "accent": 5.0 }), 0.05));
    assert!((soft - 5.0).abs() < 0.3, "{soft}");
    assert!((7.5..11.0).contains(&hard), "{hard}");
}

#[test]
fn cb_settles_to_silence() {
    let x = render(CB, json!({ "accent": 5.0 }), 3.0);
    assert!(x[(2.9 * SR) as usize..].iter().all(|&v| v == 0.0));
}

#[test]
fn cb_matches_across_sample_rates() {
    let at48 = peak(&render(CB, json!({}), 0.05));
    for sample_rate in [44_100.0, 96_000.0] {
        let p = peak(&render_at(CB, json!({}), 0.05, sample_rate));
        assert!((p / at48 - 1.0).abs() < 0.1, "{sample_rate}: {p} vs {at48}");
    }
}

const CY: &str = "$tr808.cymbal";

/// Energy of `x` between `from` and `to` seconds.
fn energy(x: &[f32], from: f32, to: f32) -> f32 {
    x[(from * SR) as usize..(to * SR) as usize]
        .iter()
        .map(|v| v * v)
        .sum()
}

#[test]
fn cy_accent_raises_the_level() {
    let soft = peak(&render(CY, json!({ "accent": 0.0 }), 0.1));
    let hard = peak(&render(CY, json!({ "accent": 5.0 }), 0.1));
    assert!((soft - 5.0).abs() < 0.5, "{soft}");
    assert!((9.0..13.0).contains(&hard), "{hard}");
}

#[test]
fn cy_is_bright_and_metallic() {
    // The two band-passes sit near 3.4 and 6.9 kHz; almost nothing is left
    // below 1 kHz.
    let x = render(CY, json!({}), 0.5);
    let dark = energy_below(&x, 1000.0);
    assert!(dark < 0.01, "{dark}");
}

#[test]
fn cy_decay_lengthens_the_tail() {
    let short = render(CY, json!({ "decay": 0.0 }), 1.2);
    let long = render(CY, json!({ "decay": 5.0 }), 1.2);
    let (s, l) = (energy(&short, 0.8, 1.2), energy(&long, 0.8, 1.2));
    assert!(l > 4.0 * s, "short {s} long {l}");
}

#[test]
fn cy_tone_brightens() {
    let dark = render(CY, json!({ "tone": 0.0 }), 0.3);
    let bright = render(CY, json!({ "tone": 5.0 }), 0.3);
    let (d, b) = (energy_above(&dark, 8000.0), energy_above(&bright, 8000.0));
    assert!(b > 1.5 * d, "dark {d} bright {b}");
}

#[test]
fn cy_settles_to_silence() {
    let x = render(CY, json!({ "decay": 0.0 }), 6.0);
    assert!(x[(5.9 * SR) as usize..].iter().all(|&v| v == 0.0));
}

#[test]
fn cy_matches_across_sample_rates() {
    // Compared by rms: a bright attack's sampled peak depends on the rate.
    let at48 = rms(&render(CY, json!({}), 0.3));
    for sample_rate in [44_100.0, 96_000.0] {
        let r = rms(&render_at(CY, json!({}), 0.3, sample_rate));
        assert!((r / at48 - 1.0).abs() < 0.1, "{sample_rate}: {r} vs {at48}");
    }
}

const HH: &str = "$tr808.hiHat";

/// The hats' `output`s, each with the params that strike it alone.
const HATS: [(&str, &str); 2] = [("closed", "open"), ("open", "closed")];

/// Render `secs` of the hi-hat's `output` with `params`, which name the
/// hat inputs to strike.
fn render_hat(params: Value, output: &str, secs: f32) -> Vec<f32> {
    render_hat_at(params, output, secs, SR)
}

fn render_hat_at(mut params: Value, output: &str, secs: f32, sample_rate: f32) -> Vec<f32> {
    if params.get("closed").is_none() {
        params["closed"] = json!(0.0);
    }
    render_from(HH, params, output, secs, sample_rate)
}

/// `hat` struck by a held trigger, the other hat idle, with `params`.
fn one_hat(hat: &str, mut params: Value) -> Value {
    params[hat] = json!(5.0);
    params
}

/// The median peak of `hat`'s hits struck from rest by a 4 Hz clock.
fn median_hat_peak(hat: &str, mut params: Value) -> f32 {
    if hat == "open" {
        params["closed"] = json!(0.0);
    }
    median_peak(HH, params, hat, hat)
}

#[test]
fn each_hat_sounds_on_its_own_output() {
    for (hat, other) in HATS {
        let x = render_hat(one_hat(hat, json!({})), hat, 0.1);
        let quiet = render_hat(one_hat(hat, json!({})), other, 0.1);
        assert!(peak(&x) > 1.0 && peak(&quiet) == 0.0, "{hat}");
    }
}

#[test]
fn the_output_is_both_hats() {
    let params = json!({ "closed": 5.0, "open": 5.0, "cut": -5.0 });
    let (both, closed, open) = (
        render_hat(params.clone(), "output", 0.2),
        render_hat(params.clone(), "closed", 0.2),
        render_hat(params, "open", 0.2),
    );
    for ((b, c), o) in both.iter().zip(&closed).zip(&open) {
        assert_eq!(*b, c + o);
    }
    assert!(peak(&closed) > 1.0 && peak(&open) > 1.0);
}

#[test]
fn hats_accent_doubles_the_level() {
    for (hat, _) in HATS {
        let soft = median_hat_peak(hat, json!({ "accent": 0.0 }));
        let hard = median_hat_peak(hat, json!({ "accent": 5.0 }));
        assert!((soft - 5.0).abs() < 0.5, "{hat} {soft}");
        assert!((9.0..11.5).contains(&hard), "{hat} {hard}");
    }
}

#[test]
fn hats_are_bright() {
    // The upper band sits near 6.9 kHz, and the high-passes cut below 8 kHz.
    for (hat, _) in HATS {
        let dark = energy_below(&render_hat(one_hat(hat, json!({})), hat, 0.1), 2000.0);
        assert!(dark < 0.01, "{hat} {dark}");
    }
}

#[test]
fn closed_hat_is_short() {
    // The service notes put the closed hat's decay near 50 ms.
    let x = render_hat(one_hat("closed", json!({})), "closed", 0.3);
    assert!(peak(&x[(0.1 * SR) as usize..]) < 0.01 * peak(&x));
    assert!(peak(&x[(0.02 * SR) as usize..(0.03 * SR) as usize]) > 0.1 * peak(&x));
}

#[test]
fn decay_sets_how_long_the_open_hat_holds() {
    // The service notes give 90 ms to 600 ms over the decay pot's travel.
    let end = |decay: f32| {
        let x = render_hat(one_hat("open", json!({ "decay": decay })), "open", 1.5);
        let p = peak(&x);
        x.iter().rposition(|v| v.abs() > 0.01 * p).unwrap() as f32 / SR
    };
    let (short, mid, long) = (end(0.0), end(2.5), end(5.0));
    assert!((0.08..0.2).contains(&short), "{short}");
    assert!((0.3..0.55).contains(&mid), "{mid}");
    assert!((0.5..0.9).contains(&long), "{long}");
}

/// The open hat struck at 0 s with a closed hit at once, at `cut`.
fn cut_open_hat(cut: f32) -> Vec<f32> {
    let params = json!({ "open": 5.0, "decay": 5.0, "cut": cut });
    let (x, edges) = render_clocks(HH, params, &[("closed", 1.0)], "open", 0.4);
    assert_eq!(edges.first(), Some(&0));
    x
}

#[test]
fn a_closed_hit_cuts_off_the_open_hat() {
    // A closed hit at the strike leaves C63 up long enough for Q23 to empty
    // the open hat's envelope.
    let open = render_hat(one_hat("open", json!({})), "open", 0.4);
    let closed = cut_open_hat(0.0);
    let tail = |x: &[f32]| peak(&x[(0.1 * SR) as usize..]);
    assert!(tail(&open) > 0.5 * peak(&open), "{}", tail(&open));
    assert!(tail(&closed) < 0.01 * peak(&open), "{}", tail(&closed));
}

#[test]
fn cut_bends_how_hard_a_closed_hit_cuts() {
    let ring = |x: &[f32]| rms(&x[(0.1 * SR) as usize..(0.2 * SR) as usize]);
    let open = ring(&render_hat(
        one_hat("open", json!({ "decay": 5.0 })),
        "open",
        0.4,
    ));
    let (none, partial, stock) = (
        ring(&cut_open_hat(-5.0)),
        ring(&cut_open_hat(-2.5)),
        ring(&cut_open_hat(0.0)),
    );
    assert!((none / open - 1.0).abs() < 0.1, "{none} {open}");
    assert!((0.05..0.8).contains(&(partial / open)), "{partial} {open}");
    assert!(stock < 0.002 * open, "{stock} {open}");
    // Above stock the cut is sharper.
    let gone_by = |x: &[f32]| {
        let p = peak(x);
        x.iter().rposition(|v| v.abs() > 0.01 * p).unwrap() as f32 / SR
    };
    let (stock, sharp) = (gone_by(&cut_open_hat(0.0)), gone_by(&cut_open_hat(5.0)));
    assert!(sharp < 0.01 && stock > 0.03, "{sharp} {stock}");
}

#[test]
fn a_closed_hit_lets_go_of_later_open_hits() {
    // A closed hit at 0 s cuts that open hit; by the open hit at 0.5 s, C63
    // has emptied below Q23's turn-on and the hat rings as if never cut.
    let hits = |clocks: &[(&str, f32)]| {
        let params = json!({ "closed": 0.0, "decay": 5.0 });
        let (x, edges) = render_clocks(HH, params, clocks, "open", 0.95);
        assert!(edges[0] == 0 && edges[1].abs_diff((0.5 * SR) as usize) <= 1);
        x[edges[1]..].to_vec()
    };
    let open = hits(&[("open", 2.0)]);
    let closed = hits(&[("open", 2.0), ("closed", 1.0)]);
    let ring = |x: &[f32]| rms(&x[(0.1 * SR) as usize..(0.4 * SR) as usize]);
    let (open, closed) = (ring(&open), ring(&closed));
    assert!((closed / open - 1.0).abs() < 0.1, "{closed} {open}");
}

#[test]
fn hats_settle_to_silence() {
    for (hat, _) in HATS {
        let x = render_hat(one_hat(hat, json!({ "decay": 5.0 })), "output", 2.0);
        assert!(x[(1.9 * SR) as usize..].iter().all(|&v| v == 0.0), "{hat}");
    }
}

#[test]
fn hats_match_across_sample_rates() {
    for (hat, _) in HATS {
        let params = one_hat(hat, json!({}));
        let at48 = rms(&render_hat(params.clone(), hat, 0.1));
        for sample_rate in [44_100.0, 96_000.0] {
            let r = rms(&render_hat_at(params.clone(), hat, 0.1, sample_rate));
            assert!(
                (r / at48 - 1.0).abs() < 0.15,
                "{hat} {sample_rate}: {r} vs {at48}"
            );
        }
    }
}

const CP: &str = "$tr808.clap";
const MA: &str = "$tr808.maracas";

/// Peak of `x` in each millisecond.
fn peaks_per_ms(x: &[f32]) -> Vec<f32> {
    x.chunks((SR / 1000.0) as usize).map(peak).collect()
}

#[test]
fn cp_and_ma_accent_doubles_the_level() {
    for voice in [CP, MA] {
        let soft = peak(&render(voice, json!({ "accent": 0.0 }), 0.2));
        let hard = peak(&render(voice, json!({ "accent": 5.0 }), 0.2));
        assert!((soft - 5.0).abs() < 1.0, "{voice} {soft}");
        assert!((8.0..13.0).contains(&hard), "{voice} {hard}");
    }
}

#[test]
fn cp_claps_three_times_then_rings() {
    // C144 empties at the strike and again each time it charges past
    // IC23d's threshold, about every 13 ms while IC23b's window lasts; the
    // window's close empties it once more for the last clap.
    let p = peaks_per_ms(&render(CP, json!({}), 0.2));
    let rises: Vec<usize> = (1..45)
        .filter(|&ms| p[ms] > 2.0 * p[ms - 1].max(0.2))
        .collect();
    let onsets: Vec<usize> = rises
        .iter()
        .copied()
        .filter(|&ms| !rises.contains(&(ms - 1)))
        .collect();
    assert!(onsets.len() >= 2, "{onsets:?} {p:?}");
    for (ms, want) in onsets.iter().zip([13usize, 27]) {
        assert!(ms.abs_diff(want) <= 2, "{onsets:?}");
    }
    // The last clap's tail lasts some 100 ms.
    let tail = peak(&p[60..80]);
    assert!(
        tail > 0.1 * peak(&p) && peak(&p[150..]) < 0.05 * peak(&p),
        "{p:?}"
    );
}

#[test]
fn cp_is_band_passed_noise() {
    // IC21's band-pass sits near 1.25 kHz; C150 cuts below 1.6 kHz.
    let x = render(CP, json!({}), 0.1);
    assert!(energy_below(&x, 300.0) < 0.05);
    assert!(energy_above(&x, 8000.0) < 0.2);
}

#[test]
fn ma_is_a_short_hiss() {
    // The service notes put the maracas' decay near 25 ms; Q68's high-pass
    // sits near 10.6 kHz.
    let x = render(MA, json!({}), 0.2);
    let p = peaks_per_ms(&x);
    assert!(peak(&p[60..]) < 0.05 * peak(&p), "{p:?}");
    assert!(
        energy_below(&x, 2000.0) < 0.05,
        "{}",
        energy_below(&x, 2000.0)
    );
}

#[test]
fn cp_and_ma_settle_to_silence() {
    for voice in [CP, MA] {
        let x = render(voice, json!({}), 4.0);
        assert!(
            x[(3.9 * SR) as usize..].iter().all(|&v| v == 0.0),
            "{voice}"
        );
    }
}

#[test]
fn cp_and_ma_match_across_sample_rates() {
    for voice in [CP, MA] {
        let at48 = rms(&render(voice, json!({}), 0.1));
        for sample_rate in [44_100.0, 96_000.0] {
            let r = rms(&render_at(voice, json!({}), 0.1, sample_rate));
            assert!(
                (r / at48 - 1.0).abs() < 0.2,
                "{voice} {sample_rate}: {r} vs {at48}"
            );
        }
    }
}

#[test]
fn channels_added_by_a_patch_update_get_their_own_noise() {
    let toms = TOMS.map(|(voice, _)| voice);
    for voice in [SD, CP, MA, CY, HH, CB].into_iter().chain(toms) {
        let trigger = if voice == HH { "closed" } else { "trigger" };
        let old = make(voice, json!({ trigger: 5.0 }), SR);
        old.on_patch_update();
        for _ in 0..480 {
            old.start_block();
            old.ensure_processed();
        }
        let new = make(voice, json!({ trigger: [5.0, 5.0, 5.0] }), SR);
        new.transfer_state_from(old.as_ref());
        new.on_patch_update();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for _ in 0..(0.2 * SR) as usize {
            new.start_block();
            new.ensure_processed();
            a.push(new.get_value_at("output", 1, 0));
            b.push(new.get_value_at("output", 2, 0));
        }
        assert!(peak(&a) > 0.1, "{voice}");
        let diff: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
        assert!(peak(&diff) > 0.01 * peak(&a), "{voice} {}", peak(&diff));
    }
}

#[test]
fn pitched_voices_hold_their_level_across_freq() {
    let ranges = [
        (BD, -3.5, -1.0),
        (SD, -1.5, 1.5),
        ("$tr808.lowTom", -3.1, -0.1),
        ("$tr808.midTom", -2.46, 0.54),
        ("$tr808.highTom", -2.02, 0.98),
        ("$tr808.lowConga", -1.95, 1.05),
        ("$tr808.midConga", -1.42, 1.58),
        ("$tr808.highConga", -0.87, 2.13),
        (RS, 1.5, 4.0),
        (CL, 1.5, 4.5),
        (CB, -0.5, 2.5),
    ];
    for (voice, lo, hi) in ranges {
        for freq in [lo, (lo + hi) / 2.0, hi] {
            let params = json!({ "freq": freq });
            // Noise in the snare and the cowbell's free-running oscillators
            // make each of their hits peak differently.
            let level = if [SD, CB].contains(&voice) {
                median_hit_peak(voice, params)
            } else {
                peak(&render(voice, params, 0.15))
            };
            assert!((level - 5.0).abs() < 0.6, "{voice} at {freq}: {level}");
        }
    }
}

#[test]
fn retriggers_stay_within_the_rails() {
    let loud = json!({ "accent": 5.0, "decay": 5.0, "tone": 5.0 });
    let (x, _) = render_retriggered(BD, loud, 16.0, 1.0);
    let top = peak(&x);
    assert!((10.0..=12.0).contains(&top), "{top}");
}

#[test]
fn a_restarting_hit_peaks_no_higher_than_a_fresh_one() {
    let voice = "$tr808.lowConga";
    let params = json!({ "accent": 5.0, "decay": 5.0, "restart": true });
    let fresh = peak(&render(voice, params.clone(), 0.1));
    let (x, strikes) = render_retriggered(voice, params, 16.0, 1.0);
    for p in hit_peaks(&x, &strikes) {
        assert!(p <= fresh * 1.001, "{p} over {fresh}");
    }
}

#[test]
fn voices_are_silent_until_struck() {
    let toms = toms_and_congas().map(|(voice, _)| voice);
    for voice in [BD, SD, RS, CL, CB, CY, CP, MA].into_iter().chain(toms) {
        assert_silent_until_struck(voice, json!({ "trigger": 0.0 }), &["output"]);
    }
    let hats = json!({ "closed": 0.0, "open": 0.0 });
    assert_silent_until_struck(HH, hats, &["output", "closed", "open"]);
}
