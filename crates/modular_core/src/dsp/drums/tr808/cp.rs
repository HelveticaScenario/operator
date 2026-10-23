//! TR-808 hand clap, simulated from the 808 service notes' voicing board
//! schematic. Part names (Q70, C144, …) are the service notes' designators.
//!
//! White noise through IC21's band-pass feeds two VCAs. The first, a BA662
//! OTA, is gated by the clap envelope in [`crate::dsp::drums::clap`]. The
//! second VCA,
//! Q70, is biased by a slow envelope charged by the trigger through Q69 and
//! adds the "reverb" that lingers after the claps.

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{PULSE_V, Timing, WhiteNoise, pulse_volts};
use crate::dsp::drums::analog::VT;
use crate::dsp::drums::clap::{Envelope, EnvelopeNetwork, ota};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState, Switch};
use crate::dsp::drums::voice::{Circuit, Level, Pulse, Quiet, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal};

const RAIL: f32 = 15.0;
const BETA: f32 = 300.0;
const IS: f64 = 10.0e-15;

// The noise network: the white noise driven in stiffly at `WN`, IC21's
// multiple-feedback band-pass, the OTA's input divider, the divider into
// C137 and Q70's base, and the envelopes Q69 charges: C139 and C138 for
// Q70's bias, C143 for Q71's base.
const WN: usize = 0;
const X21: usize = 1;
const INV21: usize = 2;
const OUT21: usize = 3;
const Y36: usize = 4;
const OTA_IN: usize = 5;
const V37: usize = 6;
const B70: usize = 7;
const A139: usize = 8;
const B138: usize = 9;
const C143: usize = 10;
const B71: usize = 11;
const OP21: usize = 12;
const NOISE_UNKNOWNS: usize = 13;
const NOISE_CAPS: usize = 8;
// Its switches: D69 and D68 charging from Q69's emitter.
const D69: usize = 0;
const D68: usize = 1;
const NOISE_SWITCHES: usize = 2;
const NOISE_COMBOS: usize = 1 << NOISE_SWITCHES;
/// Conductance driving the noise input to the generator's voltage.
const STIFF: f32 = 10.0;
/// Q69's emitter and a diode's resistance as they charge an envelope.
const R_CHARGE: f32 = 100.0;
/// Q69's base-emitter drop and the diodes' drop as it charges.
const Q69_VBE: f32 = 0.65;
const DIODE_DROP: f32 = 0.6;

/// The offset trimmer TM3 (10 kΩ between ground and −15 V) feeds Q71's
/// base through R370; it is set, as the service notes' adjustment asks, so
/// the OTA nearly closes between claps.
const TM3_WIPER: f32 = -0.58 * RAIL;
const R370: f32 = 47.0e3;

// R360 and R361 divide the trigger line against −15 V into IC23c.
const R360: f32 = 27.0e3;
const R361: f32 = 22.0e3;

// Q70's emitter: R369 to ground and R200 to −15 V.
const R369: f32 = 330.0;
const R200: f32 = 10.0e3;

// The output network: the OTA's buffer driven in stiffly, Q70's collector,
// and the mixer IC19 with its level pot at maximum.
const OTA_OUT: usize = 0;
const Q150: usize = 1;
const C70: usize = 2;
const R152: usize = 3;
const M19: usize = 4;
const OUT19: usize = 5;
const OP19: usize = 6;
const OUT_UNKNOWNS: usize = 7;
const OUT_CAPS: usize = 4;
/// IC19's feedback: 470 kΩ with the 100 kΩ level pot across it.
const R_FEEDBACK: f32 = 470.0e3 * 100.0e3 / 570.0e3;

/// Scales the output so an unaccented clap peaks near 5 V.
const OUTPUT_GAIN: f32 = 1.53;
/// Below this running peak the clap has faded out.
const SETTLED: f32 = 3.0e-4;

fn noise_network(dt: f32) -> Mna<NOISE_UNKNOWNS, NOISE_CAPS, NOISE_SWITCHES, NOISE_COMBOS> {
    let mut b = MnaBuilder::<NOISE_UNKNOWNS>::default();
    b.resistor(WN, GROUND, 1.0 / STIFF)
        // IC21: R332 with C127 across it, R333, C128, C129 and R334.
        .resistor(WN, X21, 27.0e3)
        .capacitor(WN, X21, 0.0018e-6)
        .resistor(X21, GROUND, 10.0e3)
        .capacitor(X21, INV21, 0.0047e-6)
        .capacitor(X21, OUT21, 0.0047e-6)
        .resistor(OUT21, INV21, 100.0e3)
        .opamp(GROUND, INV21, OUT21, OP21)
        // C136, R372 and R382 into the OTA.
        .capacitor(OUT21, Y36, 0.022e-6)
        .resistor(Y36, OTA_IN, 39.0e3)
        .resistor(OTA_IN, GROUND, 1.0e3)
        // R342 and R346, then C137 into Q70's base.
        .resistor(OUT21, V37, 15.0e3)
        .resistor(V37, GROUND, 680.0)
        .capacitor(V37, B70, 0.033e-6)
        .resistor(B70, B138, 1.0e6)
        .capacitor(B138, GROUND, 0.047e-6)
        .resistor(B138, A139, 1.0e6)
        .capacitor(A139, GROUND, 0.018e-6)
        // C143 into Q71's base through R362, with R364 and R370.
        .capacitor(C143, GROUND, 1.0e-6)
        .resistor(C143, B71, 330.0e3)
        .resistor(B71, GROUND, 2.2e3)
        .resistor(B71, GROUND, R370);
    let charge = |node| Switch {
        node,
        other: GROUND,
        g: 1.0 / R_CHARGE,
        volts: 0.0,
    };
    b.build(dt, [charge(A139), charge(C143)])
}

