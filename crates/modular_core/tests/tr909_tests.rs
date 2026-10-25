//! Behavioural tests for the TR-909 analog voices, built through the public
//! module API and checked against the 909 service notes' circuit and its
//! description of each voice.
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

const BD: &str = "$tr909.kick";
const SD: &str = "$tr909.snare";
const LT: &str = "$tr909.lowTom";
const MT: &str = "$tr909.midTom";
const HT: &str = "$tr909.highTom";
const RS: &str = "$tr909.rimShot";
const CP: &str = "$tr909.clap";
const ALL: [&str; 7] = [BD, SD, LT, MT, HT, RS, CP];

/// The median peak of hits on a voice at rest: one on each of `channels`
/// channels, which each draw their own noise, struck once and again after
/// the first hit has settled.
fn median_hit_peak(module_type: &str, mut params: Value, channels: usize) -> f32 {
    const BLOCK: usize = 64;
    let clock = json!({ "type": "cable", "module": "clock", "port": "output", "channel": 0 });
    params["trigger"] = json!(vec![clock; channels]);
    let modules = [
        ("clock", "$pulse", json!({ "freq": voct(1.0) })),
        ("drum", module_type, params),
    ];
    let deserializers = get_params_deserializers();
    TEST_SEED_BASE.set(Some(SEED));
    let mut patch = Patch::new();
    patch
        .insert_modules(
            modules.into_iter().map(|(id, module_type, params)| {
                let cached = deserializers[module_type](params).unwrap();
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
        .unwrap();
    patch.connect_all();
    let (clock, drum) = (&patch.sampleables["clock"], &patch.sampleables["drum"]);
    let mut peaks = vec![0.0f32; channels];
    let (mut edges, mut prev) = (0, 0.0);
    for _ in 0..(1.3 * SR) as usize / BLOCK {
        clock.start_block();
        drum.start_block();
        clock.ensure_processed_to(BLOCK);
        drum.ensure_processed_to(BLOCK);
        for i in 0..BLOCK {
            let gate = clock.get_value_at("output", 0, i);
            edges += (prev < 1.0 && gate >= 1.0) as usize;
            prev = gate;
            if edges == 2 {
                for (ch, p) in peaks.iter_mut().enumerate() {
                    *p = p.max(drum.get_value_at("output", ch, i).abs());
                }
            }
        }
    }
    peaks.sort_by(f32::total_cmp);
    peaks[channels / 2]
}

/// Seconds at `t` of window `w`.
fn window(x: &[f32], t: f32, w: f32) -> &[f32] {
    &x[(t * SR) as usize..((t + w) * SR) as usize]
}

/// Seconds until the output last stands above `fraction` of its peak.
fn ring(x: &[f32], fraction: f32) -> f32 {
    let p = peak(x);
    x.iter().rposition(|v| v.abs() > fraction * p).unwrap_or(0) as f32 / SR
}

#[test]
fn voices_are_silent_until_struck() {
    for voice in ALL {
        assert_silent_until_struck(voice, json!({ "trigger": 0.0 }), &["output"]);
    }
}

#[test]
fn accent_doubles_the_level() {
    for voice in ALL {
        // Noise makes each kick, snare and clap peak differently.
        let channels = if [BD, SD, CP].contains(&voice) { 8 } else { 1 };
        let soft = median_hit_peak(voice, json!({ "accent": 0.0 }), channels);
        let hard = median_hit_peak(voice, json!({ "accent": 5.0 }), channels);
        assert!((soft - 5.0).abs() < 0.75, "{voice} {soft}");
        assert!((hard - 10.0).abs() < 1.5, "{voice} {hard}");
    }
}

#[test]
fn voices_settle_to_silence() {
    for voice in ALL {
        let params = if [BD, LT, MT, HT].contains(&voice) {
            json!({ "decay": 5.0 })
        } else {
            json!({})
        };
        let x = render(voice, params, 3.0);
        assert!(
            x[(2.9 * SR) as usize..].iter().all(|&v| v == 0.0),
            "{voice}"
        );
    }
}

#[test]
fn voices_match_across_sample_rates() {
    for voice in ALL {
        let at48 = rms(&render(voice, json!({}), 0.2));
        for sample_rate in [44_100.0, 96_000.0] {
            let r = rms(&render_at(voice, json!({}), 0.2, sample_rate));
            assert!(
                (r / at48 - 1.0).abs() < 0.2,
                "{voice} {sample_rate}: {r} vs {at48}"
            );
        }
    }
}

#[test]
fn bd_sweeps_down_to_rest() {
    // IC12a's offset through R27 sets the rest near 56 Hz; the pitch
    // envelope starts it far higher.
    let x = render(BD, json!({ "attack": 0.0, "decay": 5.0 }), 0.4);
    let early = crossing_hz(&x, 0.003, 0.012, SR);
    let rest = crossing_hz(&x, 0.15, 0.3, SR);
    assert!(early > 130.0, "{early}");
    assert!((rest - 56.0).abs() < 3.0, "{rest}");
}

#[test]
fn bd_sweep_lengthens_the_sweep() {
    let hz = |sweep: f32| {
        let x = render(BD, json!({ "attack": 0.0, "sweep": sweep }), 0.1);
        crossing_hz(&x, 0.03, 0.06, SR)
    };
    let (short, long) = (hz(0.0), hz(5.0));
    assert!(long > short + 20.0, "{short} {long}");
}

#[test]
fn bd_decay_lengthens_the_ring() {
    let short = ring(&render(BD, json!({ "decay": 0.0 }), 1.0), 0.01);
    let long = ring(&render(BD, json!({ "decay": 5.0 }), 1.0), 0.01);
    assert!(long > 2.0 * short, "{short} {long}");
}

#[test]
fn bd_attack_adds_the_click() {
    let soft = peak(&render(BD, json!({ "attack": 0.0 }), 0.004));
    let hard = peak(&render(BD, json!({ "attack": 5.0 }), 0.004));
    assert!(hard > 1.3 * soft, "{soft} {hard}");
}

#[test]
fn sd_pitch_bends_down_after_the_strike() {
    // IC35 lifts the inverters' supply at the strike, then lets it fall.
    let x = render(SD, json!({ "snappy": 0.0 }), 0.2);
    let early = crossing_hz(&x, 0.003, 0.012, SR);
    let late = crossing_hz(&x, 0.05, 0.12, SR);
    assert!(early > late * 1.05, "{early} {late}");
}

#[test]
fn sd_snappy_adds_noise() {
    let dry = energy_above(&render(SD, json!({ "snappy": 0.0 }), 0.3), 2000.0);
    let wet = energy_above(&render(SD, json!({ "snappy": 5.0 }), 0.3), 2000.0);
    assert!(wet > 5.0 * dry, "{dry} {wet}");
}

#[test]
fn hold_keeps_the_controls_of_the_strike() {
    // The tone control swings from 5 V to −5 V an eighth of a second into
    // the hit. Held, the hit sounds as one at a steady 5 V.
    let tail = |tone: Option<f32>, hold: bool| {
        let mut params = json!({ "snappy": 5.0, "hold": hold });
        let mut clocks = vec![("trigger", 1.0)];
        match tone {
            Some(volts) => params["tone"] = json!(volts),
            None => clocks.push(("tone", 4.0)),
        }
        let (x, _) = render_clocks(SD, params, &clocks, "output", 0.25);
        rms(&x[(0.15 * SR) as usize..])
    };
    let steady = tail(Some(5.0), false);
    let (held, live) = (tail(None, true), tail(None, false));
    assert!((held / steady - 1.0).abs() < 0.01, "{held} {steady}");
    assert!(live < 0.5 * steady, "{live} {steady}");
}

#[test]
fn sd_tone_lengthens_the_noise() {
    // On the hardware, the noise 100–250 ms after the strike is some 20 dB
    // louder with the tone knob all the way up than all the way down.
    let tail = |tone: f32| {
        let x = render(SD, json!({ "tone": tone, "snappy": 5.0 }), 0.25);
        let w = window(&x, 0.1, 0.15);
        rms(w) * energy_above(w, 3000.0).sqrt()
    };
    let (short, long) = (tail(0.0), tail(5.0));
    assert!(long > 10.0 * short, "{short} {long}");
}

#[test]
fn freq_tracks_volts_per_octave() {
    // Each voice with its noise turned down, its stock freq, and a window
    // of its ring clear of the strike.
    let voices = [
        (
            BD,
            json!({ "attack": 0.0, "decay": 5.0 }),
            -2.2235,
            0.15,
            0.3,
        ),
        (SD, json!({ "snappy": 0.0 }), -0.8243, 0.04, 0.12),
        (LT, json!({}), -2.0993, 0.1, 0.3),
        (MT, json!({}), -1.8098, 0.1, 0.3),
        (HT, json!({}), -1.5143, 0.1, 0.3),
    ];
    for (voice, params, stock, from, to) in voices {
        let hz = |freq: f32| {
            let mut params = params.clone();
            params["freq"] = json!(freq);
            let x = render(voice, params, to);
            crossing_hz(&x, from, to, SR)
        };
        let base = hz(stock);
        for octaves in [-1.0f32, 1.0] {
            let ratio = hz(stock + octaves) / base;
            assert!(
                (ratio / octaves.exp2() - 1.0).abs() < 0.02,
                "{voice} {octaves}: {ratio}"
            );
        }
    }
}

#[test]
fn toms_rise_from_low_to_high() {
    let hz = |voice| crossing_hz(&render(voice, json!({}), 0.4), 0.1, 0.3, SR);
    let (lt, mt, ht) = (hz(LT), hz(MT), hz(HT));
    assert!(lt < mt && mt < ht, "{lt} {mt} {ht}");
}

#[test]
fn toms_decay_lengthens_the_ring() {
    for voice in [LT, MT, HT] {
        let short = ring(&render(voice, json!({ "decay": 0.0 }), 1.0), 0.02);
        let long = ring(&render(voice, json!({ "decay": 5.0 }), 1.0), 0.02);
        assert!(long > 1.5 * short, "{voice} {short} {long}");
    }
}

#[test]
fn rs_rings_briefly_at_its_resonators() {
    // IC49a's bridged-T rings near 220 Hz and IC49b's near 1050 Hz.
    let x = render(RS, json!({}), 0.3);
    let floor = power_at(&x, 3000.0, SR);
    for hz in [220.0, 1050.0] {
        assert!(power_at(&x, hz, SR) > 20.0 * floor, "{hz}");
    }
    assert!(peak(window(&x, 0.08, 0.1)) < 0.01 * peak(&x));
}

#[test]
fn cp_claps_several_times() {
    let p: Vec<f32> = render(CP, json!({}), 0.2)
        .chunks((SR / 1000.0) as usize)
        .map(peak)
        .collect();
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
}

#[test]
fn channels_added_by_a_patch_update_get_their_own_noise() {
    for voice in [BD, SD, LT, MT, HT, CP] {
        let old = make(voice, json!({ "trigger": 5.0 }), SR);
        old.on_patch_update();
        for _ in 0..480 {
            old.start_block();
            old.ensure_processed();
        }
        let new = make(voice, json!({ "trigger": [5.0, 5.0, 5.0] }), SR);
        new.transfer_state_from(old.as_ref(), &modular_core::types::SameIds);
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
        assert!(peak(&diff) > 1.0e-3 * peak(&a), "{voice} {}", peak(&diff));
    }
}
