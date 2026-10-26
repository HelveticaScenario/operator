use super::super::tests::{SR, poly_volts, rms, sine};
use super::*;
use crate::types::{OutputStruct, PolySignalFields, Signal};

/// Engine samples per bar of the test playhead: 1 s.
const BAR: f64 = 48_000.0;

const ALL_QUALITIES: [GranularQuality; 4] = [
    GranularQuality::Digital,
    GranularQuality::Vintage,
    GranularQuality::CleanTape,
    GranularQuality::DirtyTape,
];

fn delay_params(quality: GranularQuality) -> ClockedDelayParams {
    ClockedDelayParams {
        input: PolySignal::mono(Signal::Volts(0.0)),
        playhead: None,
        bpm: None,
        bar_length: None,
        groove: None,
        bars: None,
        time: None,
        shape: None,
        groove_amount: None,
        freeze: None,
        freq: None,
        root_freq: None,
        tone: None,
        feedback: poly_volts(&[0.0]),
        mix: poly_volts(&[5.0]),
        quality,
        fresh: None,
    }
}

/// A module up to the point where state transfer happens.
fn construct(params: ClockedDelayParams) -> ClockedDelay {
    let channels = params
        .poly_signal_fields()
        .iter()
        .map(|p| p.channels())
        .max()
        .unwrap_or(1)
        .max(1);
    let mut outputs = ClockedDelayOutputs::default();
    outputs.set_all_channels(channels);
    let mut m = ClockedDelay {
        outputs,
        state: Default::default(),
        params,
        channel_state: (0..channels).map(|_| Default::default()).collect(),
        _channel_count: channels,
        _block_index: Default::default(),
    };
    m.init(SR);
    m
}

/// Build a module the way the engine does: `init`, then `on_patch_update`.
fn make(params: ClockedDelayParams) -> ClockedDelay {
    let mut m = construct(params);
    m.on_patch_update();
    m
}

/// Carry `old`'s state into `new` as a patch update does.
fn transfer(old: &mut ClockedDelay, new: &mut ClockedDelay) {
    std::mem::swap(&mut old.state, &mut new.state);
    let n = old.channel_state.len().min(new.channel_state.len());
    new.channel_state[..n].swap_with_slice(&mut old.channel_state[..n]);
    new.on_patch_update();
}

fn playhead(bars: f64) -> Option<MonoSignal> {
    Some(MonoSignal::from_poly(PolySignal::mono(Signal::Volts(
        bars as f32,
    ))))
}

/// Advance one sample with a mono input and the playhead at `bars`.
fn step(m: &mut ClockedDelay, input: f32, bars: f64) -> f32 {
    m.params.input = PolySignal::mono(Signal::Volts(input));
    m.params.playhead = playhead(bars);
    m.update(SR);
    m.outputs.sample.get(0)
}

/// Run a single impulse at sample `at` through `m` with the playhead
/// running at one bar per `bar` samples; returns the output.
fn impulse_response(m: &mut ClockedDelay, at: usize, len: usize, bar: f64) -> Vec<f32> {
    (0..len)
        .map(|n| {
            let x = if n == at { 4.0 } else { 0.0 };
            step(m, x, n as f64 / bar)
        })
        .collect()
}

/// Index of the loudest sample from `from`.
fn peak_after(xs: &[f32], from: usize) -> usize {
    (from..xs.len())
        .max_by(|&a, &b| xs[a].abs().total_cmp(&xs[b].abs()))
        .unwrap()
}

#[test]
fn silence_stays_silent_in_every_quality() {
    for q in ALL_QUALITIES {
        let mut p = delay_params(q);
        p.feedback = poly_volts(&[5.0]);
        p.shape = poly_volts(&[2.5]);
        let mut m = make(p);
        for n in 0..20_000 {
            let y = step(&mut m, 0.0, n as f64 / BAR);
            assert!(y.abs() < 1e-6, "{q:?}: {y} at {n}");
        }
    }
}