fn output_network(dt: f32) -> Mna<OUT_UNKNOWNS, OUT_CAPS, 0, 1> {
    let mut b = MnaBuilder::<OUT_UNKNOWNS>::default();
    b.resistor(OTA_OUT, GROUND, 1.0 / STIFF)
        // C150 and R378 from the OTA's buffer.
        .capacitor(OTA_OUT, Q150, 0.001e-6)
        .resistor(Q150, M19, 100.0e3)
        // R363 and C146 on Q70's collector, then C152 and R376.
        .resistor(C70, GROUND, 10.0e3)
        .capacitor(C70, GROUND, 0.018e-6)
        .capacitor(C70, R152, 0.0047e-6)
        .resistor(R152, M19, 47.0e3)
        .resistor(M19, OUT19, R_FEEDBACK)
        .capacitor(M19, OUT19, 220.0e-12)
        .opamp(GROUND, M19, OUT19, OP19);
    b.build(dt, [])
}

/// Q70's emitter voltage when its emitter current is `ie`.
#[inline]
fn q70_emitter(ie: f64) -> f64 {
    let g = 1.0 / R369 as f64 + 1.0 / R200 as f64;
    (ie - RAIL as f64 / R200 as f64) / g
}

/// Q70's base current, from its base port's open-circuit voltage `open`
/// and impedance `z`, by Newton on its base-emitter voltage from `vbe`;
/// returns the base and collector currents.
#[inline]
fn q70(open: f32, z: f32, vbe: &mut f64) -> (f32, f32) {
    let (open, z, vt) = (open as f64, z as f64, VT as f64);
    let beta = BETA as f64;
    let g_emit = 1.0 / R369 as f64 + 1.0 / R200 as f64;
    let mut v = *vbe;
    let mut ie = 0.0;
    for _ in 0..30 {
        let e = (v / vt).min(80.0).exp();
        ie = IS * (e - 1.0);
        let die = IS * e / vt;
        // open − z·Ib = Vbe + Ve(Ie)
        let f = v + q70_emitter(ie) + z * ie / (beta + 1.0) - open;
        let df = 1.0 + die / g_emit + z * die / (beta + 1.0);
        let mut next = v - f / df;
        if next > 0.5 {
            next = v + (next - v).clamp(-0.4, 2.0 * vt);
        }
        let moved = (next - v).abs();
        v = next;
        if moved < 1.0e-7 {
            break;
        }
    }
    *vbe = v;
    let ib = ie / (beta + 1.0);
    (ib as f32, (ie - ib) as f32)
}

/// The clap's circuit state.
#[derive(Clone, Copy, Default)]
struct Clap {
    pulse: Pulse,
    noise: MnaState<NOISE_CAPS>,
    output: MnaState<OUT_CAPS>,
    envelope: Envelope,
    /// Node voltages on the last sample that set the switches.
    a139: f32,
    c143: f32,
    /// Q70's base-emitter voltage and collector current.
    q70_vbe: f64,
    q70: f32,
    /// Output scale for the hit's accent.
    level: Level,
    primed: bool,
    quiet: Quiet,
}

#[derive(Default)]
struct ClapShared {
    timing: Timing,
    noise_scale: f32,
    noise: Mna<NOISE_UNKNOWNS, NOISE_CAPS, NOISE_SWITCHES, NOISE_COMBOS>,
    envelope: EnvelopeNetwork,
    output: Mna<OUT_UNKNOWNS, OUT_CAPS, 0, 1>,
    /// The circuit at rest, its envelopes settled.
    rest: Clap,
}

