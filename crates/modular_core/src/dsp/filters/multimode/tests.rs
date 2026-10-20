use std::f32::consts::PI;

use super::*;
use crate::dsp::utils::hz_to_voct;
use crate::types::{OutputStruct, PatchUpdateHandler, Signal};

const SR: f32 = 48_000.0;

fn volts(v: f32) -> PolySignal {
    PolySignal::mono(Signal::Volts(v))
}

fn make(high_res: bool, cutoff_hz: f32, resonance: f32, mode: f32) -> Multimode {
    let params = MultimodeParams {
        input: volts(0.0),
        cutoff: volts(hz_to_voct(cutoff_hz)),
        resonance: Some(volts(resonance)),
        mode: Some(volts(mode)),
        drive: None,
        fold: None,
        high_res,
    };
    let mut outputs = MultimodeOutputs::default();
    outputs.set_all_channels(1);
    let mut m = Multimode {
        params,
        outputs,
        _channel_count: 1,
        _block_index: Default::default(),
        state: MultimodeState::default(),
        channel_state: vec![GroupState::default(); 1].into_boxed_slice(),
    };
    m.init(SR);
    m.on_patch_update();
    m
}

/// A module with one channel per `(cutoff_hz, resonance, mode)`.
fn make_channels(high_res: bool, channels: &[(f32, f32, f32)]) -> Multimode {
    let poly = |f: fn(&(f32, f32, f32)) -> f32| {
        PolySignal::poly(
            &channels
                .iter()
                .map(|c| Signal::Volts(f(c)))
                .collect::<Vec<_>>(),
        )
    };
    let n = channels.len();
    let mut outputs = MultimodeOutputs::default();
    outputs.set_all_channels(n);
    let mut m = Multimode {
        params: MultimodeParams {
            input: volts(0.0),
            cutoff: poly(|c| hz_to_voct(c.0)),
            resonance: Some(poly(|c| c.1)),
            mode: Some(poly(|c| c.2)),
            drive: None,
            fold: None,
            high_res,
        },
        outputs,
        _channel_count: n,
        _block_index: Default::default(),
        state: MultimodeState::default(),
        channel_state: vec![GroupState::default(); n].into_boxed_slice(),
    };
    m.init(SR);
    m.on_patch_update();
    m
}

/// Moves `old`'s state into `new` as a patch update does: the whole `state`,
/// and the overlapping `channel_state` entries.
fn transfer(new: &mut Multimode, old: &mut Multimode) {
    std::mem::swap(&mut new.state, &mut old.state);
    let n = new.channel_state.len().min(old.channel_state.len());
    new.channel_state[..n].swap_with_slice(&mut old.channel_state[..n]);
    new.on_patch_update();
}
/// Steady-state peak gain for a sine of `amp` volts at `hz`.
fn sine_peak(m: &mut Multimode, hz: f32, amp: f32) -> f32 {
    let settle = (SR * 0.5) as usize;
    let mut peak = 0.0_f32;
    for n in 0..settle * 2 {
        m.params.input = volts(amp * (2.0 * PI * hz * n as f32 / SR).sin());
        m.update(SR);
        if n >= settle {
            peak = peak.max(m.outputs.sample.get(0).abs());
        }
    }
    peak / amp
}

/// Steady-state RMS gain for a 0.5 V sine at `hz`, which stays accurate near
/// Nyquist where sampled peaks miss the waveform's crest.
fn rms_gain(m: &mut Multimode, hz: f32) -> f32 {
    let settle = (SR * 0.25) as usize;
    let (mut sx, mut sy) = (0.0_f64, 0.0_f64);
    for n in 0..settle * 4 {
        let x = 0.5 * (2.0 * PI * hz * n as f32 / SR).sin();
        m.params.input = volts(x);
        m.update(SR);
        if n >= settle {
            sx += (x as f64).powi(2);
            sy += (m.outputs.sample.get(0) as f64).powi(2);
        }
    }
    (sy / sx).sqrt() as f32
}

