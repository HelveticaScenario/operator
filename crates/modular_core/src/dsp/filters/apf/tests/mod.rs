mod crossfade;
mod feedback;

use super::{
    channel::{AllpassChannel, Controls},
    stage::{MAX_Q, damping, prewarp},
    *,
};

pub(super) const SR: f32 = 48_000.0;

pub(super) fn log2_c4_over_fs() -> f32 {
    (C4_HZ_F32 / SR).log2()
}

pub(super) fn controls(cutoff: f32, stages: f32, resonance: f32) -> Controls {
    Controls {
        cutoff,
        stages,
        resonance,
        feedback: 0.0,
    }
}

pub(super) fn channel() -> AllpassChannel {
    AllpassChannel::default()
}

/// Impulse response of one channel held at `c`.
pub(super) fn impulse_response(ch: &mut AllpassChannel, c: Controls, len: usize) -> Vec<f32> {
    faded_impulse_response(c, 0, len, ch)
}

/// Impulse response of one channel held at `c` with crossfade step
/// `fade_step`.
pub(super) fn faded_impulse_response(
    c: Controls,
    fade_step: usize,
    len: usize,
    ch: &mut AllpassChannel,
) -> Vec<f32> {
    (0..len)
        .map(|n| {
            let x = if n == 0 { 1.0 } else { 0.0 };
            ch.step(x, c, fade_step, log2_c4_over_fs())
        })
        .collect()
}

pub(super) fn energy(response: &[f32]) -> f32 {
    response.iter().map(|y| y * y).sum()
}

/// RMS of `y` over `from..to` seconds.
pub(super) fn rms(y: &[f32], from: f32, to: f32) -> f32 {
    let window = &y[(from * SR) as usize..(to * SR) as usize];
    (window.iter().map(|v| v * v).sum::<f32>() / window.len() as f32).sqrt()
}

#[test]
fn chain_passes_all_energy() {
    for stages in [1.0, 4.0, 50.0] {
        for resonance in [0.0, 2.5, 5.0] {
            let e = energy(&impulse_response(
                &mut channel(),
                controls(0.0, stages, resonance),
                1 << 16,
            ));
            assert!(
                (e - 1.0).abs() < 1e-3,
                "stages {stages} resonance {resonance}: energy {e}"
            );
        }
    }
}

/// One sample through a first-order allpass pole with memory `s` and
/// coefficient `a`.
fn single_pole(s: &mut f32, a: f32, x: f32) -> f32 {
    let y = a * x + *s;
    *s = x - a * y;
    y
}

#[test]
fn unresonant_stage_is_two_single_poles() {
    let pair = impulse_response(&mut channel(), controls(1.0, 1.0, 0.0), 512);
    let g = (std::f32::consts::PI * C4_HZ_F32 * 2.0 / SR).tan();
    let a = (g - 1.0) / (g + 1.0);
    let (mut s1, mut s2) = (0.0, 0.0);
    for (n, y) in pair.iter().enumerate() {
        let x = if n == 0 { 1.0 } else { 0.0 };
        let expected = single_pole(&mut s2, a, single_pole(&mut s1, a, x));
        assert!((y - expected).abs() < 1e-4, "sample {n}: {y} vs {expected}");
    }
}

#[test]
fn fractional_stages_pass_all_energy() {
    for stages in [1.25, 1.5, 2.6, 3.4, 49.75] {
        for resonance in [0.0, 2.0, 4.0] {
            let c = controls(0.0, stages, resonance);
            let e = energy(&impulse_response(&mut channel(), c, 1 << 16));
            assert!(
                (e - 1.0).abs() < 1e-3,
                "stages {stages} resonance {resonance}: energy {e}"
            );
        }
    }
}

#[test]
fn fractional_stages_keep_a_sine_at_cutoff_level() {
    let omega = std::f32::consts::TAU * C4_HZ_F32 / SR;
    for stages in [1.25, 1.5, 2.75] {
        let mut ch = channel();
        let c = controls(0.0, stages, 3.0);
        let (mut input, mut output) = (0.0, 0.0);
        for n in 0..SR as usize {
            let x = (omega * n as f32).sin();
            let y = ch.step(x, c, 0, log2_c4_over_fs());
            if n >= SR as usize / 2 {
                input += x * x;
                output += y * y;
            }
        }
        let ratio = (output / input).sqrt();
        assert!((ratio - 1.0).abs() < 1e-3, "stages {stages}: level {ratio}");
    }
}

#[test]
fn fractional_stages_meet_their_neighbours() {
    for whole in [1.0, 3.0, 49.0] {
        for resonance in [0.0, 4.0] {
            let response =
                |stages| impulse_response(&mut channel(), controls(0.5, stages, resonance), 4096);
            for (stages, neighbour) in [(whole + 1e-4, whole), (whole + 1.0 - 1e-4, whole + 1.0)] {
                let distance: f32 = response(stages)
                    .iter()
                    .zip(response(neighbour))
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum();
                assert!(
                    distance < 1e-4,
                    "{stages} stages vs {neighbour}, resonance {resonance}: {distance}"
                );
            }
        }
    }
}

#[test]
fn prewarp_and_damping_stay_in_range() {
    assert!((prewarp(-2.0) - (std::f32::consts::PI / 4.0).tan()).abs() < 1e-6);
    assert!((prewarp(0.0) - prewarp(0.49f32.log2())).abs() < 1e-3);
    assert!(prewarp(-1000.0) > 0.0);
    assert!((damping(0.0) - 2.0).abs() < 1e-6);
    assert!((damping(5.0) - 1.0 / MAX_Q).abs() < 1e-6);
}

