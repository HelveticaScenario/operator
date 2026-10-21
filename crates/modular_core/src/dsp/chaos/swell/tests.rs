use super::*;
use crate::dsp::utils::hz_to_voct;
use crate::types::{OutputStruct, PatchUpdateHandler, Signal};

const SR: f32 = 48_000.0;

fn volts(v: f32) -> Option<PolySignal> {
    Some(PolySignal::mono(Signal::Volts(v)))
}

fn params(hz: f32) -> SwellParams {
    SwellParams {
        freq: PolySignal::mono(Signal::Volts(hz_to_voct(hz))),
        steepness: None,
        chop: None,
        spread: None,
        waves: default_waves(),
        seed: 0,
    }
}

/// Build a swell the way the engine does: construct, then `on_patch_update`.
fn make(params: SwellParams) -> Swell {
    let channels = __swell_derive_channel_count(&params);
    let mut outputs = SwellOutputs::default();
    outputs.set_all_channels(channels);
    let mut s = Swell {
        params,
        outputs,
        channel_state: vec![ChannelState::default(); channels].into_boxed_slice(),
        _channel_count: channels,
        _block_index: Default::default(),
    };
    s.on_patch_update();
    s
}

fn render(params: SwellParams, samples: usize) -> Vec<f32> {
    let mut s = make(params);
    (0..samples)
        .map(|_| {
            s.update(SR);
            s.outputs.height.get(0)
        })
        .collect()
}

/// Horizontal offset of the surface point above rest position `p`, minus `p`'s
/// own position: zero exactly when that particle sits over the gauge.
fn gauge_residual(state: &ChannelState, count: usize, q: f32) -> f32 {
    let p = state.origin;
    let mut x = p;
    for wave in &state.waves[..count] {
        let theta = wave.k * (wave.dir[0] * p[0] + wave.dir[1] * p[1]) - TAU * wave.phase;
        x[0] -= q * wave.orbit * wave.dir[0] * theta.sin();
        x[1] -= q * wave.orbit * wave.dir[1] * theta.sin();
    }
    x[0].hypot(x[1])
}

#[test]
fn flat_single_wave_is_a_cosine() {
    let mut p = params(10.0);
    p.waves = 1;
    p.steepness = volts(0.0);
    let out = render(p, 4800);
    for (n, &v) in out.iter().enumerate() {
        let expected = 5.0 * (TAU * 10.0 * (n + 1) as f32 / SR).cos();
        assert!((v - expected).abs() < 1e-3, "sample {n}: {v} vs {expected}");
    }
}

#[test]
fn steepness_narrows_crests_and_widens_troughs() {
    let above_zero = |steepness: f32| {
        let mut p = params(10.0);
        p.waves = 1;
        p.steepness = volts(steepness);
        let out = render(p, 4800);
        let peak = out.iter().copied().fold(f32::MIN, f32::max);
        let trough = out.iter().copied().fold(f32::MAX, f32::min);
        assert!(
            peak > 4.99 && trough < -4.99,
            "peak {peak}, trough {trough}"
        );
        out.iter().filter(|&&v| v > 0.0).count() as f32 / out.len() as f32
    };
    let rolling = above_zero(0.0);
    let steep = above_zero(5.0);
    assert!(
        (rolling - 0.5).abs() < 0.01,
        "sine spends half its time above 0: {rolling}"
    );
    assert!(steep < 0.4, "steep crests should be narrow: {steep}");
}

#[test]
fn gauge_solve_converges() {
    let mut p = params(40.0);
    p.waves = MAX_WAVES;
    p.steepness = volts(5.0);
    p.chop = volts(5.0);
    p.spread = volts(5.0);
    let mut s = make(p);
    for n in 0..48_000 {
        s.update(SR);
        let residual = gauge_residual(&s.channel_state[0], MAX_WAVES, MAX_STEEPNESS);
        assert!(residual < 1e-4, "sample {n}: residual {residual}");
    }
}

#[test]
fn outputs_stay_bounded_at_extremes() {
    for hz in [0.01, 1.0, 200.0] {
        let mut p = params(hz);
        p.waves = MAX_WAVES;
        p.steepness = volts(5.0);
        p.chop = volts(5.0);
        p.spread = volts(5.0);
        let mut s = make(p);
        for _ in 0..9600 {
            s.update(SR);
            for v in [
                s.outputs.height.get(0),
                s.outputs.tilt.get(0),
                s.outputs.roll.get(0),
            ] {
                assert!(v.is_finite() && v.abs() <= 5.0, "{hz} Hz: {v}");
            }
        }
    }
}

#[test]
fn zero_chop_is_periodic() {
    let mut p = params(10.0);
    p.chop = volts(0.0);
    let out = render(p, 9600);
    let period = (SR / 10.0) as usize;
    for n in 0..period {
        let (a, b) = (out[n], out[n + period]);
        assert!((a - b).abs() < 1e-2, "sample {n}: {a} vs {b}");
    }
}

#[test]
fn chop_rolls_in_sets() {
    // Waves at different frequencies drift in and out of line, so successive
    // crests differ in height.
    let mut p = params(10.0);
    p.chop = volts(3.0);
    let out = render(p, 48_000);
    let crests: Vec<f32> = out
        .windows(3)
        .filter(|w| w[1] > w[0] && w[1] >= w[2])
        .map(|w| w[1])
        .collect();
    let max = crests.iter().copied().fold(f32::MIN, f32::max);
    let min = crests.iter().copied().fold(f32::MAX, f32::min);
    assert!(max - min > 0.5, "crest heights should vary: {min}..{max}");
}

#[test]
fn spread_drives_roll() {
    let peak_roll = |spread: f32| {
        let mut p = params(10.0);
        p.spread = volts(spread);
        let mut s = make(p);
        (0..9600)
            .map(|_| {
                s.update(SR);
                s.outputs.roll.get(0).abs()
            })
            .fold(0.0, f32::max)
    };
    assert!(peak_roll(0.0) < 1e-4, "long-crested sea does not roll");
    assert!(peak_roll(5.0) > 0.1, "confused sea rolls");
}

#[test]
fn channels_and_seeds_are_distinct_seas() {
    let mut p = params(10.0);
    p.freq = PolySignal::poly(&[
        Signal::Volts(hz_to_voct(10.0)),
        Signal::Volts(hz_to_voct(10.0)),
    ]);
    let mut s = make(p);
    let mut diff = 0.0f32;
    for _ in 0..9600 {
        s.update(SR);
        diff = diff.max((s.outputs.height.get(0) - s.outputs.height.get(1)).abs());
    }
    assert!(diff > 0.1, "channels should differ: {diff}");

    let mut seeded = params(10.0);
    seeded.seed = 7;
    let a = render(params(10.0), 9600);
    let b = render(seeded, 9600);
    let diff = a
        .iter()
        .zip(&b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max);
    assert!(diff > 0.1, "seeds should differ: {diff}");
}

#[test]
fn param_change_keeps_the_sea_moving() {
    // Changing `chop` mid-run reshapes the waves but keeps their phases, so
    // the output moves on without a jump.
    let mut s = make(params(1.0));
    for _ in 0..4800 {
        s.update(SR);
    }
    let before = s.outputs.height.get(0);
    s.params.chop = volts(1.01);
    s.update(SR);
    let after = s.outputs.height.get(0);
    assert!((after - before).abs() < 0.05, "{before} → {after}");
}
