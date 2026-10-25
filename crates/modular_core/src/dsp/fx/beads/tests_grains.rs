use super::super::tests::{SR, poly_volts, rms, sine, volts};
use super::*;
use crate::types::{OutputStruct, PolySignalFields, Signal};

fn grains_params(quality: GranularQuality) -> GranularParams {
    GranularParams {
        input: PolySignal::mono(Signal::Volts(0.0)),
        trigger: poly_volts(&[0.0]),
        freeze: None,
        time: None,
        size: None,
        shape: None,
        freq: None,
        root_freq: None,
        spread: None,
        width: None,
        feedback: None,
        mix: None,
        quality,
        fresh: None,
    }
}

/// Build a module the way the engine does: one voice per channel of the
/// widest input, `init`, then `on_patch_update`.
fn make_grains(params: GranularParams, sample_rate: f32) -> Granular {
    let mut m = construct(params, sample_rate);
    m.on_patch_update();
    m
}

/// Carry `old`'s state into `new` as a patch update does: the whole module
/// state, and the voices both modules have.
fn transfer(old: &mut Granular, new: &mut Granular) {
    std::mem::swap(&mut old.state, &mut new.state);
    let n = old.channel_state.len().min(new.channel_state.len());
    new.channel_state[..n].swap_with_slice(&mut old.channel_state[..n]);
    new.on_patch_update();
}

/// A module up to the point where state transfer happens.
fn construct(params: GranularParams, sample_rate: f32) -> Granular {
    let channels = params
        .poly_signal_fields()
        .iter()
        .map(|p| p.channels())
        .max()
        .unwrap_or(1)
        .max(1);
    let mut outputs = GranularOutputs::default();
    outputs.set_all_channels(channels);
    let mut m = Granular {
        outputs,
        state: Default::default(),
        params,
        channel_state: (0..channels).map(|_| Default::default()).collect(),
        _channel_count: channels,
        _block_index: Default::default(),
    };
    m.init(sample_rate);
    m
}

/// Advance one sample with a mono input and trigger level; returns the
/// stereo output.
fn step(m: &mut Granular, input: f32, trigger: f32, sample_rate: f32) -> (f32, f32) {
    m.params.input = PolySignal::mono(Signal::Volts(input));
    m.params.trigger = poly_volts(&[trigger]);
    m.update(sample_rate);
    (m.outputs.sample.get(0), m.outputs.sample.get(1))
}

const ALL_QUALITIES: [GranularQuality; 4] = [
    GranularQuality::Digital,
    GranularQuality::Vintage,
    GranularQuality::CleanTape,
    GranularQuality::DirtyTape,
];

#[test]
fn silence_stays_silent_in_every_quality() {
    for q in ALL_QUALITIES {
        let mut m = make_grains(grains_params(q), SR);
        for n in 0..20_000 {
            let trig = if n % 2400 < 10 { 5.0 } else { 0.0 };
            let (l, r) = step(&mut m, 0.0, trig, SR);
            assert!(l.abs() < 1e-6 && r.abs() < 1e-6, "{q:?}: {l} {r} at {n}");
        }
    }
}

#[test]
fn dry_signal_passes_at_hardware_dry_gain() {
    // Fully dry, the hardware's dry/wet law leaves the input at 0.75×.
    let mut p = grains_params(GranularQuality::Digital);
    p.mix = poly_volts(&[0.0]);
    let mut m = make_grains(p, SR);
    for n in 0..4800 {
        let x = sine(220.0, n, SR, 1.0);
        let (l, r) = step(&mut m, x, 0.0, SR);
        if n > 100 {
            assert!((l - 0.75 * x).abs() < 1e-5, "left {l} vs {x} at {n}");
            assert!((r - 0.75 * x).abs() < 1e-5, "right {r} vs {x} at {n}");
        }
    }
}

#[test]
fn grains_play_only_after_a_trigger() {
    let mut p = grains_params(GranularQuality::Digital);
    p.mix = poly_volts(&[5.0]);
    p.size = poly_volts(&[3.5]);
    let mut m = make_grains(p, SR);
    let mut before = Vec::new();
    let mut after = Vec::new();
    for n in 0..48_000 {
        let trig = if (24_000..24_010).contains(&n) {
            5.0
        } else {
            0.0
        };
        let (l, _) = step(&mut m, sine(330.0, n, SR, 2.0), trig, SR);
        if (12_000..24_000).contains(&n) {
            before.push(l);
        } else if (24_500..30_000).contains(&n) {
            after.push(l);
        }
    }
    assert!(
        rms(&before) < 1e-3,
        "wet output before any trigger: {}",
        rms(&before)
    );
    assert!(rms(&after) > 0.3, "grain output too quiet: {}", rms(&after));
}

