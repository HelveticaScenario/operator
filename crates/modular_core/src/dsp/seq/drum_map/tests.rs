use super::*;
use crate::types::{OutputStruct, Signal};

const SAMPLE_RATE: f32 = 48000.0;
const SAMPLES_PER_STEP: usize = 64;

fn volts(v: f32) -> Option<PolySignal> {
    Some(PolySignal::mono(Signal::Volts(v)))
}

fn params(x: f32, y: f32, density: f32, resolution: DrumMapResolution) -> DrumMapParams {
    DrumMapParams {
        playhead: None,
        x: volts(x),
        y: volts(y),
        chaos: None,
        bd_density: volts(density),
        sd_density: volts(density),
        hh_density: volts(density),
        resolution,
    }
}

/// Mirrors the production lifecycle: construction, then `on_patch_update`.
fn make_fresh(params: DrumMapParams) -> DrumMap {
    let channels = 1;
    let mut outputs = DrumMapOutputs::default();
    outputs.set_all_channels(channels);
    let mut m = DrumMap {
        params,
        outputs,
        _channel_count: channels,
        _block_index: Default::default(),
        state: DrumMapState::default(),
        channel_state: vec![ChannelState::default(); channels].into_boxed_slice(),
    };
    m.on_patch_update();
    m
}

/// A module whose transport has just started.
fn make(params: DrumMapParams) -> DrumMap {
    let mut m = make_fresh(params);
    m.on_clock_message(&ClockMessages::Start).unwrap();
    m
}

/// Drives the playhead the way the root clock does: bar phase on channel 0,
/// completed bar count on channel 1.
fn set_playhead(m: &mut DrumMap, p: f64) {
    let bars = p.floor();
    m.params.playhead = Some(MonoSignal::from_poly(PolySignal::poly(&[
        Signal::Volts((p - bars) as f32),
        Signal::Volts(bars as f32),
    ])));
}

/// Moves the playhead to `p` and reports whether the bass drum fired there.
fn bd_fires_at(m: &mut DrumMap, p: f64) -> bool {
    set_playhead(m, p);
    m.update(SAMPLE_RATE);
    let fired = m.outputs.bd.get(0) > 2.5;
    for _ in 0..SAMPLE_RATE as usize / 100 {
        m.update(SAMPLE_RATE);
    }
    fired
}

/// Playhead position of `step` within bar 0, nudged inside the step.
fn at_step(step: usize) -> f64 {
    (step as f64 + 0.25) / STEPS_PER_PATTERN as f64
}

/// Runs one cycle and returns, per output, the 32-step grid of which steps
/// produced a rising edge.
fn run_cycle(m: &mut DrumMap, cycle: usize) -> [[bool; STEPS_PER_PATTERN]; 6] {
    let mut hits = [[false; STEPS_PER_PATTERN]; 6];
    let mut prev = [0.0f32; 6];
    let total = STEPS_PER_PATTERN * SAMPLES_PER_STEP;
    for i in 0..total {
        set_playhead(m, cycle as f64 + i as f64 / total as f64);
        m.update(SAMPLE_RATE);
        let o = &m.outputs;
        let now = [
            o.bd.get(0),
            o.sd.get(0),
            o.hh.get(0),
            o.bd_accent.get(0),
            o.sd_accent.get(0),
            o.hh_accent.get(0),
        ];
        for k in 0..6 {
            if now[k] > 2.5 && prev[k] <= 2.5 {
                hits[k][i / SAMPLES_PER_STEP] = true;
            }
        }
        prev = now;
    }
    hits
}

/// At a node corner both mix stages run at balance 0, which scales by
/// 255/256 each (avrlib `U8Mix`).
#[test]
fn map_corner_reads_node() {
    let attenuate = |v: u8| ((v as u16 * 255) >> 8) as u8;
    for step in 0..STEPS_PER_PATTERN {
        for part in 0..NUM_PARTS {
            let node = NODES[10][part * STEPS_PER_PATTERN + step];
            assert_eq!(read_drum_map(step, part, 0, 0), attenuate(attenuate(node)));
        }
    }
}

#[test]
fn hits_match_map_threshold() {
    let mut m = make(params(1.3, 3.7, 3.0, DrumMapResolution::ThirtySecond));
    let hits = run_cycle(&mut m, 0);
    let (x, y, density) = (knob(1.3), knob(3.7), knob(3.0));
    for part in 0..NUM_PARTS {
        for step in 0..STEPS_PER_PATTERN {
            let level = read_drum_map(step, part, x, y);
            assert_eq!(
                hits[part][step],
                level > !density,
                "part {part} step {step}"
            );
            assert_eq!(
                hits[part + 3][step],
                level > !density && level > ACCENT_LEVEL,
                "accent part {part} step {step}"
            );
        }
    }
}

#[test]
fn zero_density_is_silent() {
    let mut m = make(params(2.5, 2.5, 0.0, DrumMapResolution::ThirtySecond));
    let hits = run_cycle(&mut m, 0);
    assert!(hits.iter().flatten().all(|&h| !h));
}

