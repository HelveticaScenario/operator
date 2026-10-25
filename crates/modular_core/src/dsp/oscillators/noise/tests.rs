use super::generators::lfsr_phase_distance;
use super::*;
use crate::dsp::utils::hz_to_voct;
use crate::types::{OutputStruct, PatchUpdateHandler, Signal};

const SR: f32 = 48000.0;

fn params(color: NoiseKind, channels: usize) -> NoiseParams {
    NoiseParams {
        color,
        rate: None,
        interp: NoiseInterp::Step,
        channels,
    }
}

fn with_rate(color: NoiseKind, hz: f32, interp: NoiseInterp) -> NoiseParams {
    NoiseParams {
        rate: Some(PolySignal::mono(Signal::Volts(hz_to_voct(hz)))),
        interp,
        ..params(color, 1)
    }
}

/// Build a Noise module with the given params, mirroring the production
/// lifecycle: the macro allocates `channel_state` to the channel count, `init`
/// caches the sample rate, then `on_patch_update` seeds each channel's RNG.
fn make_noise(params: NoiseParams) -> Noise {
    let mut n = build_noise(params);
    n.on_patch_update();
    n
}

/// [`make_noise`], seeded from a fixed `base` in place of the module address.
fn make_noise_seeded(params: NoiseParams, base: u64) -> Noise {
    let mut n = build_noise(params);
    n.prepare_channels(base);
    n
}

/// A constructed and initialized module whose channels are not seeded yet.
fn build_noise(params: NoiseParams) -> Noise {
    let channels = __noise_derive_channel_count(&params);
    let mut outputs = NoiseOutputs::default();
    outputs.set_all_channels(channels);
    let mut n = Noise {
        params,
        outputs,
        state: NoiseState::default(),
        channel_state: (0..channels)
            .map(|_| NoiseChannelState::default())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        _channel_count: channels,
        _block_index: Default::default(),
    };
    n.init(SR);
    n
}

fn render(params: NoiseParams, samples: usize) -> Vec<f32> {
    render_from(make_noise(params), samples)
}

/// Render channel 0 from a fixed seed, for statistical checks whose bounds a
/// small fraction of seeds would miss.
fn render_seeded(params: NoiseParams, samples: usize) -> Vec<f32> {
    render_from(make_noise_seeded(params, 0x5eed), samples)
}

fn render_from(mut n: Noise, samples: usize) -> Vec<f32> {
    (0..samples)
        .map(|_| {
            n.update(SR);
            n.outputs.sample.get(0)
        })
        .collect()
}

const ALL_KINDS: [NoiseKind; 12] = [
    NoiseKind::White,
    NoiseKind::Pink,
    NoiseKind::Brown,
    NoiseKind::Blue,
    NoiseKind::Violet,
    NoiseKind::Binary,
    NoiseKind::Gaussian,
    NoiseKind::Lfsr,
    NoiseKind::LfsrShort,
    NoiseKind::Velvet,
    NoiseKind::Telegraph,
    NoiseKind::Dust,
];

#[test]
fn channels_param_sets_channel_count() {
    assert_eq!(
        __noise_derive_channel_count(&params(NoiseKind::White, 4)),
        4
    );
}

#[test]
fn channels_clamped() {
    assert_eq!(
        __noise_derive_channel_count(&params(NoiseKind::White, 0)),
        1
    );
    assert_eq!(
        __noise_derive_channel_count(&params(NoiseKind::White, 1000)),
        crate::poly::PORT_MAX_CHANNELS
    );
}

#[test]
fn channels_produce_different_sequences() {
    for kind in ALL_KINDS {
        let mut n = make_noise(params(kind, 4));
        let mut series = [[0.0f32; 64]; 4];
        for i in 0..64 {
            n.update(SR);
            for (ch, s) in series.iter_mut().enumerate() {
                s[i] = n.outputs.sample.get(ch);
            }
        }
        for a in 0..4 {
            for b in (a + 1)..4 {
                assert_ne!(series[a], series[b], "{kind:?}: channels {a} and {b} match");
            }
        }
    }
}