/// Estimate frequency from the interpolated positions of the first and last
/// rising zero crossings.
fn frequency(xs: &[f32], sample_rate: f32) -> f32 {
    let crossings: Vec<f32> = xs
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
        .map(|(i, w)| i as f32 + w[0] / (w[0] - w[1]))
        .collect();
    let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
    (crossings.len() - 1) as f32 * sample_rate / (last - first)
}

#[test]
fn freq_transposes_the_grain_from_root_freq_by_volts_per_octave() {
    // 440 Hz is A4, 0.75 V above C4.
    let a4 = (440.0f32 / 261.625_58).log2();
    for (freq, root, expected) in [
        (0.0, 0.0, 440.0),
        (1.0, 0.0, 880.0),
        (-1.0, 0.0, 220.0),
        (1.0, a4, 523.251),
        (a4, a4, 440.0),
    ] {
        let mut p = grains_params(GranularQuality::Digital);
        p.mix = poly_volts(&[5.0]);
        // An ~86 ms square-enveloped grain.
        p.size = poly_volts(&[2.0]);
        p.shape = poly_volts(&[0.0]);
        p.time = poly_volts(&[1.0]);
        p.freq = poly_volts(&[freq]);
        p.root_freq = poly_volts(&[root]);
        let mut m = make_grains(p, SR);
        let mut out = Vec::new();
        for n in 0..60_000 {
            let trig = if (48_000..48_010).contains(&n) {
                5.0
            } else {
                0.0
            };
            let (l, _) = step(&mut m, sine(440.0, n, SR, 2.0), trig, SR);
            if (48_500..51_500).contains(&n) {
                out.push(l);
            }
        }
        assert!(rms(&out) > 0.3, "freq {freq} root {root}: grain silent");
        let f = frequency(&out, SR);
        assert!(
            // Within the accuracy of the frequency estimate.
            (f - expected).abs() < expected * 0.002,
            "freq {freq} root {root}: {f} Hz"
        );
    }
}

#[test]
fn low_rate_qualities_band_limit_the_dry_path() {
    // Dirty tape runs at 24 kHz and vintage at 32 kHz: an 11.5 kHz tone is
    // above the dirty-tape passband but inside vintage's.
    for (q, passes) in [
        (GranularQuality::DirtyTape, false),
        (GranularQuality::Vintage, true),
    ] {
        let mut p = grains_params(q);
        p.mix = poly_volts(&[0.0]);
        let mut m = make_grains(p, SR);
        let mut out = Vec::new();
        for n in 0..24_000 {
            let (l, _) = step(&mut m, sine(11_500.0, n, SR, 1.0), 0.0, SR);
            if n > 4_000 {
                out.push(l);
            }
        }
        let gain = rms(&out) / (1.0 / std::f32::consts::SQRT_2);
        if passes {
            assert!(gain > 0.6, "{q:?} gain {gain}");
        } else {
            assert!(gain < 0.05, "{q:?} gain {gain}");
        }
    }
}

#[test]
fn resampled_dry_path_keeps_its_level() {
    for (q, sample_rate) in [
        (GranularQuality::DirtyTape, 48_000.0),
        (GranularQuality::Vintage, 48_000.0),
        (GranularQuality::Digital, 44_100.0),
    ] {
        let mut p = grains_params(q);
        p.mix = poly_volts(&[0.0]);
        let mut m = make_grains(p, sample_rate);
        let mut out = Vec::new();
        for n in 0..(sample_rate as usize) {
            let (l, _) = step(&mut m, sine(1000.0, n, sample_rate, 1.0), 0.0, sample_rate);
            if n > 4_000 {
                out.push(l);
            }
        }
        let gain = rms(&out) / (1.0 / std::f32::consts::SQRT_2);
        assert!(
            (gain - 0.75).abs() < 0.03,
            "{q:?} at {sample_rate}: gain {gain}"
        );
    }
}

#[test]
fn freeze_keeps_replaying_the_held_audio() {
    let run = |frozen: bool| {
        let mut p = grains_params(GranularQuality::Digital);
        p.mix = poly_volts(&[5.0]);
        p.size = poly_volts(&[3.5]);
        let mut m = make_grains(p, SR);
        let mut out = Vec::new();
        for n in 0..96_000 {
            // Audio for the first second, then silence.
            let x = if n < 48_000 {
                sine(330.0, n, SR, 2.0)
            } else {
                0.0
            };
            m.params.freeze = poly_volts(&[if frozen && n >= 47_000 { 5.0 } else { 0.0 }]);
            let trig = if n % 4800 < 10 { 5.0 } else { 0.0 };
            let (l, _) = step(&mut m, x, trig, SR);
            if n >= 72_000 {
                out.push(l);
            }
        }
        rms(&out)
    };
    let held = run(true);
    let released = run(false);
    assert!(held > 0.3, "frozen grains silent: {held}");
    assert!(
        released < 1e-3,
        "grains of silence should be silent: {released}"
    );
}

