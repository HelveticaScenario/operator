use super::super::feedback::{MAX_LOOP_GAIN, MIN_INPUT_GAIN, input_gain, loop_gain};
use super::*;

#[test]
fn feedback_holds_average_power() {
    // A small impulse keeps the feedback clipper linear. Up to loop gain
    // 0.954 the input's scaling holds the power exactly.
    for feedback in [-3.0, 1.0, 3.0] {
        for stages in [1.0, 4.0, 25.5] {
            let c = Controls {
                feedback,
                ..controls(0.0, stages, 2.0)
            };
            let mut ch = channel();
            let e: f32 = (0..1 << 17)
                .map(|n| {
                    let x = if n == 0 { 0.01 } else { 0.0 };
                    let y = ch.step(x, c, 0, log2_c4_over_fs());
                    y * y
                })
                .sum();
            let gain = e / (0.01 * 0.01);
            assert!(
                (gain - 1.0).abs() < 2e-3,
                "feedback {feedback}, stages {stages}: power gain {gain}"
            );
        }
    }
}

#[test]
fn full_feedback_stays_bounded() {
    // A square wave and a sweep through the chain's resonances, at both
    // polarities of full feedback, on a long and a short resonant chain.
    for stages in [1.0, 50.0] {
        for feedback in [6.0, -6.0] {
            let mut ch = channel();
            let mut peak = 0.0f32;
            for n in 0..2 * SR as usize {
                let x = if (n / 50) % 2 == 0 { 5.0 } else { -5.0 };
                let c = Controls {
                    feedback,
                    ..controls((n as f32 / SR * 7.0).sin() * 3.0, stages, 5.0)
                };
                let y = ch.step(x, c, 0, log2_c4_over_fs());
                assert!(y.is_finite());
                peak = peak.max(y.abs());
            }
            assert!(
                peak < 15.0,
                "stages {stages}, feedback {feedback}: peak {peak}"
            );
        }
    }
}

#[test]
fn loop_gain_reaches_one_at_five_and_passes_it() {
    assert_eq!(loop_gain(0.0), 0.0);
    assert_eq!(loop_gain(5.0), 1.0);
    assert_eq!(loop_gain(-5.0), -1.0);
    assert!((loop_gain(6.0) - MAX_LOOP_GAIN).abs() < 1e-6);
    assert_eq!(loop_gain(9.0), loop_gain(6.0));
    assert!(loop_gain(4.9) < 1.0 && loop_gain(5.1) > 1.0);
    let mut previous = 0.0;
    for i in 1..=600 {
        let gain = loop_gain(i as f32 / 100.0);
        assert!(gain >= previous, "feedback {}", i as f32 / 100.0);
        previous = gain;
    }
    assert!((input_gain(0.0) - 1.0).abs() < 1e-6);
    assert!((input_gain(1.0) - MIN_INPUT_GAIN).abs() < 1e-6);
}

#[test]
fn each_step_of_feedback_rings_about_four_times_longer_until_endless() {
    // Passes for a ring to fall 60 dB: 4 at 1, 16 at 2, 68 at 3, 341 at 4,
    // then on toward endless at 5.
    let passes = |feedback: f32| -3.0 / loop_gain(feedback).log10();
    for (feedback, low, high) in [(1.0, 3.8, 4.2), (2.0, 3.8, 4.4), (3.0, 4.5, 5.5)] {
        let ratio = passes(feedback + 1.0) / passes(feedback);
        assert!(
            (low..high).contains(&ratio),
            "{feedback} to {}: {ratio}",
            feedback + 1.0
        );
    }
    assert!(passes(4.9) > 2_000.0);
}

/// Response to a small impulse, which keeps the feedback clipper linear.
fn ring(feedback: f32, seconds: f32) -> Vec<f32> {
    let c = Controls {
        feedback,
        ..controls(0.0, 8.0, 2.0)
    };
    let mut ch = channel();
    (0..(seconds * SR) as usize)
        .map(|n| ch.step(if n == 0 { 0.01 } else { 0.0 }, c, 0, log2_c4_over_fs()))
        .collect()
}

#[test]
fn feedback_five_rings_endlessly() {
    let y = ring(5.0, 3.0);
    let ratio = rms(&y, 2.0, 3.0) / rms(&y, 0.5, 1.5);
    assert!((ratio - 1.0).abs() < 0.05, "late over early {ratio}");
}

#[test]
fn feedback_below_five_decays() {
    let y = ring(4.0, 3.0);
    let ratio = rms(&y, 2.0, 3.0) / rms(&y, 0.5, 1.5);
    assert!(ratio < 0.5, "late over early {ratio}");
}

/// A second of self-oscillation from silence, after `settle` seconds: its
/// mean, RMS, and frequency from its zero crossings.
fn self_oscillation(stages: f32, resonance: f32, feedback: f32, settle: f32) -> (f32, f32, f32) {
    let c = Controls {
        feedback,
        ..controls(-1.0, stages, resonance)
    };
    let mut ch = channel();
    let y: Vec<f32> = (0..((settle + 1.0) * SR) as usize)
        .map(|_| ch.step(0.0, c, 0, log2_c4_over_fs()))
        .collect();
    let tail = &y[(settle * SR) as usize..];
    let mean = tail.iter().sum::<f32>() / tail.len() as f32;
    let rms =
        (tail.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / tail.len() as f32).sqrt();
    let crossings = tail
        .windows(2)
        .filter(|w| (w[0] - mean) * (w[1] - mean) < 0.0)
        .count();
    (mean, rms, crossings as f32 / 2.0)
}

#[test]
fn self_oscillation_starts_from_silence_at_the_cutoff() {
    // Four unresonant stages at C3 sing at C3, near the soft clip's level.
    let (mean, rms, hz) = self_oscillation(4.0, 0.0, 6.0, 2.0);
    let cutoff = C4_HZ_F32 / 2.0;
    assert!(mean.abs() < 0.05, "mean {mean}");
    assert!((1.0..5.0).contains(&rms), "rms {rms}");
    assert!((hz / cutoff - 1.0).abs() < 0.05, "{hz} Hz");
}

#[test]
fn self_oscillation_stays_clear_of_nyquist() {
    // The loop has modes far from the cutoff, toward DC and Nyquist; it
    // passes gain past 1 only around the cutoff, so it oscillates near there.
    for (stages, feedback) in [(20.0, -5.5), (20.0, 5.5), (20.5, 5.5)] {
        let (mean, rms, hz) = self_oscillation(stages, 2.0, feedback, 7.0);
        assert!(
            mean.abs() < 0.05,
            "{stages} stages, feedback {feedback}: mean {mean}"
        );
        assert!(rms > 0.5, "{stages} stages, feedback {feedback}: rms {rms}");
        assert!(
            hz < 1_000.0,
            "{stages} stages, feedback {feedback}: {hz} Hz"
        );
    }
}