#[test]
fn every_kind_stays_in_range_and_moves() {
    for kind in ALL_KINDS {
        for p in [params(kind, 1), with_rate(kind, 200.0, NoiseInterp::Smooth)] {
            let out = render(p, 20_000);
            assert!(out.iter().all(|v| v.abs() <= 5.0), "{kind:?} out of range");
            let distinct = out.iter().any(|&v| v != out[0]);
            assert!(distinct, "{kind:?} is constant");
        }
    }
}

#[test]
fn switching_kinds_sit_on_the_rails() {
    for kind in [
        NoiseKind::Binary,
        NoiseKind::Lfsr,
        NoiseKind::LfsrShort,
        NoiseKind::Telegraph,
        NoiseKind::Dust,
    ] {
        let out = render(params(kind, 1), 5000);
        assert!(
            out.iter().all(|v| v.abs() == 5.0),
            "{kind:?} left the rails"
        );
    }
}

#[test]
fn step_holds_between_ticks() {
    // 480 Hz at 48 kHz: a new value every 100 samples.
    let out = render(with_rate(NoiseKind::White, 480.0, NoiseInterp::Step), 2000);
    let changes = out.windows(2).filter(|w| w[0] != w[1]).count();
    assert!((18..=20).contains(&changes), "{changes} changes");
}

#[test]
fn linear_and_smooth_have_no_jumps() {
    for interp in [NoiseInterp::Linear, NoiseInterp::Smooth] {
        let out = render(with_rate(NoiseKind::White, 48.0, interp), 10_000);
        let max_jump = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(max_jump < 0.1, "{interp:?} jumped {max_jump}V");
    }
}

#[test]
fn velvet_fires_once_per_tick() {
    let out = render(
        with_rate(NoiseKind::Velvet, 480.0, NoiseInterp::Step),
        48_000,
    );
    let impulses = out.iter().filter(|v| v.abs() == 5.0).count();
    assert!((480..=481).contains(&impulses), "{impulses} impulses");
    assert!(out.iter().all(|&v| v == 0.0 || v.abs() == 5.0));
}

#[test]
fn telegraph_and_dust_follow_rate_on_average() {
    let out = render_seeded(
        with_rate(NoiseKind::Telegraph, 100.0, NoiseInterp::Step),
        480_000,
    );
    let flips = out.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(
        (900..1100).contains(&flips),
        "{flips} flips in 10s at 100/s"
    );

    let out = render_seeded(
        with_rate(NoiseKind::Dust, 100.0, NoiseInterp::Step),
        480_000,
    );
    let triggers = out.iter().filter(|&&v| v == 5.0).count();
    assert!(
        (900..1100).contains(&triggers),
        "{triggers} triggers in 10s at 100/s"
    );
    assert!(out.iter().all(|&v| v.abs() == 5.0));
}

/// Beads' gap bounds, `g(0.1)` and `g(1)` for `g(x) = x(1 + x(1/2 + x(1/3 + x/4)))`,
/// over its mean gap.
const DUST_GAP_RANGE: (f32, f32) = (0.105_358_3 / 0.883_138_3, 2.083_333_3 / 0.883_138_3);

#[test]
fn dust_gaps_stay_within_beads_bounds_and_average_the_rate() {
    // 100 per second at 48 kHz: 480 samples between triggers on average.
    let average = 480.0;
    let out = render_seeded(
        with_rate(NoiseKind::Dust, 100.0, NoiseInterp::Step),
        4_800_000,
    );
    let triggers: Vec<usize> = (0..out.len()).filter(|&i| out[i] == 5.0).collect();
    let gaps: Vec<f32> = triggers.windows(2).map(|w| (w[1] - w[0]) as f32).collect();
    let (lo, hi) = DUST_GAP_RANGE;
    for &gap in &gaps {
        assert!(
            gap >= (lo * average).floor() && gap <= (hi * average).ceil(),
            "gap of {gap} samples outside [{lo}, {hi}] × {average}"
        );
    }
    let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
    assert!(
        (mean / average - 1.0).abs() < 0.01,
        "mean gap {mean} samples, expected {average}"
    );
    // Short gaps are more likely than long ones.
    let short = gaps.iter().filter(|&&g| g < average).count();
    assert!(
        short * 2 > gaps.len(),
        "{short} of {} gaps short",
        gaps.len()
    );
}

