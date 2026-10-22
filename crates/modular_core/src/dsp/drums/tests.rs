//! Every channel of a drum sounds the same whether it runs alone or in a
//! group, whether it runs by block or sample by sample, and keeps sounding
//! across patch updates that regroup it.

use serde_json::{Value, json};

use super::group::ALONE;
use crate::Patch;
use crate::dsp::get_params_deserializers;
use crate::dsp::utils::rng::TEST_SEED_BASE;
use crate::params::DeserializedParams;
use crate::types::ProcessingMode;

const SR: f32 = 48_000.0;
const BLOCK: usize = 64;

const VOICES: [&str; 22] = [
    "$tr808.kick",
    "$tr808.snare",
    "$tr808.lowTom",
    "$tr808.midTom",
    "$tr808.highTom",
    "$tr808.lowConga",
    "$tr808.midConga",
    "$tr808.highConga",
    "$tr808.rimShot",
    "$tr808.claves",
    "$tr808.cowbell",
    "$tr808.clap",
    "$tr808.maracas",
    "$tr808.cymbal",
    "$tr808.hiHat",
    "$tr909.kick",
    "$tr909.snare",
    "$tr909.lowTom",
    "$tr909.midTom",
    "$tr909.highTom",
    "$tr909.rimShot",
    "$tr909.clap",
];

/// How a drum's trigger and accent are patched, reaching each way a block
/// input resolves.
#[derive(Clone, Copy, Debug)]
enum Wiring {
    /// A clock cable per channel, and a constant accent per channel.
    Poly,
    /// One clock cable across every channel.
    Broadcast,
    /// An accent mixing cables and constants.
    Mixed,
}

/// A drum of `channels` channels struck by a clock per channel, fast enough
/// that hits run into each other, with the accent varying across channels;
/// the hi-hat's open hat is struck by a second clock.
fn patch(voice: &str, channels: usize, restart: bool) -> Patch {
    wired(
        voice,
        channels,
        restart,
        Wiring::Poly,
        ProcessingMode::Block,
    )
}

/// [`patch`], wired by `wiring`, with the drum running in `mode`.
fn wired(
    voice: &str,
    channels: usize,
    restart: bool,
    wiring: Wiring,
    mode: ProcessingMode,
) -> Patch {
    let per_channel = |f: &dyn Fn(usize) -> Value| Value::Array((0..channels).map(f).collect());
    let channel = |module: &str, ch: usize| json!({ "type": "cable", "module": module, "port": "output", "channel": ch });
    let cable = |module: &str| per_channel(&|ch| channel(module, ch));
    let accent = |ch: usize| json!([0.0, 5.0, 2.0, 3.7, 1.0, 4.4][ch % 6]);
    let trigger = if voice == "$tr808.hiHat" {
        "closed"
    } else {
        "trigger"
    };
    let mut params = json!({
        trigger: cable("clock"),
        "accent": per_channel(&accent),
        "restart": restart,
    });
    match wiring {
        Wiring::Poly => {}
        Wiring::Broadcast => params[trigger] = channel("clock", 0),
        Wiring::Mixed => {
            params["accent"] = per_channel(&|ch| {
                if ch % 2 == 0 {
                    channel("close", ch)
                } else {
                    accent(ch)
                }
            })
        }
    }
    if voice == "$tr808.hiHat" {
        params["open"] = cable("close");
    }
    let modules = [
        (
            "clock",
            "$pulse",
            json!({ "freq": per_channel(&|ch| json!(-1.3 + 0.17 * ch as f32)) }),
            ProcessingMode::Block,
        ),
        (
            "close",
            "$pulse",
            json!({ "freq": per_channel(&|ch| json!(-2.1 + 0.23 * ch as f32)) }),
            ProcessingMode::Block,
        ),
        ("drum", voice, params, mode),
    ];
    let deserializers = get_params_deserializers();
    let mut patch = Patch::new();
    patch
        .insert_modules(
            modules.into_iter().map(|(id, module_type, params, mode)| {
                let cached = deserializers[module_type](params).expect("deserialize");
                (
                    id.to_string(),
                    module_type.to_string(),
                    DeserializedParams {
                        params: cached.params,
                        channel_count: cached.channel_count,
                    },
                    mode,
                )
            }),
            SR,
            BLOCK,
        )
        .expect("insert");
    patch
}