/// Record a second of a sine into every voice.
fn record(m: &mut Granular) {
    for n in 0..48_000 {
        step(m, sine(220.0, n, SR, 1.0), 0.0, SR);
    }
}

fn recording_level(m: &Granular, voice: usize) -> usize {
    let buffer = &m.channel_state[voice].voice.granular.buffer;
    (0..buffer.max_delay())
        .filter(|&i| buffer.raw(i as usize) != 0)
        .count()
}

#[test]
fn a_quality_change_takes_a_fresh_buffer_of_the_new_length() {
    let mut old = make_grains(grains_params(GranularQuality::Digital), SR);
    record(&mut old);
    assert!(recording_level(&old, 0) > 0);
    let mut new = construct(grains_params(GranularQuality::DirtyTape), SR);
    transfer(&mut old, &mut new);
    assert_eq!(new.state.media, Media::DirtyTape);
    let buffer = &new.channel_state[0].voice.granular.buffer;
    assert_eq!(buffer.max_delay() as usize, Media::DirtyTape.max_delay());
    assert_eq!(recording_level(&new, 0), 0);
    for n in 0..4800 {
        let (l, r) = step(&mut new, sine(220.0, n, SR, 1.0), 0.0, SR);
        assert!(l.is_finite() && r.is_finite());
    }
}

#[test]
fn an_unchanged_quality_keeps_the_recording() {
    let mut old = make_grains(grains_params(GranularQuality::DirtyTape), SR);
    record(&mut old);
    let recorded = recording_level(&old, 0);
    assert!(recorded > 0);
    let mut new = construct(grains_params(GranularQuality::DirtyTape), SR);
    transfer(&mut old, &mut new);
    assert_eq!(recording_level(&new, 0), recorded);
    // Later patch updates keep it too.
    new.on_patch_update();
    assert_eq!(recording_level(&new, 0), recorded);
}

#[test]
fn a_fresh_dirty_tape_buffer_plays_silence() {
    let mut p = grains_params(GranularQuality::DirtyTape);
    p.mix = poly_volts(&[5.0]);
    p.time = poly_volts(&[5.0]);
    let mut m = make_grains(p, SR);
    for n in 0..24_000 {
        let trig = if n % 2400 < 10 { 5.0 } else { 0.0 };
        let (l, r) = step(&mut m, 0.0, trig, SR);
        assert!(l.abs() < 1e-6 && r.abs() < 1e-6, "{l} {r} at {n}");
    }
}

#[test]
fn new_voices_get_their_own_buffers() {
    let mut old = make_grains(grains_params(GranularQuality::Digital), SR);
    record(&mut old);
    let mut p = grains_params(GranularQuality::Digital);
    p.freq = poly_volts(&[0.0, 0.5]);
    let mut new = construct(p, SR);
    transfer(&mut old, &mut new);
    assert!(recording_level(&new, 0) > 0);
    assert_eq!(recording_level(&new, 1), 0);
    let buffer = &new.channel_state[1].voice.granular.buffer;
    assert_eq!(buffer.max_delay() as usize, Media::Digital.max_delay());
}

#[test]
fn narrowing_drops_triggers_for_voices_that_are_gone() {
    // At 44.1 kHz triggers wait for the resampled input, so some are still
    // queued at the patch update.
    let sr = 44_100.0;
    let mut p = grains_params(GranularQuality::Digital);
    p.freq = poly_volts(&[0.0, 0.25, 0.5, 0.75]);
    p.mix = poly_volts(&[5.0]);
    let mut old = make_grains(p, sr);
    for n in 0..4_000 {
        old.params.input = PolySignal::mono(Signal::Volts(sine(220.0, n, sr, 1.0)));
        old.params.trigger = poly_volts(&[if n % 7 == 0 { 5.0 } else { 0.0 }]);
        old.update(sr);
    }
    old.params.trigger = poly_volts(&[5.0]);
    old.update(sr);
    assert!(!old.state.pending.is_empty());
    let mut new = construct(grains_params(GranularQuality::Digital), sr);
    transfer(&mut old, &mut new);
    assert_eq!(new.channel_state.len(), 1);
    for n in 0..4_000 {
        let (l, r) = step(&mut new, sine(220.0, n, sr, 1.0), 0.0, sr);
        assert!(l.is_finite() && r.is_finite());
    }
}