/// Group delay in seconds of impulse response `h` at `hz`.
fn group_delay(h: &[f32], hz: f32) -> f32 {
    let phase = |omega: f64| {
        let (re, im) = h.iter().enumerate().fold((0.0, 0.0), |(re, im), (n, &y)| {
            let (sin, cos) = (omega * n as f64).sin_cos();
            (re + y as f64 * cos, im - y as f64 * sin)
        });
        f64::atan2(im, re)
    };
    let omega = std::f64::consts::TAU * hz as f64 / SR as f64;
    let delta = 1e-5;
    let mut step = phase(omega + delta) - phase(omega - delta);
    if step > std::f64::consts::PI {
        step -= std::f64::consts::TAU;
    } else if step < -std::f64::consts::PI {
        step += std::f64::consts::TAU;
    }
    (-step / (2.0 * delta) / SR as f64) as f32
}

#[test]
fn cutoff_delay_is_poles_over_two_pi_cutoff() {
    // 50 stages, 100 poles, at C3: about 120 ms.
    let hz = C4_HZ_F32 / 2.0;
    let h = impulse_response(&mut channel(), controls(-1.0, 50.0, 0.0), 1 << 15);
    let delay = group_delay(&h, hz);
    let expected = 100.0 / (std::f32::consts::TAU * hz);
    assert!(
        (delay / expected - 1.0).abs() < 0.02,
        "{delay} s, expected {expected} s"
    );
}

#[test]
fn full_resonance_delays_the_cutoff_forty_times_longer() {
    let delay = |resonance| {
        let h = impulse_response(&mut channel(), controls(1.0, 10.0, resonance), 1 << 17);
        group_delay(&h, C4_HZ_F32 * 2.0)
    };
    let ratio = delay(5.0) / delay(0.0);
    assert!((ratio / 40.0 - 1.0).abs() < 0.02, "ratio {ratio}");
}

#[test]
fn extreme_cutoffs_stay_finite() {
    for cutoff in [-1000.0, 1000.0] {
        let mut ch = channel();
        for n in 0..256 {
            let y = ch.step(
                (n as f32).sin(),
                controls(cutoff, 3.5, 2.0),
                0,
                log2_c4_over_fs(),
            );
            assert!(y.is_finite(), "cutoff {cutoff}");
        }
    }
}

#[test]
fn gliding_stages_do_not_ring_at_nyquist() {
    // A low sine while `stages` sweeps up and down. A stage gliding in starts
    // near Nyquist; started off its input, it rings there at many times the
    // sine's own curvature.
    for (from, to) in [(1.0, 6.0), (6.0, 1.0)] {
        let mut ch = channel();
        let len = 4 * SR as usize;
        let (mut y1, mut y2) = (0.0f32, 0.0f32);
        let mut largest = 0.0f32;
        for n in 0..len {
            let x = (std::f32::consts::TAU * 50.0 * n as f32 / SR).sin();
            let stages = from + (to - from) * n as f32 / len as f32;
            let y = ch.step(x, controls(0.0, stages, 1.0), 0, log2_c4_over_fs());
            // Past the chain's start-up transient.
            if n > SR as usize / 10 {
                largest = largest.max((y - 2.0 * y1 + y2).abs());
            }
            (y2, y1) = (y1, y);
        }
        assert!(largest < 1e-3, "{from} to {to}: curvature {largest}");
    }
}

#[test]
fn gliding_stage_follows_a_cutoff_change_made_at_a_whole_count() {
    // A sine's steady state after a cutoff change made while `stages` sat on
    // a whole count matches a channel that only ever saw the new cutoff.
    let sine = |n: usize| (std::f32::consts::TAU * 1_000.0 * n as f32 / SR).sin();
    let len = SR as usize / 2;
    let mut moved = channel();
    for n in 0..len {
        let c = match n {
            0..1000 => controls(0.0, 2.5, 1.0),
            1000..2000 => controls(1.0, 3.0, 1.0),
            _ => controls(1.0, 2.5, 1.0),
        };
        moved.step(sine(n), c, 0, log2_c4_over_fs());
    }
    let mut fresh = channel();
    for n in 0..len {
        fresh.step(sine(n), controls(1.0, 2.5, 1.0), 0, log2_c4_over_fs());
    }
    for n in len..len + 100 {
        let c = controls(1.0, 2.5, 1.0);
        let (a, b) = (
            moved.step(sine(n), c, 0, log2_c4_over_fs()),
            fresh.step(sine(n), c, 0, log2_c4_over_fs()),
        );
        assert!((a - b).abs() < 1e-3, "sample {n}: {a} vs {b}");
    }
}

#[test]
fn recovers_from_an_input_that_overflows_the_stages() {
    let mut ch = channel();
    let c = controls(0.0, 4.0, 5.0);
    for _ in 0..16 {
        ch.step(f32::MAX, c, 0, log2_c4_over_fs());
    }
    let y: Vec<f32> = (0..SR as usize / 10)
        .map(|n| {
            let x = (std::f32::consts::TAU * 200.0 * n as f32 / SR).sin();
            ch.step(x, c, 0, log2_c4_over_fs())
        })
        .collect();
    assert!(y.iter().all(|v| v.is_finite()));
    assert!(rms(&y, 0.05, 0.1) > 0.1, "silent after overflow");
}