#[test]
fn dry_signal_passes_at_hardware_dry_gain() {
    let mut p = delay_params(GranularQuality::Digital);
    p.mix = poly_volts(&[0.0]);
    let mut m = make(p);
    for n in 0..4800 {
        let x = sine(220.0, n, SR, 1.0);
        let y = step(&mut m, x, n as f64 / BAR);
        if n > 100 {
            assert!((y - 0.75 * x).abs() < 1e-5, "{y} vs {x} at {n}");
        }
    }
}

#[test]
fn repeat_lands_after_bars_of_the_playhead() {
    for q in ALL_QUALITIES {
        let mut m = make(delay_params(q));
        let y = impulse_response(&mut m, 2000, 20_000, BAR);
        // A quarter bar of a one-second bar, give or take the resampler's
        // latency.
        let peak = peak_after(&y, 2100);
        let expected = 2000 + 12_000;
        assert!(
            (peak as i64 - expected as i64).abs() < 80,
            "{q:?}: repeat at {peak}, expected {expected}"
        );
        assert!(y[peak].abs() > 0.2, "{q:?}: repeat level {}", y[peak]);
    }
}

#[test]
fn delay_time_follows_the_tempo() {
    let mut p = delay_params(GranularQuality::Digital);
    p.bars = poly_volts(&[0.5]);
    let mut m = make(p);
    // Half-second bars: half a bar is 12 000 samples.
    let y = impulse_response(&mut m, 4000, 20_000, BAR / 2.0);
    let peak = peak_after(&y, 4100);
    assert!((peak as i64 - 16_000).abs() < 4, "repeat at {peak}");
}

#[test]
fn time_multiplies_the_delay() {
    let mut p = delay_params(GranularQuality::Digital);
    p.bars = poly_volts(&[0.125]);
    // ×2 is a fifteenth of the way up.
    p.time = poly_volts(&[5.0 / 15.0]);
    let mut m = make(p);
    let y = impulse_response(&mut m, 2000, 20_000, BAR);
    let peak = peak_after(&y, 2100);
    assert!((peak as i64 - 14_000).abs() < 4, "repeat at {peak}");
}

#[test]
fn feedback_brings_more_repeats() {
    let run = |feedback: f32| {
        let mut p = delay_params(GranularQuality::Digital);
        p.bars = poly_volts(&[0.125]);
        p.feedback = poly_volts(&[feedback]);
        let mut m = make(p);
        let y = impulse_response(&mut m, 1000, 30_000, BAR);
        // The third repeat, three eighths of a bar after the impulse.
        rms(&y[18_500..19_500])
    };
    let dry = run(0.0);
    let wet = run(4.0);
    assert!(dry < 1e-4, "no feedback leaves a third repeat of {dry}");
    assert!(wet > 1e-3, "feedback leaves a third repeat of {wet}");
}

#[test]
fn feedback_stays_bounded_at_full() {
    let mut p = delay_params(GranularQuality::Digital);
    p.bars = poly_volts(&[1.0 / 16.0]);
    p.feedback = poly_volts(&[5.0]);
    let mut m = make(p);
    for n in 0..96_000 {
        let x = sine(330.0, n, SR, 4.0);
        let y = step(&mut m, x, n as f64 / BAR);
        assert!(y.is_finite() && y.abs() <= 5.0 + 1e-3, "{y} at {n}");
    }
}

#[test]
fn shape_windows_repeats_in_step_with_the_playhead() {
    let mut p = delay_params(GranularQuality::Digital);
    p.bars = poly_volts(&[0.25]);
    // A triangle window, silent at the start of each quarter bar.
    p.shape = poly_volts(&[2.5]);
    let mut m = make(p);
    // The playhead jumps a tenth of a bar ahead, and the windows follow it.
    let position = |n: usize| n as f64 / BAR + if n >= 30_000 { 0.1 } else { 0.0 };
    let y: Vec<f32> = (0..80_000)
        .map(|n| step(&mut m, sine(437.0, n, SR, 2.0), position(n)))
        .collect();
    let quarter = 12_000;
    let edges = [12_000, 24_000, 43_200, 55_200, 67_200];
    for edge in edges {
        let middle = edge + quarter / 2;
        let at_edge = rms(&y[edge - 100..edge + 100]);
        let at_middle = rms(&y[middle - 100..middle + 100]);
        assert!(
            at_edge < 0.1 * at_middle,
            "window at {edge}: {at_edge} at the edge, {at_middle} in the middle"
        );
    }
}