/// Runs `seconds` with no input, returning the output samples.
fn free_run(m: &mut Multimode, seconds: f32) -> Vec<f32> {
    (0..(SR * seconds) as usize)
        .map(|_| {
            m.update(SR);
            m.outputs.sample.get(0)
        })
        .collect()
}

fn zero_crossing_hz(samples: &[f32]) -> f32 {
    let ups: Vec<usize> = (1..samples.len())
        .filter(|&i| samples[i - 1] < 0.0 && samples[i] >= 0.0)
        .collect();
    let periods = (ups.len() - 1) as f32;
    SR * periods / (ups[ups.len() - 1] - ups[0]) as f32
}

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

// Reference gains below come from an ngspice simulation of the transcribed
// schematic: frequency knob at −2 V on its wiper (1047 Hz), unity drive,
// soft-clip response.
const SPICE_FC: f32 = 1047.0;

/// Expands each test body into a `high_res` and a `low_res` test, so every
/// behaviour is checked at both core rates.
macro_rules! both_resolutions {
    ($(fn $name:ident($high_res:ident: bool) $body:block)*) => {$(
        mod $name {
            use super::*;

            fn run($high_res: bool) $body

            #[test]
            fn high_res() {
                run(true)
            }

            #[test]
            fn low_res() {
                run(false)
            }
        }
    )*};
}