/// Connects `patch` and runs its patch-update hooks, as a patch update does.
fn start(patch: &Patch) {
    patch.connect_all();
    for module in patch.sampleables.values() {
        module.on_patch_update();
    }
}

/// `blocks` blocks of every channel's output, appended to `out`.
fn render(patch: &Patch, blocks: usize, out: &mut [Vec<f32>]) {
    render_by(patch, blocks, &[BLOCK], out);
}

/// [`render`], each block run up to each of `steps` in turn, the drum
/// first so its reads pull its sources.
fn render_by(patch: &Patch, blocks: usize, steps: &[usize], out: &mut [Vec<f32>]) {
    for _ in 0..blocks {
        for module in patch.sampleables.values() {
            module.start_block();
        }
        for &step in steps {
            for id in ["drum", "clock", "close"] {
                patch.sampleables[id].ensure_processed_to(step);
            }
        }
        for (ch, samples) in out.iter_mut().enumerate() {
            samples.extend(
                (0..BLOCK).map(|i| patch.sampleables["drum"].get_value_at("output", ch, i)),
            );
        }
    }
}

/// `blocks` blocks of every channel of a fresh drum.
fn fresh(voice: &str, channels: usize, restart: bool, blocks: usize) -> Vec<Vec<f32>> {
    let patch = patch(voice, channels, restart);
    start(&patch);
    let mut out = vec![Vec::new(); channels];
    render(&patch, blocks, &mut out);
    out
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

#[test]
fn channels_sound_the_same_alone_and_in_groups() {
    TEST_SEED_BASE.set(Some(0x5eed));
    for voice in VOICES {
        for channels in [2, 3, 6] {
            for restart in [false, true] {
                let grouped = fresh(voice, channels, restart, 20);
                ALONE.set(true);
                let alone = fresh(voice, channels, restart, 20);
                ALONE.set(false);
                for (ch, (g, a)) in grouped.iter().zip(&alone).enumerate() {
                    assert!(
                        same_bits(g, a),
                        "{voice} channel {ch} of {channels}, restart {restart}"
                    );
                    assert!(g.iter().any(|v| *v != 0.0), "{voice} channel {ch} silent");
                }
            }
        }
    }
}

#[test]
fn block_and_sample_paths_agree() {
    TEST_SEED_BASE.set(Some(0x5eed));
    let run = |voice, channels, wiring, mode, steps: &[usize]| {
        let patch = wired(voice, channels, true, wiring, mode);
        start(&patch);
        let mut out = vec![Vec::new(); channels];
        render_by(&patch, 8, steps, &mut out);
        out
    };
    for voice in VOICES {
        for channels in [1, 3, 6] {
            for wiring in [Wiring::Poly, Wiring::Broadcast, Wiring::Mixed] {
                let by_sample = run(voice, channels, wiring, ProcessingMode::Sample, &[BLOCK]);
                for steps in [&[BLOCK][..], &[1, 10, 11, 40, BLOCK]] {
                    let by_block = run(voice, channels, wiring, ProcessingMode::Block, steps);
                    for (ch, (b, s)) in by_block.iter().zip(&by_sample).enumerate() {
                        assert!(
                            same_bits(b, s),
                            "{voice} channel {ch} of {channels}, {wiring:?}, steps {steps:?}"
                        );
                        assert!(b.iter().any(|v| *v != 0.0), "{voice} channel {ch} silent");
                    }
                }
            }
        }
    }
}

#[test]
fn channels_keep_sounding_as_updates_regroup_them() {
    TEST_SEED_BASE.set(Some(0x5eed));
    const COUNTS: [usize; 5] = [6, 4, 1, 2, 6];
    const BLOCKS: usize = 8;
    for voice in VOICES {
        let whole = fresh(voice, 6, false, BLOCKS * COUNTS.len());
        let mut old = patch(voice, COUNTS[0], false);
        start(&old);
        // A channel sounds on unbroken from the start until an update drops
        // it.
        let mut kept = COUNTS[0];
        for (segment, &count) in COUNTS.iter().enumerate() {
            if segment > 0 {
                let new = patch(voice, count, false);
                for (id, module) in &new.sampleables {
                    module.transfer_state_from(old.sampleables[id].as_ref());
                }
                start(&new);
                old = new;
            }
            kept = kept.min(count);
            let mut out = vec![Vec::new(); count];
            render(&old, BLOCKS, &mut out);
            let span = segment * BLOCKS * BLOCK..(segment + 1) * BLOCKS * BLOCK;
            for (ch, samples) in out.iter().enumerate().take(kept) {
                assert!(
                    same_bits(samples, &whole[ch][span.clone()]),
                    "{voice} channel {ch} through {COUNTS:?}, segment {segment}"
                );
            }
        }
    }
}

mod parking {
    //! A circuit that settles within a few samples, struck on a schedule,
    //! so its lanes settle, park and strike again while others ring.

    use super::super::group::{ALONE, Channels, Drum, Groups, Run};
    use super::super::lanes::{Lanes, Mono, Real, c, lanewise};
    use super::super::voice::{Circuit, Fire, Voices};

    #[derive(Clone, Copy, Default)]
    struct Ring<L: Lanes = Mono> {
        level: L::S,
        phase: L::S,
    }

    lanewise!(Ring { level, phase });

    impl Fire for Ring {
        fn fire(&mut self, accent: f32, _: &()) {
            self.level += 1.0 + accent;
        }
    }

    impl<L: Lanes> Circuit<L> for Ring<L> {
        type Shared = ();
        type Controls = L::S;

        fn settled(&self) -> L::SM {
            self.level.lt(c(0.05))
        }

        fn step(&mut self, _: &(), &rate: &L::S) -> L::S {
            self.phase = (self.phase + rate).fract();
            self.level *= 0.7;
            self.level * self.phase
        }
    }

    struct RingDrum<const QUAD: bool, const PAIR: bool>;

    impl<const Q: bool, const P: bool> Drum for RingDrum<Q, P> {
        type Shared = ();
        type Circuit<L: Lanes> = Ring<L>;
        type Controls<L: Lanes> = L::S;
        type Sources<L: Lanes> = ();
        const QUAD: bool = Q;
        const PAIR: bool = P;
    }

    /// Strikes channel `ch` on samples where `strikes(sample, ch)`.
    struct Script {
        sample: usize,
    }

    fn strikes(sample: usize, ch: usize) -> bool {
        (sample + 3 * ch) % (7 + 5 * ch) < 2
    }

    impl<const Q: bool, const P: bool> Channels<RingDrum<Q, P>> for Script {
        type Channel = Voices<Ring>;

        fn voices(channel: &mut Voices<Ring>) -> &mut Voices<Ring> {
            channel
        }

        fn inputs(&self, ch: usize, _: &mut Voices<Ring>) -> (f32, f32) {
            let trigger = if strikes(self.sample, ch) { 5.0 } else { 0.0 };
            (trigger, 0.11 + 0.07 * ch as f32)
        }
    }

    /// Every channel's output over `samples` samples, the channel count
    /// changing to each of `counts` in turn.
    fn run<const Q: bool, const P: bool>(
        restart: bool,
        counts: &[usize],
        samples: usize,
    ) -> Vec<Vec<f32>> {
        let accent: &[f32] = &[0.0; 6];
        let run = Run::<RingDrum<Q, P>, _> {
            accent,
            restart,
            fade_samples: 3,
            shared: &(),
        };
        let mut groups = Groups::<RingDrum<Q, P>>::new();
        let mut channels = vec![Voices::<Ring>::default(); 6];
        let mut out = vec![Vec::new(); 6];
        let mut last = 0;
        for (segment, &count) in counts.iter().enumerate() {
            // As a patch update does, channels past the last count start
            // afresh.
            channels[last..].fill(Voices::default());
            groups.layout::<Script>(&mut channels, count);
            last = count;
            for sample in segment * samples..(segment + 1) * samples {
                let mut values = [0.0; 6];
                groups.run(&Script { sample }, &mut channels[..count], &run, |ch, v| {
                    values[ch] = v;
                });
                for (ch, samples) in out.iter_mut().enumerate() {
                    samples.push(values[ch]);
                }
            }
        }
        out
    }

    fn check<const Q: bool, const P: bool>() {
        for restart in [false, true] {
            for counts in [&[6][..], &[4, 6, 3, 1, 5, 2, 6]] {
                let grouped = run::<Q, P>(restart, counts, 60);
                ALONE.set(true);
                let alone = run::<Q, P>(restart, counts, 60);
                ALONE.set(false);
                assert_eq!(
                    grouped, alone,
                    "quad {Q}, pair {P}, restart {restart}, {counts:?}"
                );
            }
        }
    }

    #[test]
    fn settled_lanes_park_and_strike_again_as_alone() {
        check::<true, true>();
        check::<true, false>();
        check::<false, true>();
    }
}