#[test]
fn groove_amount_moves_the_windows_by_its_share_of_the_groove() {
    let mut p = delay_params(GranularQuality::Digital);
    p.bars = poly_volts(&[0.25]);
    p.shape = poly_volts(&[2.5]);
    // Half the groove, which runs a tenth of a bar ahead of the playhead.
    p.groove_amount = poly_volts(&[2.5]);
    p.groove = Some(MonoSignal::from_poly(PolySignal::mono(Signal::Volts(0.1))));
    let mut m = make(p);
    let y: Vec<f32> = (0..60_000)
        .map(|n| step(&mut m, sine(437.0, n, SR, 2.0), n as f64 / BAR))
        .collect();
    // Windows start where the playhead plus 0.05 bars is a quarter bar.
    let quarter = 12_000;
    for edge in [21_600, 33_600, 45_600] {
        let middle = edge + quarter / 2;
        let at_edge = rms(&y[edge - 100..edge + 100]);
        let at_middle = rms(&y[middle - 100..middle + 100]);
        assert!(
            at_edge < 0.1 * at_middle,
            "window at {edge}: {at_edge} at the edge, {at_middle} in the middle"
        );
    }
}

#[test]
fn windows_follow_a_delay_clamped_to_the_buffer() {
    let mut p = delay_params(GranularQuality::Digital);
    // Six one-second bars, clamped to the four seconds the buffer holds.
    p.bars = poly_volts(&[6.0]);
    p.shape = poly_volts(&[2.5]);
    let mut m = make(p);
    let y: Vec<f32> = (0..400_000)
        .map(|n| step(&mut m, sine(437.0, n, SR, 2.0), n as f64 / BAR))
        .collect();
    let edge = 384_000;
    let middle = edge - 96_000;
    let at_edge = rms(&y[edge - 100..edge + 100]);
    let at_middle = rms(&y[middle - 100..middle + 100]);
    assert!(
        at_edge < 0.1 * at_middle,
        "{at_edge} at the edge, {at_middle} in the middle"
    );
}

#[test]
fn shape_zero_leaves_repeats_unwindowed() {
    let mut m = make(delay_params(GranularQuality::Digital));
    let y: Vec<f32> = (0..48_000)
        .map(|n| step(&mut m, sine(440.0, n, SR, 2.0), n as f64 / BAR))
        .collect();
    let a = rms(&y[24_000..30_000]);
    let b = rms(&y[30_000..36_000]);
    assert!((a - b).abs() < 0.02 * a, "{a} vs {b}");
}

#[test]
fn tempo_change_crossfades_to_the_new_time() {
    let mut p = delay_params(GranularQuality::Digital);
    p.bars = poly_volts(&[0.25]);
    let mut m = make(p);
    let mut position = 0.0;
    let mut y = Vec::new();
    for n in 0..60_000 {
        // The tempo doubles half a second in.
        position += if n < 24_000 { 1.0 / BAR } else { 2.0 / BAR };
        let x = if n == 40_000 { 4.0 } else { 0.0 };
        // Not a whole number of cycles in either delay time, so a hard
        // swap would jump.
        let tone = if n < 40_000 {
            sine(437.0, n, SR, 2.0)
        } else {
            0.0
        };
        y.push(step(&mut m, x + tone, position));
    }
    // A quarter of a half-second bar.
    let peak = peak_after(&y, 46_000 - 200);
    assert!((peak as i64 - 46_000).abs() < 4, "repeat at {peak}");
    // The repeats of the steady sine, at most 0.17 V apart from one sample
    // to the next, never jump as the time swaps over.
    for (n, w) in y[..40_000].windows(2).enumerate() {
        assert!((w[1] - w[0]).abs() < 0.2, "step of {} at {n}", w[1] - w[0]);
    }
}

