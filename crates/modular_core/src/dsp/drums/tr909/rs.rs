//! TR-909 rim shot, simulated from the 909 service notes' voicing board
//! schematic. Part names (Q65, C119, …) are the service notes' designators.
//!
//! The trigger pulse, through 1 MΩ, rings three resonators: a bridged-T in
//! the feedback of each of IC48a, IC49a and IC49b, near 500, 220 and
//! 1050 Hz. Their outputs mix onto a node that diodes D91 and D92
//! clip, and transistor Q65, its base fed from the accent envelope on C119,
//! passes the mix to IC50a. A Sallen-Key high-pass with gain on IC50b
//! thins it to a click.

use std::ops::Range;

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{DIODE_DROP, Shaper, TRIG_V, Timing, VCA, VSAT, accent_volts};
use crate::dsp::drums::analog::Junction;
use crate::dsp::drums::group::{Channels, Drum, Groups, Run};
use crate::dsp::drums::input::{Input, Live, each_slot};
use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState};
use crate::dsp::drums::voice::{Circuit, Fire, Pulse, Quiet, Voices, rails};
use crate::poly::{PolyOutput, PolySignal};

// IC48b scales the accent voltage by 1 + R406/R397.
const ACCENT_GAIN: f32 = 1.0 + 10.0e3 / 4.7e3;
/// The accent voltage of an unaccented hit.
const ACCENT_LOW: f32 = 1.4;

// The trigger pulse through 1 MΩ onto 22 kΩ to ground, driving every
// resonator's non-inverting input.
const R_DRIVE: f32 = 1.0e6;
const R_DRIVE_GROUND: f32 = 22.0e3;

// The resonator network: the drive node, then for each resonator its
// inverting input, bridged-T junction and output, and the outputs' mixing
// resistors into the mix node; then the op-amps' output currents.
const DRIVE: usize = 0;
const MIX: usize = 10;
const RES_UNKNOWNS: usize = 14;
const RES_CAPS: usize = 6;

/// One resonator: its twin capacitors, the bridge across them, the shunt
/// from their junction to ground, and its mixing resistor.
struct Resonator {
    c: f32,
    bridge: f32,
    shunt: f32,
    mix: f32,
}

const RESONATORS: [Resonator; 3] = [
    // IC48a: C113, C112, R407, R394, R408.
    Resonator {
        c: 0.01e-6,
        bridge: 470.0e3,
        shunt: 2.2e3,
        mix: 12.0e3,
    },
    // IC49a: C116, C115, R414, R411.
    Resonator {
        c: 0.027e-6,
        bridge: 330.0e3,
        shunt: 2.2e3,
        mix: 12.0e3,
    },
    // IC49b: C118, C117, R416, R404, R417.
    Resonator {
        c: 0.0047e-6,
        bridge: 470.0e3,
        shunt: 2.2e3,
        mix: 3.3e3,
    },
];

// The mix node's clipping diodes, then R423 into Q65's collector with R418
// to ground; C119 feeds Q65's base through R403, charged from the gated
// accent through R401.
const R423: f64 = 12.0e3;
const R418: f64 = 680.0;
const C119: f32 = 0.047e-6;
const R401: f32 = 2.7e3;
const R403: f32 = 1.0e6;

// The output: IC50a with the level pot at maximum, then the Sallen-Key
// high-pass on IC50b (C121, C122, R422, R420) with a gain of two. R422
// returns to IC50b's inverting input, which follows its non-inverting one.
const R_LEVEL: f64 = 100.0e3;
const OUT_IN: usize = 0;
const H1: usize = 1;
const H2: usize = 2;
const M50: usize = 3;
const OUT: usize = 4;
const OP50: usize = 5;
const OUT_UNKNOWNS: usize = 6;
const OUT_CAPS: usize = 2;
const STIFF: f32 = 10.0;

/// Below this running peak, some 84 dB under an unaccented hit, the drum
/// has faded out.
const SETTLED: f32 = 3.0e-4;
/// The envelope below this has let its VCA all but close; it drains only
/// through the VCA's base, ever more slowly.
const ENV_SETTLED: f32 = 0.65;

