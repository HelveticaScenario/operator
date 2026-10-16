//! TR-909 rim shot, simulated from the 909 service notes' voicing board
//! schematic. Part names (Q65, C119, …) are the service notes' designators.
//!
//! The trigger pulse, through 1 MΩ, rings three resonators: a bridged-T in
//! the feedback of each of IC48a, IC49a and IC49b, near 500, 220 and
//! 1050 Hz. Their outputs mix onto a node that diodes D91 and D92
//! clip, and transistor Q65, its base fed from the accent envelope on C119,
//! passes the mix to IC50a. A Sallen-Key high-pass with gain on IC50b
//! thins it to a click.

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{DIODE_DROP, Shaper, TRIG_V, Timing, VCA, VSAT, accent_volts};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState};
use crate::dsp::drums::voice::{Circuit, Pulse, Quiet, Voices};
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

/// Scales the output so an unaccented hit peaks near 5 V.
const OUTPUT_GAIN: f32 = 1.0;
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
struct Rim {
    trigger: Pulse,
    accent: f32,
    env: f32,
    res: MnaState<RES_CAPS>,
    out: MnaState<OUT_CAPS>,
    mix: f64,
    vc: f64,
    vbe: f64,
    quiet: Quiet,
}

#[derive(Default)]
struct RimShared {
    timing: Timing,
    dt: f32,
    res: Mna<RES_UNKNOWNS, RES_CAPS, 0, 1>,
    out: Mna<OUT_UNKNOWNS, OUT_CAPS, 0, 1>,
}

impl Circuit for Rim {
    type Shared = RimShared;
    type Controls = ();

    fn fire(&mut self, accent: f32, shared: &RimShared) {
        self.accent = (ACCENT_GAIN * accent_volts(ACCENT_LOW, accent)).min(VSAT);
        self.trigger.fire(TRIG_V, shared.timing.trig_samples);
    }

    fn settled(&self) -> bool {
        !self.trigger.active() && self.env < ENV_SETTLED && self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &RimShared, _: &()) -> f32 {
        let dt = sh.dt;
        let trig = self.trigger.next();

        // C119 charges from the gated accent, then feeds Q65's base.
        if trig > 0.0 {
            let top = self.accent - DIODE_DROP;
            self.env = top + (self.env - top) * (-dt / (R401 * C119)).exp();
        }
        let ib = VCA.base_current(self.env, R403, &mut self.vbe);
        self.env -= ib as f32 * dt / C119;

        let mut injected = [0.0f32; RES_UNKNOWNS];
        injected[DRIVE] = trig / R_DRIVE;
        let x = sh.res.open(&self.res, 0, &injected);
        let z = sh.res.impedance(0, MIX, MIX) as f64;
        // The mix node's diodes and Q65's collector, fed from the
        // network's Thevenin equivalent at the mix node.
        let open = x[MIX] as f64;
        let shaper = Shaper {
            r_in: z,
            r_out: R423,
            r_load: R418,
        };
        let ie = shaper.solve(&mut self.mix, &mut self.vc, open, ib);
        let drawn = (open - self.mix) / z;
        sh.res.close(&mut self.res, 0, x, &[(MIX, -drawn as f32)]);

        let level = (-(R_LEVEL * ie) as f32).clamp(-VSAT, VSAT);
        let mut injected = [0.0f32; OUT_UNKNOWNS];
        injected[OUT_IN] = level * STIFF;
        let x = sh.out.step(&mut self.out, 0, &injected);
        let out = x[OUT].clamp(-VSAT, VSAT);
        self.quiet.track(out);
        out
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
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
    choke: bool,
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
/// $tr909.rs($clock.beatTrigger).out()
/// ```
#[module(name = "$tr909.rs", has_init, args(trigger))]
pub struct Rs909 {
    outputs: Rs909Outputs,
    params: Rs909Params,
    state: RimShared,
    channel_state: Box<[Voices<Rim>]>,
}

impl Rs909 {
    fn init(&mut self, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        let sh = &mut self.state;
        sh.timing = Timing::new(sample_rate);
        sh.dt = dt;
        sh.res = resonator_network(dt);
        sh.out = output_network(dt);
    }

    fn update(&mut self, _sample_rate: f32) {
        let sh = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let out = self.channel_state[ch].process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                sh.timing.choke_samples,
                sh,
                &(),
            );
            self.outputs.sample.set(ch, out * OUTPUT_GAIN);
        }
    }
}

message_handlers!(impl Rs909 {});
