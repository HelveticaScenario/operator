use std::f32::consts::PI;

use super::*;
use crate::dsp::utils::hz_to_voct;
use crate::types::{OutputStruct, PatchUpdateHandler, Signal};

const SR: f32 = 48_000.0;

fn volts(v: f32) -> PolySignal {
    PolySignal::mono(Signal::Volts(v))
}

fn make(cutoff_hz: f32, resonance: f32, mode: f32) -> Multimode {
    let params = MultimodeParams {
        input: volts(0.0),
        cutoff: volts(hz_to_voct(cutoff_hz)),
        resonance: Some(volts(resonance)),
        mode: Some(volts(mode)),
        drive: None,
        fold: None,
        high_res: true,
    };
    let mut outputs = MultimodeOutputs::default();
    outputs.set_all_channels(1);
    let mut m = Multimode {
        params,
        outputs,
        _channel_count: 1,
        _block_index: Default::default(),
        state: MultimodeState::default(),
        channel_state: vec![ChannelState::default(); 1].into_boxed_slice(),
    };
    m.init(SR);
    m.on_patch_update();
    m
}

fn make_low_res(cutoff_hz: f32, resonance: f32, mode: f32) -> Multimode {
    let mut m = make(cutoff_hz, resonance, mode);
    m.params.high_res = false;
    m.on_patch_update();
    m
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

#[test]
fn lowpass_passband_and_rolloff_match_circuit() {
    let mut m = make(SPICE_FC, 0.0, 0.0);
    let pass = db(sine_peak(&mut m, 100.0, 1.0));
    assert!((pass - 2.79).abs() < 0.2, "100 Hz: {pass} dB");
    let mut m = make(SPICE_FC, 0.0, 0.0);
    let corner = db(sine_peak(&mut m, 1000.0, 1.0));
    assert!((corner + 2.35).abs() < 0.2, "1 kHz: {corner} dB");
    let mut m = make(SPICE_FC, 0.0, 0.0);
    let stop = db(sine_peak(&mut m, 3000.0, 1.0));
    assert!((stop + 16.69).abs() < 0.4, "3 kHz: {stop} dB");
}

#[test]
fn mode_midpoint_is_bandpass() {
    let mut m = make(SPICE_FC, 0.0, 2.5);
    let centre = db(sine_peak(&mut m, 1000.0, 1.0));
    assert!((centre + 2.92).abs() < 0.2, "1 kHz: {centre} dB");
    let mut m = make(SPICE_FC, 0.0, 2.5);
    let low = db(sine_peak(&mut m, 100.0, 1.0));
    assert!((low + 17.59).abs() < 0.3, "100 Hz: {low} dB");
}

#[test]
fn mode_full_is_highpass() {
    let mut m = make(SPICE_FC, 0.0, 5.0);
    let low = db(sine_peak(&mut m, 100.0, 1.0));
    assert!((low + 11.58).abs() < 0.3, "100 Hz: {low} dB");
    let mut m = make(SPICE_FC, 0.0, 5.0);
    let high = db(sine_peak(&mut m, 10_000.0, 1.0));
    assert!((high - 0.5).abs() < 0.5, "10 kHz: {high} dB");
}

#[test]
fn resonance_raises_the_peak() {
    // SPICE puts the peak at 1027 Hz: the zener capacitance pulls it below the
    // nominal 1047 Hz.
    let mut m = make(SPICE_FC, 3.0, 0.0);
    let peak = db(sine_peak(&mut m, 1027.0, 0.1));
    assert!((peak - 19.68).abs() < 0.5, "peak at resonance 3: {peak} dB");
}

#[test]
fn stays_silent_below_self_oscillation() {
    let mut m = make(500.0, 4.0, 0.0);
    let tail = free_run(&mut m, 2.0);
    let peak = tail[tail.len() / 2..]
        .iter()
        .fold(0.0_f32, |a, &v| a.max(v.abs()));
    assert!(peak < 1.0e-3, "peak {peak}");
}

#[test]
fn self_oscillates_at_cutoff_from_silence() {
    let mut m = make(440.0, 5.0, 0.0);
    let run = free_run(&mut m, 3.0);
    let tail = &run[run.len() / 2..];
    let peak = tail.iter().fold(0.0_f32, |a, &v| a.max(v.abs()));
    assert!((peak - 10.7).abs() < 0.6, "self-oscillation peak {peak}");
    let hz = zero_crossing_hz(tail);
    assert!(
        (hz / 440.0 - 1.0).abs() < 0.02,
        "self-oscillation at {hz} Hz"
    );
}

#[test]
fn self_oscillation_tops_out_near_eighteen_and_a_half_khz() {
    // SPICE: knob fully clockwise resonates at 18.59 kHz.
    let mut m = make(40_000.0, 5.0, 0.0);
    let run = free_run(&mut m, 1.0);
    let hz = zero_crossing_hz(&run[run.len() / 2..]);
    assert!((hz / 18_590.0 - 1.0).abs() < 0.01, "top cutoff {hz} Hz");
}

#[test]
fn self_oscillation_tracks_one_volt_per_octave_at_low_cutoffs() {
    let mut low = make(110.0, 5.0, 0.0);
    let mut high = make(220.0, 5.0, 0.0);
    let low_hz = zero_crossing_hz(&free_run(&mut low, 3.0)[(SR * 1.5) as usize..]);
    let high_hz = zero_crossing_hz(&free_run(&mut high, 3.0)[(SR * 1.5) as usize..]);
    let ratio = high_hz / low_hz;
    assert!((ratio - 2.0).abs() < 0.01, "octave ratio {ratio}");
}

#[test]
fn drive_is_unity_at_zero_and_cubic_above() {
    let mut unity = make(20_000.0, 0.0, 0.0);
    let mut full = make(20_000.0, 0.0, 0.0);
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

#[test]
fn hot_input_soft_clips_then_folds() {
    let mut clip = make(20_000.0, 0.0, 0.0);
    clip.params.drive = Some(volts(5.0));
    let clipped = 5.0 * sine_peak(&mut clip, 100.0, 5.0);
    // Zener clamp ±~5.6 V × clean path 0.719 × output 1.935.
    assert!((clipped - 7.8).abs() < 0.5, "clipped peak {clipped}");

    // A ±1 V sine at unity drive crosses the folder's first peak; at full fold
    // only the folded path, with its 15.6× makeup gain, reaches the filter.
    let mut fold = make(20_000.0, 0.0, 0.0);
    fold.params.fold = Some(volts(5.0));
    let folded = sine_peak(&mut fold, 100.0, 1.0);
    // The folder peaks at 0.186 V (x ≈ 0.45 V) in SPICE.
    let expected = 15.6 * 0.186 * OUTPUT_GAIN;
    assert!(
        (folded / expected - 1.0).abs() < 0.1,
        "folded gain {folded} vs {expected}"
    );
}

#[test]
fn outputs_stay_finite_under_extreme_modulation() {
    let mut m = make(1000.0, 0.0, 0.0);
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
            assert!(v.is_finite() && v.abs() <= RAIL_V, "sample {n}: {v}");
        }
    }
}

