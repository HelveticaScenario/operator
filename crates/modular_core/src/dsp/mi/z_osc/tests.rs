use super::*;
use crate::dsp::mi::core::control::Ramp;
use crate::dsp::mi::core::fixture::{assert_rate_independent, max_error, records};
use crate::dsp::utils::voct_to_hz;
use crate::types::Signal;

fn mark() -> GlideMark {
    PolySignal::mono(Signal::Volts(0.0)).glide_mark(0)
}

/// Replays `grain.f32` (the Z oscillator of Plaits' granular formant engine)
/// through a voice ramping its pitch block by block as Plaits does.
#[test]
fn matches_plaits() {
    let state = ZOscState::new(48000.0);
    // Plaits starts its interpolated controls from zero.
    let mut voice = Voice {
        started: true,
        ..Voice::default()
    };
    let mut f0 = Ramp::default();
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for record in records("grain", 7 + 24) {
        let knobs = Knobs {
            cutoff: (record[4], mark()),
            shape: (record[5], mark()),
            mode: (record[6], mark()),
        };
        voice.tick(record[0], knobs, 12.0, &state);
        // Plaits ramps to a limited pitch, so the replayed ramp limits its targets.
        f0.set(record[0].min(0.125), Some(12.0));
        for &plaits in &record[19..31] {
            actual.push(voice.step(f0.next_value(), &state) / OUTPUT_GAIN);
            expected.push(plaits);
        }
    }
    let (error, at) = max_error(&actual, &expected);
    assert!(error < 1e-5, "max error {error} at sample {at}");
}

/// Under through-zero FM the waveform runs backward: the output stays bounded
/// and its band-limited restarts never jump.
#[test]
fn runs_backward() {
    let state = ZOscState::new(48000.0);
    for f0 in [-0.003, -0.02, -0.11] {
        let knobs = || Knobs {
            cutoff: (2.0, mark()),
            shape: (1.0, mark()),
            mode: (1.0, mark()),
        };
        let mut voice = Voice::default();
        voice.tick(f0, knobs(), 12.0, &state);
        let out: Vec<f32> = (0..4800).map(|_| voice.step(f0, &state)).collect();
        let mut forward = Voice::default();
        forward.tick(-f0, knobs(), 12.0, &state);
        let reference: Vec<f32> = (0..4800).map(|_| forward.step(-f0, &state)).collect();
        let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let jump = |x: &[f32]| x.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(peak(&out) < 1.5 * peak(&reference), "f0 {f0}");
        assert!(
            jump(&out) < 1.5 * jump(&reference),
            "f0 {f0} jump {} vs {} peak {} vs {}",
            jump(&out),
            jump(&reference),
            peak(&out),
            peak(&reference)
        );
    }
}

#[test]
fn sounds_the_same_at_any_rate() {
    for (pitch, cutoff, shape, mode) in [(-1.0, 2.0, 1.0, 1.0), (2.0, 4.0, 4.0, 4.0)] {
        assert_rate_independent(
            |sample_rate| {
                let mut state = ZOscState::new(sample_rate);
                let mut voice = Voice::default();
                let f0 = voct_to_hz(pitch) / sample_rate;
                (0..(sample_rate * 0.5) as usize)
                    .map(|_| {
                        if let (Some(length), _) = state.clock.advance(1, sample_rate) {
                            let knobs = Knobs {
                                cutoff: (cutoff, mark()),
                                shape: (shape, mark()),
                                mode: (mode, mark()),
                            };
                            voice.tick(f0, knobs, length, &state);
                        }
                        voice.step(f0, &state)
                    })
                    .collect()
            },
            voct_to_hz(pitch),
            0.02,
        );
    }
}