impl Clap {
    /// Advances one sample with the trigger line at `line` (or released,
    /// pulled toward −15 V, while it is 0) and white noise `white`.
    #[inline]
    fn advance(&mut self, sh: &ClapShared, line: f32, white: f32) -> f32 {
        // Q69 follows the trigger line, charging C139 and C143.
        let mut closed = 0;
        let mut injected = [0.0f32; NOISE_UNKNOWNS];
        injected[WN] = white * STIFF;
        injected[B71] = TM3_WIPER / R370 + self.envelope.q71_base();
        if line > 0.0 {
            let target = line - Q69_VBE - DIODE_DROP;
            for (bit, node, now) in [(D69, A139, self.a139), (D68, C143, self.c143)] {
                if target > now {
                    closed |= 1 << bit;
                    injected[node] += target / R_CHARGE;
                }
            }
        }
        let x = sh.noise.open(&self.noise, closed, &injected);
        let z = sh.noise.impedance(closed, B70, B70);
        let (ib, ic) = q70(x[B70], z, &mut self.q70_vbe);
        let x = sh.noise.close(&mut self.noise, closed, x, &[(B70, -ib)]);
        self.a139 = x[A139];
        self.c143 = x[C143];
        self.q70 = ic;
        let (ota_in, b71) = (x[OTA_IN], x[B71]);

        // IC23c sees the trigger line divided against −15 V; released, the
        // line falls to −15 V.
        let m = if line > 0.0 {
            (line * R361 - RAIL * R360) / (R360 + R361)
        } else {
            -RAIL
        };
        let iabc = self.envelope.step(&sh.envelope, m, b71);
        let ota = ota(iabc, ota_in);

        let mut injected = [0.0f32; OUT_UNKNOWNS];
        injected[OTA_OUT] = ota * STIFF;
        injected[C70] = RAIL / 10.0e3 - self.q70;
        let x = sh.output.step(&mut self.output, 0, &injected);
        x[OUT19]
    }
}

impl Circuit for Clap {
    type Shared = ClapShared;
    /// The white noise generator's voltage.
    type Controls = f32;

    fn fire(&mut self, accent: f32, shared: &ClapShared) {
        let volts = pulse_volts(accent);
        let sounding = self.primed && !self.settled();
        if !self.primed {
            *self = shared.rest;
            self.primed = true;
        }
        self.level
            .set(volts / PULSE_V, sounding, shared.timing.choke_samples);
        self.pulse.fire(volts, shared.timing.pulse_samples);
    }

    fn settled(&self) -> bool {
        !self.pulse.active() && !self.envelope.window_open() && self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &ClapShared, &white: &f32) -> f32 {
        let line = self.pulse.next();
        let out = self.advance(sh, line, white) * self.level.next();
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Cp808Params {
    /// rising edges clap
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; doubles
    /// the level at full accent and lets the tail ring longer (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// fade out a still-sounding clap over 2 ms as the next one strikes, so
    /// every clap starts from rest; otherwise claps run into each other, as
    /// on the hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Cp808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 hand clap.
///
/// Each rising edge on **trigger** fires the 808's trigger pulse into a
/// circuit model of its hand clap: band-passed noise through a VCA that a
/// sawtooth envelope opens several times in quick succession, over a slower
/// "reverb" of the same noise. **accent**, sampled at each strike, doubles
/// the level and lets the tail ring longer.
///
/// An unaccented clap peaks near **±5 V**.
///
/// ```js
/// $tr808.cp($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.cp", has_init, patch_update, args(trigger))]
pub struct Cp808 {
    outputs: Cp808Outputs,
    params: Cp808Params,
    state: CpState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct CpState {
    shared: ClapShared,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Clap>,
    noise: WhiteNoise,
}

/// A time step long enough that every capacitor in the circuit carries no
/// current: the networks discretised at it solve for the circuit at DC.
const DC_STEP: f32 = 1.0e3;
/// Iterations that settle the switches and Q70 at DC.
const DC_ITERATIONS: usize = 20;

impl Cp808 {
    fn init(&mut self, sample_rate: f32) {
        let sh = &mut self.state.shared;
        sh.noise = noise_network(DC_STEP);
        sh.envelope = EnvelopeNetwork::new(DC_STEP);
        sh.output = output_network(DC_STEP);
        let mut rest = Clap {
            primed: true,
            ..Clap::default()
        };
        for _ in 0..DC_ITERATIONS {
            rest.advance(sh, 0.0, 0.0);
        }
        rest.quiet = Quiet::default();
        sh.rest = rest;
        let dt = 1.0 / sample_rate;
        sh.timing = Timing::new(sample_rate);
        sh.noise_scale = WhiteNoise::scale(sample_rate);
        sh.noise = noise_network(dt);
        sh.envelope = EnvelopeNetwork::new(dt);
        sh.output = output_network(dt);
    }

    fn update(&mut self, _sample_rate: f32) {
        let sh = &self.state.shared;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];
            let white = cs.noise.next(sh.noise_scale);
            let out = cs.voices.process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                sh.timing.choke_samples,
                sh,
                &white,
            );
            self.outputs.sample.set(ch, out * OUTPUT_GAIN);
        }
    }
}

impl crate::types::PatchUpdateHandler for Cp808 {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.noise.seed(base, ch);
        }
    }
}

message_handlers!(impl Cp808 {});