#[test]
fn full_density_plays_every_nonzero_step() {
    let mut m = make(params(0.0, 0.0, 5.0, DrumMapResolution::ThirtySecond));
    let hits = run_cycle(&mut m, 0);
    for part in 0..NUM_PARTS {
        for step in 0..STEPS_PER_PATTERN {
            assert_eq!(hits[part][step], read_drum_map(step, part, 0, 0) > 0);
        }
    }
}

#[test]
fn sixteenth_resolution_skips_odd_steps() {
    let mut m = make(params(4.0, 1.0, 5.0, DrumMapResolution::Sixteenth));
    let hits = run_cycle(&mut m, 0);
    assert!(hits[0][0], "downbeat bass drum plays");
    for part in 0..NUM_PARTS {
        for step in (1..STEPS_PER_PATTERN).step_by(2) {
            assert!(!hits[part][step], "part {part} odd step {step}");
        }
    }
}

#[test]
fn every_cycle_repeats_without_chaos() {
    let mut m = make(params(2.0, 2.0, 3.5, DrumMapResolution::ThirtySecond));
    let first = run_cycle(&mut m, 0);
    assert_eq!(run_cycle(&mut m, 1), first);
    assert_eq!(run_cycle(&mut m, 2), first);
}

#[test]
fn chaos_varies_cycles() {
    let mut p = params(2.0, 2.0, 2.0, DrumMapResolution::ThirtySecond);
    p.chaos = volts(5.0);
    let mut m = make(p);
    let first = run_cycle(&mut m, 0);
    assert!((1..8).any(|c| run_cycle(&mut m, c) != first));
}

#[test]
fn first_update_mid_step_does_not_fire() {
    let mut m = make_fresh(params(0.0, 0.0, 5.0, DrumMapResolution::ThirtySecond));
    set_playhead(&mut m, at_step(0));
    m.update(SAMPLE_RATE);
    assert_eq!(m.outputs.bd.get(0), 0.0);
}

#[test]
fn resolution_accepts_only_16_or_32() {
    let parse = |v: serde_json::Value| {
        deserr::deserialize::<DrumMapParams, _, crate::param_errors::ModuleParamErrors>(
            serde_json::json!({ "resolution": v }),
        )
        .map(|p| p.resolution)
    };
    assert_eq!(parse(16.into()).ok(), Some(DrumMapResolution::Sixteenth));
    assert_eq!(parse(32.into()).ok(), Some(DrumMapResolution::ThirtySecond));
    assert_eq!(
        parse(32.0.into()).ok(),
        Some(DrumMapResolution::ThirtySecond)
    );
    assert!(parse(24.into()).is_err());
    assert!(parse("sixteenth".into()).is_err());
}

/// Every step of node 10 that plays at full density, for picking test steps.
fn full_density_bd_steps() -> Vec<usize> {
    (0..STEPS_PER_PATTERN)
        .filter(|&s| read_drum_map(s, 0, 0, 0) > 0)
        .collect()
}

#[test]
fn transport_start_fires_downbeat_after_first_clock_tick() {
    let mut m = make_fresh(params(0.0, 0.0, 5.0, DrumMapResolution::ThirtySecond));
    assert!(!bd_fires_at(&mut m, 3.0 + at_step(17)));

    m.on_clock_message(&ClockMessages::Stop).unwrap();
    m.on_clock_message(&ClockMessages::Start).unwrap();
    // The clock advances its phase before publishing the first playhead.
    assert!(bd_fires_at(&mut m, 1e-5));
}

#[test]
fn transport_stop_is_silent() {
    let mut m = make(params(0.0, 0.0, 5.0, DrumMapResolution::ThirtySecond));
    assert!(bd_fires_at(&mut m, 2.0 + at_step(0)));
    bd_fires_at(&mut m, 2.0 + at_step(17));

    m.on_clock_message(&ClockMessages::Stop).unwrap();
    assert!(!bd_fires_at(&mut m, 0.0));
    assert!(!bd_fires_at(&mut m, 0.0));
}

#[test]
fn rewind_within_one_step_does_not_replay() {
    let b = full_density_bd_steps()[2];
    let mut m = make(params(0.0, 0.0, 5.0, DrumMapResolution::ThirtySecond));
    bd_fires_at(&mut m, 0.0);
    assert!(bd_fires_at(&mut m, at_step(b)));
    // Back across the step boundary into the previous step, then forward again.
    assert!(!bd_fires_at(&mut m, at_step(b - 1)));
    assert!(!bd_fires_at(&mut m, at_step(b)));
}

#[test]
fn resolution_change_on_odd_step_does_not_replay() {
    let mut m = make(params(0.0, 0.0, 5.0, DrumMapResolution::ThirtySecond));
    bd_fires_at(&mut m, 0.0);
    // Step 12 plays in node 10; the playhead sits on step 13 when the
    // resolution drops to 16, which rounds it down to 12.
    assert!(read_drum_map(12, 0, 0, 0) > 0);
    bd_fires_at(&mut m, at_step(13));
    m.params.resolution = DrumMapResolution::Sixteenth;
    assert!(!bd_fires_at(&mut m, at_step(13)));
}