#[test]
fn dust_without_rate_fires_every_two_samples_on_average() {
    let out = render_seeded(params(NoiseKind::Dust, 1), 100_000);
    let triggers = out.iter().filter(|&&v| v == 5.0).count();
    // Gaps under a sample merge, so slightly fewer than half the samples fire.
    assert!(
        (40_000..=50_000).contains(&triggers),
        "{triggers} triggers in 100000 samples"
    );
}

#[test]
fn slow_brown_wanders_without_jumps() {
    let out = render(
        with_rate(NoiseKind::Brown, 4.0, NoiseInterp::Smooth),
        48_000,
    );
    let max_jump = out
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max);
    assert!(max_jump < 0.01, "jumped {max_jump}V");
}

#[test]
fn polyphonic_rate_sets_channels_and_clocks_each() {
    let hz = [480.0, 960.0, 1920.0];
    let p = NoiseParams {
        rate: Some(PolySignal::poly(&hz.map(|h| Signal::Volts(hz_to_voct(h))))),
        ..params(NoiseKind::White, 1)
    };
    assert_eq!(__noise_derive_channel_count(&p), 3);
    assert_eq!(
        __noise_derive_channel_count(&NoiseParams {
            channels: 5,
            ..p.clone()
        }),
        5
    );

    let mut n = make_noise(p);
    let mut changes = [0usize; 3];
    let mut prev = [0.0f32; 3];
    for _ in 0..4800 {
        n.update(SR);
        for (ch, c) in changes.iter_mut().enumerate() {
            let v = n.outputs.sample.get(ch);
            *c += usize::from(v != prev[ch]);
            prev[ch] = v;
        }
    }
    // 0.1 s at 480, 960 and 1920 ticks per second.
    for (ch, (&c, expected)) in changes.iter().zip([48, 96, 192]).enumerate() {
        assert!(c.abs_diff(expected) <= 1, "channel {ch}: {c} ticks");
    }
}

#[test]
fn velvet_without_rate_fires_every_sample() {
    let out = render(params(NoiseKind::Velvet, 1), 1000);
    assert!(out.iter().all(|v| v.abs() == 5.0));
}

/// Largest delay, in samples, at which [`assert_no_shifted_copies`] compares
/// channels. One short-mode period, so it covers every rotation of a loop.
const MAX_LAG: usize = 93;

/// Assert that no LFSR channel is another channel delayed by up to
/// [`MAX_LAG`] samples. Any 15 consecutive output bits are the whole register,
/// so two channels sharing one 64-sample window are shifted copies.
fn assert_no_shifted_copies(n: &mut Noise) {
    const WINDOW: usize = 64;
    let channels = n.channel_count();
    let mut series = vec![Vec::new(); channels];
    for _ in 0..WINDOW + 2 * MAX_LAG {
        n.update(SR);
        for (ch, bits) in series.iter_mut().enumerate() {
            bits.push(n.outputs.sample.get(ch) > 0.0);
        }
    }
    let mut owner = std::collections::HashMap::new();
    for (ch, bits) in series.iter().enumerate() {
        for lag in 0..=2 * MAX_LAG {
            let window = bits[lag..lag + WINDOW]
                .iter()
                .fold(0u64, |w, &bit| (w << 1) | u64::from(bit));
            if let Some(other) = owner.insert(window, ch) {
                assert_eq!(
                    other, ch,
                    "{:?}: channels {other} and {ch} are shifted copies",
                    n.params.color
                );
            }
        }
    }
}

/// The smallest long-mode phase distance between any two channels.
fn min_phase_distance(n: &Noise) -> u32 {
    let phases: Vec<u16> = n.channel_state.iter().map(|s| s.lfsr_phase()).collect();
    let mut min = u32::MAX;
    for (a, &pa) in phases.iter().enumerate() {
        for &pb in &phases[a + 1..] {
            min = min.min(lfsr_phase_distance(pa, pb));
        }
    }
    min
}