#[test]
fn low_res_keeps_the_response_and_tuning() {
    let mut m = make_low_res(SPICE_FC, 0.0, 0.0);
    let pass = db(sine_peak(&mut m, 100.0, 1.0));
    assert!((pass - 2.79).abs() < 0.2, "100 Hz: {pass} dB");
    let mut m = make_low_res(SPICE_FC, 0.0, 0.0);
    let corner = db(sine_peak(&mut m, 1000.0, 1.0));
    assert!((corner + 2.35).abs() < 0.3, "1 kHz: {corner} dB");

    let mut m = make_low_res(440.0, 5.0, 0.0);
    let run = free_run(&mut m, 3.0);
    let hz = zero_crossing_hz(&run[run.len() / 2..]);
    assert!(
        (hz / 440.0 - 1.0).abs() < 0.02,
        "self-oscillation at {hz} Hz"
    );
}

#[test]
fn switching_resolution_keeps_oscillating_at_the_same_pitch() {
    let mut m = make(440.0, 5.0, 0.0);
    free_run(&mut m, 1.0);
    m.params.high_res = false;
    m.on_patch_update();
    let run = free_run(&mut m, 1.0);
    let tail = &run[run.len() / 2..];
    let peak = tail.iter().fold(0.0_f32, |a, &v| a.max(v.abs()));
    assert!((peak - 10.7).abs() < 0.6, "peak {peak}");
    let hz = zero_crossing_hz(tail);
    assert!(
        (hz / 440.0 - 1.0).abs() < 0.02,
        "self-oscillation at {hz} Hz"
    );
}

#[test]
fn non_finite_signals_never_reach_the_state() {
    let mut m = make(1000.0, 3.0, 2.5);
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