fn resonator_network(dt: f32) -> Mna<RES_UNKNOWNS, RES_CAPS, 0, 1> {
    let mut b = MnaBuilder::<RES_UNKNOWNS>::default();
    b.resistor(
        DRIVE,
        GROUND,
        R_DRIVE * R_DRIVE_GROUND / (R_DRIVE + R_DRIVE_GROUND),
    );
    for (k, r) in RESONATORS.iter().enumerate() {
        let (minus, junction, out, opamp) = (1 + 3 * k, 2 + 3 * k, 3 + 3 * k, 11 + k);
        b.capacitor(minus, junction, r.c)
            .capacitor(junction, out, r.c)
            .resistor(junction, GROUND, r.shunt)
            .resistor(minus, out, r.bridge)
            .opamp(DRIVE, minus, out, opamp)
            .resistor(out, MIX, r.mix);
    }
    b.build(dt, [])
}

fn output_network(dt: f32) -> Mna<OUT_UNKNOWNS, OUT_CAPS, 0, 1> {
    let mut b = MnaBuilder::<OUT_UNKNOWNS>::default();
    b.resistor(OUT_IN, GROUND, 1.0 / STIFF)
        .capacitor(OUT_IN, H1, 0.01e-6)
        .capacitor(H1, H2, 0.01e-6)
        .resistor(H1, M50, 22.0e3)
        .resistor(H2, GROUND, 220.0e3)
        .resistor(M50, GROUND, 4.7e3)
        .resistor(M50, OUT, 4.7e3)
        .opamp(H2, M50, OUT, OP50);
    b.build(dt, [])
}

/// The rim shot's circuit state.
#[derive(Clone, Copy, Default)]
struct Rim<L: Lanes = Mono> {
    trigger: Pulse<L>,
    accent: L::S,
    env: L::S,
    res: MnaState<RES_CAPS, L>,
    out: MnaState<OUT_CAPS, L>,
    mix: Junction<L>,
    vc: Junction<L>,
    vbe: Junction<L>,
    quiet: Quiet<L>,
}

lanewise!(Rim {
    trigger,
    accent,
    env,
    res,
    out,
    mix,
    vc,
    vbe,
    quiet
});

#[derive(Default)]
struct RimShared {
    timing: Timing,
    dt: f32,
    res: Mna<RES_UNKNOWNS, RES_CAPS, 0, 1>,
    out: Mna<OUT_UNKNOWNS, OUT_CAPS, 0, 1>,
    /// C119's per-sample approach to the accent through R401.
    env_charge: f32,
}

/// The rim shot at every width.
struct RimDrum;

impl Drum for RimDrum {
    type Shared = RimShared;
    type Circuit<L: Lanes> = Rim<L>;
    type Controls<L: Lanes> = ();
    type Sources<L: Lanes> = ();
    const QUAD: bool = false;
    const PAIR: bool = false;
}

impl Fire for Rim {
    fn fire(&mut self, accent: f32, shared: &RimShared) {
        self.accent = (ACCENT_GAIN * accent_volts(ACCENT_LOW, accent)).min(VSAT);
        self.trigger.fire(TRIG_V, shared.timing.trig_samples);
    }
}

impl<L: Lanes> Circuit<L> for Rim<L> {
    type Shared = RimShared;
    type Controls = ();

    fn settled(&self) -> L::SM {
        !self.trigger.active() & self.env.lt(c(ENV_SETTLED)) & self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &RimShared, _: &()) -> L::S {
        let dt = sh.dt;
        let trig = self.trigger.next();

        // C119 charges from the gated accent, then feeds Q65's base.
        let on = trig.gt(c(0.0));
        if on.any() {
            let top = self.accent - DIODE_DROP;
            self.env = L::S::select(on, top + (self.env - top) * sh.env_charge, self.env);
        }
        let ib = VCA.base_current(self.env, R403, &mut self.vbe);
        self.env -= L::narrow(ib) * dt / C119;

        let mut injected = [c(0.0); RES_UNKNOWNS];
        injected[DRIVE] = trig / R_DRIVE;
        let open = L::closed([]);
        let x = sh.res.open(&self.res, open, &injected);
        let z = sh.res.impedance::<Mono>(0, MIX, MIX) as f64;
        // The mix node's diodes and Q65's collector, fed from the
        // network's Thevenin equivalent at the mix node.
        let v = L::wide(x[MIX]);
        let shaper = Shaper {
            r_in: z,
            r_out: R423,
            r_load: R418,
        };
        let ie = shaper.solve(!L::DM::default(), &mut self.mix, &mut self.vc, v, ib);
        let drawn = (v - self.mix.v) / z;
        sh.res
            .close(&mut self.res, open, x, &[(MIX, -L::narrow(drawn))]);

        let level = L::narrow(-(ie * R_LEVEL)).clamp(c(-VSAT), c(VSAT));
        let mut injected = [c(0.0); OUT_UNKNOWNS];
        injected[OUT_IN] = level * STIFF;
        let x = sh.out.step(&mut self.out, open, &injected);
        let out = x[OUT].clamp(c(-VSAT), c(VSAT));
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}

/// The inputs every channel of the rim shot reads.
struct RsInputs<I> {
    trigger: I,
}

impl<I: Input> Channels<RimDrum> for RsInputs<I> {
    type Channel = Voices<Rim>;