#[test]
fn stopped_playhead_keeps_the_last_tempo() {
    let mut p = delay_params(GranularQuality::Digital);
    p.bars = poly_volts(&[0.25]);
    let mut m = make(p);
    let mut y = Vec::new();
    for n in 0..40_000 {
        // Half-second bars, then the transport stops.
        let position = n.min(10_000) as f64 / (BAR / 2.0);
        let x = if n == 20_000 { 4.0 } else { 0.0 };
        y.push(step(&mut m, x, position));
    }
    let peak = peak_after(&y, 20_100);
    assert!((peak as i64 - 26_000).abs() < 4, "repeat at {peak}");
}

#[test]
fn freeze_loops_what_was_held() {
    let mut p = delay_params(GranularQuality::Digital);
    p.bars = poly_volts(&[0.25]);
    let mut m = make(p);
    let mut held = Vec::new();
    for n in 0..96_000 {
        let frozen = n >= 48_000;
        m.params.freeze = poly_volts(&[if frozen { 5.0 } else { 0.0 }]);
        let x = if frozen { 0.0 } else { sine(220.0, n, SR, 2.0) };
        let y = step(&mut m, x, n as f64 / BAR);
        if n >= 60_000 {
            held.push(y);
        }
    }
    assert!(rms(&held) > 0.3, "frozen output {}", rms(&held));
}

#[test]
fn pitch_shifts_the_repeats() {
    let mut p = delay_params(GranularQuality::Digital);
    p.bars = poly_volts(&[1.0 / 8.0]);
    p.freq = poly_volts(&[1.0]);
    let mut m = make(p);
    let y: Vec<f32> = (0..48_000)
        .map(|n| step(&mut m, sine(220.0, n, SR, 2.0), n as f64 / BAR))
        .collect();
    // An octave up, the repeats cross zero about twice as often.
    let crossings = y[24_000..48_000]
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    let expected = 2 * 440 * 24_000 / 48_000;
    assert!(
        (crossings as i64 - expected as i64).abs() < expected as i64 / 10,
        "{crossings} crossings, expected about {expected}"
    );
}

#[test]
fn recording_survives_a_patch_update() {
    let p = delay_params(GranularQuality::Digital);
    let mut old = make(p.clone());
    for n in 0..2000 {
        let x = if n == 1000 { 4.0 } else { 0.0 };
        step(&mut old, x, n as f64 / BAR);
    }
    let mut new = construct(p);
    transfer(&mut old, &mut new);
    let y: Vec<f32> = (2000..20_000)
        .map(|n| step(&mut new, 0.0, n as f64 / BAR))
        .collect();
    let peak = peak_after(&y, 0) + 2000;
    assert!((peak as i64 - 13_000).abs() < 4, "repeat at {peak}");
}

#[test]
fn quality_change_clears_the_recording() {
    let p = delay_params(GranularQuality::Digital);
    let mut old = make(p.clone());
    for n in 0..2000 {
        let x = if n == 1000 { 4.0 } else { 0.0 };
        step(&mut old, x, n as f64 / BAR);
    }
    let mut q = p;
    q.quality = GranularQuality::Vintage;
    let mut new = construct(q);
    transfer(&mut old, &mut new);
    for n in 2000..20_000 {
        let y = step(&mut new, 0.0, n as f64 / BAR);
        assert!(y.abs() < 1e-6, "{y} at {n}");
    }
}

#[test]
fn pitched_repeats_at_the_longest_delay_stay_in_the_buffer() {
    for quality in ALL_QUALITIES {
        for freq in [-1.0, 1.0] {
            let mut p = delay_params(quality);
            p.bars = poly_volts(&[4.0]);
            p.time = poly_volts(&[5.0]);
            p.freq = poly_volts(&[freq]);
            let mut m = make(p);
            for n in 0..96_000 {
                let y = step(&mut m, sine(220.0, n, SR, 2.0), n as f64 / BAR);
                assert!(y.is_finite(), "{quality:?} at {freq} V");
            }
        }
    }
}