both_resolutions! {
    fn lowpass_passband_and_rolloff_match_circuit(high_res: bool) {
        let mut m = make(high_res, SPICE_FC, 0.0, 0.0);
        let pass = db(sine_peak(&mut m, 100.0, 1.0));
        assert!((pass - 2.79).abs() < 0.2, "100 Hz: {pass} dB");
        let mut m = make(high_res, SPICE_FC, 0.0, 0.0);
        let corner = db(sine_peak(&mut m, 1000.0, 1.0));
        assert!((corner + 2.35).abs() < 0.2, "1 kHz: {corner} dB");
        let mut m = make(high_res, SPICE_FC, 0.0, 0.0);
        let stop = db(sine_peak(&mut m, 3000.0, 1.0));
        assert!((stop + 16.69).abs() < 0.4, "3 kHz: {stop} dB");
    }

    fn mode_midpoint_is_bandpass(high_res: bool) {
        let mut m = make(high_res, SPICE_FC, 0.0, 2.5);
        let centre = db(sine_peak(&mut m, 1000.0, 1.0));
        assert!((centre + 2.92).abs() < 0.2, "1 kHz: {centre} dB");
        let mut m = make(high_res, SPICE_FC, 0.0, 2.5);
        let low = db(sine_peak(&mut m, 100.0, 1.0));
        assert!((low + 17.59).abs() < 0.3, "100 Hz: {low} dB");
    }

    fn mode_full_is_highpass(high_res: bool) {
        let mut m = make(high_res, SPICE_FC, 0.0, 5.0);
        let low = db(sine_peak(&mut m, 100.0, 1.0));
        assert!((low + 11.58).abs() < 0.3, "100 Hz: {low} dB");
        let mut m = make(high_res, SPICE_FC, 0.0, 5.0);
        let high = db(sine_peak(&mut m, 10_000.0, 1.0));
        // At the engine rate the pre-emphasis leaves ~0.3 dB of extra lift here.
        let tol = if high_res { 0.3 } else { 0.6 };
        assert!((high - 1.46).abs() < tol, "10 kHz: {high} dB");
    }

    fn resonance_raises_the_peak(high_res: bool) {
        // SPICE puts the peak at 1027 Hz: the zener capacitance pulls it below the
        // nominal 1047 Hz.
        let mut m = make(high_res, SPICE_FC, 3.0, 0.0);
        let peak = db(sine_peak(&mut m, 1027.0, 0.1));
        assert!((peak - 19.68).abs() < 0.5, "peak at resonance 3: {peak} dB");
    }

    fn stays_silent_below_self_oscillation(high_res: bool) {
        let mut m = make(high_res, 500.0, 4.0, 0.0);
        let tail = free_run(&mut m, 2.0);
        let peak = tail[tail.len() / 2..]
            .iter()
            .fold(0.0_f32, |a, &v| a.max(v.abs()));
        assert!(peak < 1.0e-3, "peak {peak}");
    }

    fn self_oscillates_at_cutoff_from_silence(high_res: bool) {
        let mut m = make(high_res, 440.0, 5.0, 0.0);
        let run = free_run(&mut m, 3.0);
        let tail = &run[run.len() / 2..];
        let peak = tail.iter().fold(0.0_f32, |a, &v| a.max(v.abs()));
        // SPICE: 7.34 V, the bandpass held near ±4 V by the zeners' soft knee.
        assert!((peak - 7.34).abs() < 0.2, "self-oscillation peak {peak}");
        let hz = zero_crossing_hz(tail);
        assert!(
            (hz / 440.0 - 1.0).abs() < 0.02,
            "self-oscillation at {hz} Hz"
        );
    }

    fn self_oscillation_tops_out_near_twenty_one_khz(high_res: bool) {
        // SPICE: knob fully clockwise resonates at 21.03 kHz, where the zener
        // capacitance has shrunk under the full bandpass swing.
        let mut m = make(high_res, 40_000.0, 5.0, 0.0);
        let run = free_run(&mut m, 1.0);
        let hz = zero_crossing_hz(&run[run.len() / 2..]);
        assert!((hz / 21_030.0 - 1.0).abs() < 0.01, "top cutoff {hz} Hz");
    }

    fn self_oscillation_tracks_one_volt_per_octave_at_low_cutoffs(high_res: bool) {
        let mut low = make(high_res, 110.0, 5.0, 0.0);
        let mut high = make(high_res, 220.0, 5.0, 0.0);
        let low_hz = zero_crossing_hz(&free_run(&mut low, 3.0)[(SR * 1.5) as usize..]);
        let high_hz = zero_crossing_hz(&free_run(&mut high, 3.0)[(SR * 1.5) as usize..]);
        let ratio = high_hz / low_hz;
        assert!((ratio - 2.0).abs() < 0.01, "octave ratio {ratio}");
    }

    fn drive_is_unity_at_zero_and_cubic_above(high_res: bool) {
        let mut unity = make(high_res, 20_000.0, 0.0, 0.0);
        let mut full = make(high_res, 20_000.0, 0.0, 0.0);
        full.params.drive = Some(volts(5.0));
        let a = sine_peak(&mut unity, 100.0, 0.1);
        let b = sine_peak(&mut full, 100.0, 0.1);
        assert!((db(a) - 2.9).abs() < 0.2, "unity passband {} dB", db(a));
        let expected = 2.0_f32.powf(DRIVE_EXPONENT);
        assert!(
            (b / a - expected).abs() < 0.01,
            "ratio {} vs {expected}",
            b / a
        );
    }

    fn hot_input_soft_clips_then_folds(high_res: bool) {
        let mut clip = make(high_res, 20_000.0, 0.0, 0.0);
        clip.params.drive = Some(volts(5.0));
        let clipped = 5.0 * sine_peak(&mut clip, 100.0, 5.0);
        // Zener clamp ±5.7 V × clean path 0.719 × output gain 1.935 asks for
        // 7.9 V, which the output amp's zeners soften.
        assert!((clipped - 7.39).abs() < 0.1, "clipped peak {clipped}");

        // A ±1 V sine at unity drive crosses the folder's first peak; at full fold
        // only the folded path, with its 15.6× makeup gain, reaches the filter.
        let mut fold = make(high_res, 20_000.0, 0.0, 0.0);
        fold.params.fold = Some(volts(5.0));
        let folded = sine_peak(&mut fold, 100.0, 1.0);
        // The folder peaks at 0.186 V (x ≈ 0.45 V) in SPICE.
        let expected = 15.6 * 0.186 * output::OUTPUT_GAIN;
        assert!(
            (folded / expected - 1.0).abs() < 0.1,
            "folded gain {folded} vs {expected}"
        );
    }

    fn outputs_stay_finite_under_extreme_modulation(high_res: bool) {
        let mut m = make(high_res, 1000.0, 0.0, 0.0);
        for n in 0..48_000 {
            let t = n as f32 / SR;
            m.params.input = volts(50.0 * (2.0 * PI * 3000.0 * t).sin());
            m.params.cutoff = volts(20.0 * (2.0 * PI * 500.0 * t).sin());
            m.params.resonance = Some(volts(20.0 * (2.0 * PI * 7.0 * t).sin()));
            m.params.mode = Some(volts(10.0 * (2.0 * PI * 3.0 * t).sin()));
            m.params.drive = Some(volts(20.0));
            m.params.fold = Some(volts(5.0 + 5.0 * (2.0 * PI * t).sin()));
            m.update(SR);
            for out in [
                &m.outputs.sample,
                &m.outputs.lp,
                &m.outputs.bp,
                &m.outputs.hp,
            ] {
                let v = out.get(0);
                assert!(
                    (output::RAIL_NEG..=output::RAIL_POS).contains(&v),
                    "sample {n}: {v}"
                );
            }
        }
    }

    fn non_finite_signals_never_reach_the_state(high_res: bool) {
        let mut m = make(high_res, 1000.0, 3.0, 2.5);
        for n in 0..4800 {
            let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY][n % 3];
            let t = n as f32 / SR;
            let hit = |k: usize| n % 7 == k;
            m.params.input = volts(if hit(0) {
                bad
            } else {
                5.0 * (2.0 * PI * 220.0 * t).sin()
            });
            m.params.cutoff = volts(if hit(1) { bad } else { 1.0 });
            m.params.fold = Some(volts(if hit(2) { bad } else { 2.0 }));
            m.params.drive = Some(volts(if hit(3) { bad } else { 2.0 }));
            m.params.resonance = Some(volts(if hit(4) { bad } else { 4.8 }));
            m.params.mode = Some(volts(if hit(5) { bad } else { 2.5 }));
            m.update(SR);
            let v = m.outputs.sample.get(0);
            assert!(v.is_finite(), "sample {n}: {v}");
        }
        // After the bad values stop, the filter still passes signal.
        m.params.cutoff = volts(1.0);
        m.params.fold = Some(volts(0.0));
        m.params.drive = Some(volts(0.0));
        m.params.resonance = Some(volts(0.0));
        m.params.mode = Some(volts(0.0));
        assert!(sine_peak(&mut m, 100.0, 1.0) > 1.0);
    }
}