    fn voices(channel: &mut Voices<Rim>) -> &mut Voices<Rim> {
        channel
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, _: &mut Voices<Rim>) -> (f32, ()) {
        (self.trigger.at(ch), ())
    }
}

#[derive(Default)]
struct RsState {
    shared: RimShared,
    groups: Groups<RimDrum>,
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Rs909Params {
    /// rising edges strike the rim
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the accent voltage, for a hit up to twice as loud (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// fade out a still-sounding hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the ring, as on
    /// the hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    restart: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Rs909Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-909 rim shot.
///
/// Each rising edge on **trigger** fires the 909's trigger into a circuit
/// model of its rim shot: three resonators rung by the trigger pulse,
/// clipped by diodes, passed by an accent-driven VCA and high-passed to a
/// click. **accent**, sampled at each strike, drives it harder.
///
/// An unaccented hit peaks near **±5 V**.
///
/// ```js
/// $tr909.rimShot($clock.beatTrigger).out()
/// ```
#[module(name = "$tr909.rimShot", has_init, patch_update, args(trigger), block)]
pub struct Rs909 {
    outputs: Rs909Outputs,
    params: Rs909Params,
    state: RsState,
    channel_state: Box<[Voices<Rim>]>,
}

impl Rs909 {
    fn init(&mut self, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        self.state.groups = Groups::new();
        let sh = &mut self.state.shared;
        sh.timing = Timing::new(sample_rate);
        sh.dt = dt;
        sh.res = resonator_network(dt);
        sh.out = output_network(dt);
        sh.env_charge = (-dt / (R401 * C119)).exp();
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let p = &self.params;
        let inputs = [
            Live::new(&p.trigger),
            Live::or(&p.accent, Rs909Params::ACCENT_DEFAULT),
        ];
        let out = &mut self.outputs.sample;
        self.state.tick(
            &mut self.channel_state[..channels],
            p.restart,
            inputs,
            |ch, v| out.set(ch, v),
        );
    }

    fn process_block(
        &mut self,
        outputs: &mut Rs909BlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let Rs909ParamsBlockInputs { trigger, accent } =
            p.block_inputs(range.clone(), channels, &mut scratch)?;
        let channel_state = &mut self.channel_state[..channels];
        each_slot(
            &mut outputs.sample,
            range,
            [trigger, accent],
            |inputs, mut slot| {
                self.state
                    .tick(channel_state, p.restart, inputs, |ch, v| slot.set(ch, v))
            },
        );
        Some(())
    }
}

impl RsState {
    /// Runs one sample of every channel on its `[trigger, accent]`, handing
    /// `out` each one's output.
    #[inline(always)]
    fn tick<I: Input>(
        &mut self,
        channels: &mut [Voices<Rim>],
        restart: bool,
        [trigger, accent]: [I; 2],
        mut out: impl FnMut(usize, f32),
    ) {
        let run = Run {
            accent,
            restart,
            fade_samples: self.shared.timing.fade_samples,
            shared: &self.shared,
        };
        let inputs = RsInputs { trigger };
        self.groups
            .run(&inputs, channels, &run, |ch, v| out(ch, rails(v)));
    }
}

impl crate::types::PatchUpdateHandler for Rs909 {
    fn on_patch_update(&mut self) {
        let count = self.channel_count();
        self.state
            .groups
            .layout::<RsInputs<&[f32]>>(&mut self.channel_state, count);
    }
}

message_handlers!(impl Rs909 {});