#[test]
fn backward_seek_resyncs_silently() {
    let mut m = make(params(0.0, 0.0, 5.0, DrumMapResolution::ThirtySecond));
    bd_fires_at(&mut m, 0.0);
    bd_fires_at(&mut m, at_step(20));
    // Seek back 8 steps: no hit where the playhead lands, the next step plays.
    assert!(read_drum_map(12, 0, 0, 0) > 0);
    assert!(!bd_fires_at(&mut m, at_step(12)));
    assert!(bd_fires_at(&mut m, at_step(14)) == (read_drum_map(14, 0, 0, 0) > 0));
}

#[test]
fn phasor_playhead_wraps_into_a_new_cycle() {
    let mut m = make(params(0.0, 0.0, 5.0, DrumMapResolution::ThirtySecond));
    assert!(bd_fires_at(&mut m, 0.0));
    bd_fires_at(&mut m, at_step(31));
    // A 0–1 ramp wraps without the bar count advancing.
    m.params.playhead = Some(MonoSignal::from_poly(PolySignal::mono(Signal::Volts(
        0.001,
    ))));
    m.update(SAMPLE_RATE);
    assert_eq!(m.outputs.bd.get(0), 5.0);
}

#[test]
fn chaos_rerolls_once_per_cycle() {
    let mut p = params(2.0, 2.0, 2.0, DrumMapResolution::ThirtySecond);
    p.chaos = volts(5.0);
    let mut m = make(p);
    let mut rolls = Vec::new();
    for cycle in 0..4 {
        for step in 0..STEPS_PER_PATTERN {
            set_playhead(&mut m, cycle as f64 + at_step(step));
            m.update(SAMPLE_RATE);
            if step == 0 {
                rolls.push(m.channel_state[0].perturbation);
            } else {
                assert_eq!(m.channel_state[0].perturbation, *rolls.last().unwrap());
            }
        }
    }
    assert!(rolls.windows(2).all(|w| w[0] != w[1]));
}

#[test]
fn step_stays_exact_at_large_bar_counts() {
    let mut m = make(params(0.0, 0.0, 5.0, DrumMapResolution::ThirtySecond));
    let bars = (1u32 << 20) as f64;
    bd_fires_at(&mut m, bars);
    set_playhead(&mut m, bars + at_step(13));
    m.update(SAMPLE_RATE);
    assert_eq!(m.state.step, 13);
}

/// Playhead advance per sample at `bpm` in 4/4, in cycles.
fn clock_rate(bpm: f64) -> f64 {
    bpm / 60.0 / 4.0 / SAMPLE_RATE as f64
}

/// Creates a module whose first sample lands `delay_secs` after the start of
/// `step` in bar 5, then reports whether its first two samples fire the bass
/// drum.
fn fresh_module_fires(step: usize, delay_secs: f64, bpm: f64) -> bool {
    let mut m = make_fresh(params(0.0, 0.0, 5.0, DrumMapResolution::ThirtySecond));
    let rate = clock_rate(bpm);
    let start =
        5.0 + step as f64 / STEPS_PER_PATTERN as f64 + delay_secs * SAMPLE_RATE as f64 * rate;
    let mut fired = false;
    for i in 0..2 {
        set_playhead(&mut m, start + i as f64 * rate);
        m.update(SAMPLE_RATE);
        fired |= m.outputs.bd.get(0) > 2.5;
    }
    fired
}

#[test]
fn module_created_on_bar_line_plays_downbeat() {
    // A quantized swap lands one sample past the boundary.
    assert!(fresh_module_fires(0, 1.0 / SAMPLE_RATE as f64, 120.0));
}

#[test]
fn module_created_on_any_step_boundary_plays_it() {
    let step = full_density_bd_steps()[3];
    assert!(fresh_module_fires(step, 1.0 / SAMPLE_RATE as f64, 120.0));
}

#[test]
fn module_created_after_the_window_stays_silent() {
    assert!(fresh_module_fires(0, 0.001, 120.0));
    assert!(!fresh_module_fires(0, 0.003, 120.0));
    // Mid-step, as an immediate patch update usually lands.
    assert!(!fresh_module_fires(0, 0.03, 120.0));
}

#[test]
fn boundary_window_is_measured_in_time_at_any_tempo() {
    assert!(fresh_module_fires(0, 0.001, 30.0));
    assert!(!fresh_module_fires(0, 0.003, 30.0));
    assert!(fresh_module_fires(0, 0.001, 300.0));
    assert!(!fresh_module_fires(0, 0.003, 300.0));
}

#[test]
fn module_created_at_downbeat_rolls_chaos() {
    let mut p = params(2.0, 2.0, 2.0, DrumMapResolution::ThirtySecond);
    p.chaos = volts(5.0);
    let mut m = make_fresh(p);
    let rate = clock_rate(120.0);
    for i in 1..3 {
        set_playhead(&mut m, 5.0 + i as f64 * rate);
        m.update(SAMPLE_RATE);
    }
    assert_ne!(m.channel_state[0].perturbation, [0; NUM_PARTS]);
}