both_resolutions! {
    fn each_lane_matches_a_single_channel(high_res: bool) {
        // Five channels: a full group of four, then one beside three dead lanes.
        let settings = [
            (220.0, 0.0, 0.0),
            (1_000.0, 3.0, 2.5),
            (5_000.0, 2.0, 5.0),
            (12_000.0, 3.5, 1.0),
            (440.0, 1.0, 4.0),
        ];
        let mut poly = make_channels(high_res, &settings);
        let mut monos: Vec<_> = settings
            .iter()
            .map(|&(hz, res, mode)| make(high_res, hz, res, mode))
            .collect();
        for n in 0..4800 {
            let t = n as f32 / SR;
            let x = 4.0 * (2.0 * PI * 110.0 * t).sin() + 2.0 * (2.0 * PI * 1730.0 * t).sin();
            poly.params.input = volts(x);
            poly.update(SR);
            for (ch, mono) in monos.iter_mut().enumerate() {
                mono.params.input = volts(x);
                mono.update(SR);
                let pairs = [
                    (&poly.outputs.sample, &mono.outputs.sample),
                    (&poly.outputs.lp, &mono.outputs.lp),
                    (&poly.outputs.bp, &mono.outputs.bp),
                    (&poly.outputs.hp, &mono.outputs.hp),
                ];
                for (a, b) in pairs {
                    // The lanes' noise generators are seeded per channel.
                    let (a, b) = (a.get(ch), b.get(0));
                    assert!((a - b).abs() < 1.0e-3, "sample {n} channel {ch}: {a} vs {b}");
                }
            }
        }
    }

    fn outputs_stay_continuous_into_and_out_of_a_lone_channel(high_res: bool) {
        // Channels carried across a change that leaves a group with one live
        // channel, or takes it out of one, continue exactly as channels that
        // ran at the new count throughout.
        let input = |n: usize| volts(4.0 * (2.0 * PI * 110.0 * n as f32 / SR).sin());
        let setting = (2_000.0, 3.5, 2.5);
        for (before, after) in [(1, 2), (2, 1), (5, 6), (6, 5)] {
            let mut switched = make_channels(high_res, &vec![setting; before]);
            let mut reference = make_channels(high_res, &vec![setting; after]);
            for n in 0..2400 {
                switched.params.input = input(n);
                switched.update(SR);
                reference.params.input = input(n);
                reference.update(SR);
            }
            let mut carried = make_channels(high_res, &vec![setting; after]);
            transfer(&mut carried, &mut switched);
            for n in 2400..2600 {
                carried.params.input = input(n);
                carried.update(SR);
                reference.params.input = input(n);
                reference.update(SR);
                for ch in 0..before.min(after) {
                    let outputs = |m: &Multimode| {
                        let o = &m.outputs;
                        [o.sample.get(ch), o.lp.get(ch), o.bp.get(ch), o.hp.get(ch)]
                    };
                    for (a, b) in outputs(&carried).into_iter().zip(outputs(&reference)) {
                        assert!(
                            (a - b).abs() < 1.0e-3,
                            "{before}→{after} sample {n} channel {ch}: {a} vs {b}"
                        );
                    }
                }
            }
        }
    }

    fn channels_added_by_a_patch_update_start_from_rest(high_res: bool) {
        // Three self-oscillating channels leave the group's fourth lane dead.
        let mut old = make_channels(high_res, &[(440.0, 5.0, 0.0); 3]);
        free_run(&mut old, 1.0);
        let mut new = make_channels(high_res, &[(440.0, 5.0, 0.0); 4]);
        transfer(&mut new, &mut old);
        let mut peaks = [0.0_f32; 4];
        for _ in 0..(SR * 0.005) as usize {
            new.update(SR);
            for (ch, peak) in peaks.iter_mut().enumerate() {
                *peak = peak.max(new.outputs.sample.get(ch).abs());
            }
        }
        // The carried channels keep ringing; the new one starts silent and
        // grows from the noise floor.
        for (ch, &peak) in peaks[..3].iter().enumerate() {
            assert!(peak > 5.0, "channel {ch} peak {peak}");
        }
        assert!(peaks[3] < 0.1, "new channel peak {}", peaks[3]);
    }
}