#[test]
fn a_channel_added_mid_block_delays_its_input() {
    for quality in ALL_QUALITIES {
        let p = delay_params(quality);
        let mut old = make(p.clone());
        // An odd count, so the update lands inside a control block.
        for n in 0..2001 {
            step(&mut old, 0.0, n as f64 / BAR);
        }
        let mut q = p;
        q.input = poly_volts(&[0.0, 0.0]).unwrap();
        let mut new = construct(q);
        transfer(&mut old, &mut new);
        let mut peak = 0.0f32;
        for n in 2001..20_000 {
            let x = if n == 3000 { 4.0 } else { 0.0 };
            new.params.input = poly_volts(&[0.0, x]).unwrap();
            new.params.playhead = playhead(n as f64 / BAR);
            new.update(SR);
            if n > 3100 {
                peak = peak.max(new.outputs.sample.get(1).abs());
            }
        }
        assert!(peak > 0.5, "{quality:?}: repeat peak {peak}");
    }
}

#[test]
fn tempo_is_measured_from_the_playhead_starting() {
    // 90 BPM, starting part-way through a measuring window, either where the
    // playhead stopped or after a reset from mid-bar to 0.
    let bar = 48_000.0 * 4.0 * 60.0 / 90.0;
    let start = 10_100;
    for stopped_at in [0.0, 0.7] {
        let mut m = make(delay_params(GranularQuality::Digital));
        // The playhead first moves on the reset, or a sample after `start`.
        let first_move = if stopped_at == 0.0 { start + 1 } else { start };
        for n in 0..start + 2 * TEMPO_WINDOW as usize {
            let position = if n < start {
                stopped_at
            } else {
                (n - start) as f64 / bar
            };
            step(&mut m, 0.0, position);
            let seconds = m
                .state
                .tempo
                .bar_seconds(SR, DEFAULT_BPM, DEFAULT_BAR_LENGTH);
            let expected = if n < first_move + TEMPO_WINDOW as usize {
                2.0
            } else {
                8.0 / 3.0
            };
            assert!(
                (seconds - expected).abs() < 1e-3,
                "{seconds} s bars at {n}, stopped at {stopped_at}"
            );
        }
    }
}

#[test]
fn bpm_and_meter_time_the_delay_until_the_playhead_moves() {
    let mono = |v: f32| Some(MonoSignal::from_poly(PolySignal::mono(Signal::Volts(v))));
    let mut p = delay_params(GranularQuality::Digital);
    p.bars = poly_volts(&[0.25]);
    p.bpm = mono(90.0);
    p.bar_length = mono(3.0);
    let mut m = make(p);
    // A quarter of a 6/8 bar at 90 BPM is 24 000 samples.
    let y: Vec<f32> = (0..40_000)
        .map(|n| step(&mut m, if n == 1000 { 4.0 } else { 0.0 }, 0.0))
        .collect();
    let peak = peak_after(&y, 1100);
    assert!((peak as i64 - 25_000).abs() < 4, "repeat at {peak}");
}

#[test]
fn freeze_switches_without_a_click() {
    for quality in ALL_QUALITIES {
        let mut p = delay_params(quality);
        p.bars = poly_volts(&[0.25]);
        let mut m = make(p);
        let (on, off) = (60_013, 120_029);
        let y: Vec<f32> = (0..124_000)
            .map(|n| {
                let frozen = (on..off).contains(&n);
                m.params.freeze = poly_volts(&[if frozen { 5.0 } else { 0.0 }]);
                step(&mut m, sine(220.0, n, SR, 2.0), n as f64 / BAR)
            })
            .collect();
        let largest_step = |from: usize| {
            y[from..from + 2000]
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max)
        };
        let steady = largest_step(40_000);
        for (edge, at) in [("on", on), ("off", off)] {
            let around = largest_step(at - 5);
            assert!(
                around < 1.3 * steady,
                "{quality:?}: freeze {edge} steps {around} V, steady {steady} V"
            );
        }
    }
}