#[test]
fn recovers_after_non_finite_input() {
    let mut p = grains_params(GranularQuality::Digital);
    p.mix = poly_volts(&[2.5]);
    let mut m = make_grains(p, SR);
    for _ in 0..64 {
        step(&mut m, f32::NAN, 0.0, SR);
    }
    for n in 0..4800 {
        let (l, r) = step(&mut m, sine(220.0, n, SR, 1.0), 0.0, SR);
        assert!(l.is_finite() && r.is_finite(), "non-finite output at {n}");
    }
    assert!(m.outputs.sample.get(0).abs() > 0.0);
}

#[test]
fn an_unspread_voice_outputs_its_stereo_left_channel() {
    for quality in ALL_QUALITIES {
        let mut p = grains_params(quality);
        p.mix = poly_volts(&[3.5]);
        p.feedback = poly_volts(&[2.0]);
        p.size = poly_volts(&[1.25]);
        p.width = volts(3.0);
        let mut m = make_grains(p, SR);
        for n in 0..30_000 {
            let x = if n < 2000 {
                0.0
            } else {
                sine(330.0, n, SR, 3.0)
            };
            let trig = if n >= 2000 && n % 1500 < 10 { 5.0 } else { 0.0 };
            step(&mut m, x, trig, SR);
            let (a, b) = (m.outputs.sample.get(0), m.outputs.voices.get(0));
            assert_eq!(a, b, "{quality:?}, sample {n}");
        }
    }
}

#[test]
fn a_new_fully_wet_module_passes_no_dry_signal() {
    for q in ALL_QUALITIES {
        let mut p = grains_params(q);
        p.mix = poly_volts(&[5.0]);
        p.width = volts(5.0);
        let mut m = make_grains(p, SR);
        for n in 0..4800 {
            let x = ((n as f32 * 130.8 / SR).fract() * 2.0 - 1.0) * 4.0;
            let (l, r) = step(&mut m, x, 0.0, SR);
            assert!(l.abs() < 1e-4 && r.abs() < 1e-4, "{q:?} at {n}: {l} {r}");
        }
    }
}

#[test]
fn a_voice_added_by_a_patch_update_passes_no_dry_signal() {
    let mut p = grains_params(GranularQuality::Digital);
    p.mix = poly_volts(&[5.0]);
    let mut old = make_grains(p.clone(), SR);
    for n in 0..4800 {
        step(&mut old, sine(220.0, n, SR, 4.0), 0.0, SR);
    }
    p.input = PolySignal::poly(&[Signal::Volts(0.0), Signal::Volts(0.0)]);
    let mut new = construct(p, SR);
    transfer(&mut old, &mut new);
    for n in 0..4800 {
        let x = Signal::Volts(sine(220.0, n, SR, 4.0));
        new.params.input = PolySignal::poly(&[x.clone(), x]);
        new.params.trigger = poly_volts(&[0.0]);
        new.update(SR);
        let v = new.outputs.voices.get(1);
        assert!(v.abs() < 1e-4, "sample {n}: {v}");
    }
}

#[test]
fn no_grain_leaves_above_the_limit_from_the_first() {
    // Four loud voices of short and long grains, panned hard, from the
    // module's first grain on. Digital runs at the engine rate, so nothing
    // after the limiters can overshoot them.
    for size in [0.0, 2.75] {
        let (quality, limit) = (GranularQuality::Digital, 5.0001);
        let mut p = grains_params(quality);
        p.input = PolySignal::poly(&vec![Signal::Volts(0.0); 4]);
        p.mix = poly_volts(&[5.0]);
        p.size = poly_volts(&[size]);
        p.shape = poly_volts(&[5.0]);
        p.spread = poly_volts(&[5.0]);
        p.width = volts(5.0);
        let mut m = make_grains(p, SR);
        for n in 0..96_000 {
            let x = ((n as f32 * 130.8 / SR).fract() * 2.0 - 1.0) * 5.0;
            m.params.input = PolySignal::poly(&vec![Signal::Volts(x); 4]);
            let trig = n >= 48_000 && n % 6000 < 10;
            m.params.trigger = poly_volts(&[if trig { 5.0 } else { 0.0 }]);
            m.update(SR);
            for ch in 0..2 {
                let y = m.outputs.sample.get(ch);
                assert!(y.abs() <= limit, "{quality:?} size {size}, sample {n}: {y}");
            }
            for v in 0..4 {
                let y = m.outputs.voices.get(v);
                assert!(
                    y.abs() <= limit,
                    "{quality:?} size {size}, voice {v}, sample {n}: {y}"
                );
            }
        }
    }
}