#[test]
fn switching_resolution_keeps_oscillating_at_the_same_pitch() {
    let mut m = make(true, 440.0, 5.0, 0.0);
    free_run(&mut m, 1.0);
    m.params.high_res = false;
    m.on_patch_update();
    let run = free_run(&mut m, 1.0);
    let tail = &run[run.len() / 2..];
    let peak = tail.iter().fold(0.0_f32, |a, &v| a.max(v.abs()));
    assert!((peak - 7.34).abs() < 0.2, "peak {peak}");
    let hz = zero_crossing_hz(tail);
    assert!(
        (hz / 440.0 - 1.0).abs() < 0.02,
        "self-oscillation at {hz} Hz"
    );
}

#[test]
fn low_res_treble_tracks_high_res() {
    // Highpass passes the drive stage's treble straight through, so it shows
    // the pre-emphasis against ADAA's roll-off; above 14 kHz the engine-rate
    // output pole (19.5 kHz) and ADAA's residual roll-off leave up to 1 dB.
    // Bandpass also shows the core's frequency warping at the engine rate.
    for (mode, top_hz) in [(5.0, 16_000.0), (2.5, 10_000.0)] {
        let mut hz = 1000.0;
        while hz <= top_hz {
            let high = db(rms_gain(&mut make(true, SPICE_FC, 0.0, mode), hz));
            let low = db(rms_gain(&mut make(false, SPICE_FC, 0.0, mode), hz));
            let tol = match (mode == 5.0, hz > 14_000.0) {
                (true, false) => 0.6,
                (true, true) => 1.0,
                (false, _) => 0.8,
            };
            assert!(
                (low - high).abs() < tol,
                "mode {mode} at {hz} Hz: low {low} dB, high {high} dB"
            );
            hz += 1000.0;
        }
    }
}