/// Unseed channels `from..` so the next update adds them beside the rest.
fn drop_channels_from(n: &mut Noise, from: usize) {
    for state in n.channel_state[from..].iter_mut() {
        *state = NoiseChannelState::default();
    }
}

#[test]
fn channels_added_by_a_patch_update_get_their_own_sequence() {
    for kind in [NoiseKind::Lfsr, NoiseKind::LfsrShort] {
        let mut n = make_noise(params(kind, 4));
        drop_channels_from(&mut n, 1);
        n.on_patch_update();
        assert_no_shifted_copies(&mut n);
    }
}

/// Channels added after the carried-over ones have run for a while start
/// away from the current phases of the carried-over channels.
#[test]
fn channels_added_after_running_stay_apart() {
    for elapsed in [1, 511, 2047, 8191, 16383, 16384] {
        let mut n = make_noise(params(NoiseKind::Lfsr, 4));
        drop_channels_from(&mut n, 1);
        for _ in 0..elapsed {
            n.update(SR);
        }
        n.on_patch_update();
        // Claims beside one running channel split gaps of at least a half and
        // then a quarter period; seeding draws a few ticks of history after.
        let min = min_phase_distance(&n);
        assert!(min >= LFSR_LONG_PERIOD / 4 - 8, "{elapsed}: {min} ticks");
        assert_no_shifted_copies(&mut n);
    }
}

#[test]
fn fresh_lfsr_channels_are_evenly_spaced() {
    for channels in [2, 3, 4, 16, PORT_MAX_CHANNELS] {
        let n = make_noise(params(NoiseKind::Lfsr, channels));
        let spacing = LFSR_LONG_PERIOD / channels as u32;
        assert!(min_phase_distance(&n) >= spacing, "{channels} channels");
    }
}

#[test]
fn lfsr_channels_are_never_shifted_copies() {
    for kind in [NoiseKind::Lfsr, NoiseKind::LfsrShort] {
        for base in (0..LFSR_LONG_PERIOD).step_by(1021) {
            let mut n = make_noise_seeded(params(kind, PORT_MAX_CHANNELS), u64::from(base));
            assert_no_shifted_copies(&mut n);
        }
    }
}

#[test]
fn slow_rates_start_as_noise() {
    for kind in [NoiseKind::White, NoiseKind::Pink, NoiseKind::Brown] {
        for interp in [NoiseInterp::Step, NoiseInterp::Linear, NoiseInterp::Smooth] {
            let first = render(with_rate(kind, 0.2, interp), 1)[0];
            assert_ne!(first, 0.0, "{kind:?}/{interp:?} started at 0V");
        }
    }
    // A settled pink filter is at full level from its first slow tick.
    let mut rms = 0.0;
    for seed_offset in 0..64 {
        let p = NoiseParams {
            channels: 64,
            ..with_rate(NoiseKind::Pink, 0.2, NoiseInterp::Step)
        };
        let mut n = make_noise(p);
        n.update(SR);
        let v = n.outputs.sample.get(seed_offset);
        rms += v * v / 64.0;
    }
    assert!(rms.sqrt() > 0.5, "pink starts quiet: rms {}V", rms.sqrt());
}

#[test]
fn lfsr_short_rate_sets_loop_pitch() {
    // A 480 Hz loop at 48 kHz repeats every 100 samples.
    let out = render(
        with_rate(NoiseKind::LfsrShort, 480.0, NoiseInterp::Step),
        10_000,
    );
    let matches = (0..9_900).filter(|&i| out[i] == out[i + 100]).count();
    assert!(
        matches > 9_700,
        "{matches}/9900 samples repeat at the loop period"
    );
}

#[test]
fn brown_bounces_off_the_rails() {
    let out = render(params(NoiseKind::Brown, 1), 480_000);
    let pinned = out.iter().filter(|v| v.abs() == 5.0).count();
    assert!(pinned < 10, "{pinned} samples pinned at the rails");
}