#[test]
fn width_places_the_voices_in_stereo() {
    let mut p = grains_params(GranularQuality::Digital);
    p.mix = poly_volts(&[0.0]);
    p.width = volts(5.0);
    p.input = PolySignal::poly(&[Signal::Volts(0.0), Signal::Volts(0.0)]);
    let mut m = make_grains(p, SR);
    let (mut l, mut r) = (Vec::new(), Vec::new());
    for n in 0..20_000 {
        let x = sine(440.0, n, SR, 2.0);
        m.params.input = PolySignal::poly(&[Signal::Volts(x), Signal::Volts(0.0)]);
        m.update(SR);
        if n >= 10_000 {
            l.push(m.outputs.sample.get(0));
            r.push(m.outputs.sample.get(1));
        }
    }
    assert!(rms(&l) > 0.5, "left {}", rms(&l));
    assert!(rms(&r) < 1e-4 * rms(&l), "right {}", rms(&r));
}

/// Render `params` over a 440 Hz input per channel and return each output
/// channel while the grains sound.
fn render(mut p: GranularParams, trigger: &[f32], channels: usize) -> Vec<Vec<f32>> {
    p.size = poly_volts(&[2.0]);
    p.shape = poly_volts(&[0.0]);
    p.time = poly_volts(&[1.0]);
    p.input = PolySignal::poly(&vec![Signal::Volts(0.0); channels]);
    p.trigger = poly_volts(&vec![0.0; trigger.len()]);
    let mut m = make_grains(p, SR);
    let low = vec![0.0; trigger.len()];
    let input = vec![Signal::Volts(0.0); channels];
    let mut out = vec![Vec::new(); m.channel_state.len()];
    for n in 0..60_000 {
        let x = sine(440.0, n, SR, 2.0);
        let mut input = input.clone();
        for s in input.iter_mut() {
            *s = Signal::Volts(x);
        }
        m.params.input = PolySignal::poly(&input);
        let level = if (48_000..48_010).contains(&n) {
            trigger
        } else {
            &low
        };
        m.params.trigger = poly_volts(level);
        m.update(SR);
        if n >= 48_100 {
            for (ch, o) in out.iter_mut().enumerate() {
                o.push(m.outputs.voices.get(ch));
            }
        }
    }
    out
}

#[test]
fn outputs_one_channel_per_voice() {
    let mut p = grains_params(GranularQuality::Digital);
    p.freq = poly_volts(&[0.0, 0.25, 0.5]);
    let m = make_grains(p, SR);
    assert_eq!(m.channel_state.len(), 3);
    assert_eq!(m.outputs.voices.channels(), 3);
    assert_eq!(m.outputs.sample.channels(), 2);
}

#[test]
fn a_poly_trigger_only_sounds_its_own_channel() {
    let mut p = grains_params(GranularQuality::Digital);
    p.mix = poly_volts(&[5.0]);
    let out = render(p, &[0.0, 5.0], 2);
    assert!(rms(&out[0]) < 1e-4, "untriggered channel {}", rms(&out[0]));
    assert!(rms(&out[1]) > 0.1, "triggered channel {}", rms(&out[1]));
}

#[test]
fn grain_levels_follow_their_own_voice() {
    let mut p = grains_params(GranularQuality::Digital);
    p.mix = poly_volts(&[5.0]);
    let alone = render(p.clone(), &[5.0, 0.0], 2);
    let together = render(p, &[5.0, 5.0], 2);
    assert_eq!(alone[0], together[0]);
    assert_eq!(together[0], together[1]);
}

#[test]
fn every_voice_carries_its_dry_signal() {
    let mut p = grains_params(GranularQuality::Digital);
    p.mix = poly_volts(&[0.0]);
    p.freq = poly_volts(&[0.0, 0.25, 0.5]);
    let out = render(p, &[0.0], 1);
    let level = rms(&out[0]);
    assert!(level > 0.5, "dry level {level}");
    assert_eq!(out[0], out[1]);
    assert_eq!(out[0], out[2]);
}

#[test]
fn dirty_tape_stays_finite_and_audible() {
    let mut p = grains_params(GranularQuality::DirtyTape);
    p.mix = poly_volts(&[3.0]);
    p.feedback = poly_volts(&[4.0]);
    let out = render(p, &[5.0, 5.0, 5.0], 3);
    for ch in out {
        assert!(ch.iter().all(|x| x.is_finite()));
        assert!(rms(&ch) > 0.1);
    }
}
