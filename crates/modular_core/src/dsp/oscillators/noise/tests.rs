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
    n.on_patch_update();
    n
}

fn render(params: NoiseParams, samples: usize) -> Vec<f32> {
    let mut n = make_noise(params);
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
    let out = render(
        with_rate(NoiseKind::Telegraph, 100.0, NoiseInterp::Step),
        480_000,
    );
    let flips = out.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(
        (900..1100).contains(&flips),
        "{flips} flips in 10s at 100/s"
    );

    let out = render(
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

#[test]
fn channels_added_by_a_patch_update_get_their_own_sequence() {
    let mut n = make_noise(params(NoiseKind::Lfsr, 4));
    for state in n.channel_state[1..].iter_mut() {
        *state = NoiseChannelState::default();
    }
    n.on_patch_update();
    let mut series = [[0.0f32; 64]; 4];
    for i in 0..64 {
        n.update(SR);
        for (ch, s) in series.iter_mut().enumerate() {
            s[i] = n.outputs.sample.get(ch);
        }
    }
    for a in 0..4 {
        for b in (a + 1)..4 {
            assert_ne!(series[a], series[b], "channels {a} and {b} match");
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
