//! TR-808 cymbal, simulated from the 808 service notes' voicing board
//! schematic after Werner, Abel and Smith's analysis ("The TR-808 Cymbal: a
//! Physically-Informed, Circuit-Bendable, Digital Model", ICMC/SMC 2014).
//! Part names (Q16, R93, …) are the service notes' designators.
//!
//! The shared oscillator bank's two bands drive three swing-type VCAs: Q16
//! and Q17 take the upper band, Q18 the lower. The trigger pulse, smoothed
//! by Q19, charges three envelope networks that feed the VCAs' collectors;
//! the decay pot sets how long Q18's lasts, and Q20 passes it on to Q17's
//! as well. Each VCA's node feeds its own high-pass filter, and the tone
//! stage mixes the three bands.

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{Timing, pulse_volts};
use super::metal::{MetalBank, MetalFilters, metal_pulse};
use super::swing::{SwingState, vca};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState, Switch};
use crate::dsp::drums::voice::{Circuit, Pulse, Quiet, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

mod module;

pub use module::Cy808;

// Q19 smooths the trigger pulse: a one-pole lag on the pulse less a
// base-emitter drop, fitted by Werner, Abel and Smith.
const SMOOTH_VBE: f32 = 0.7258;
const SMOOTH_TAU: f32 = 1.0244e-4;
/// D6–D8's drop as they charge the envelopes, and Q19's output resistance
/// as it does.
const CHARGE_DROP: f32 = 0.6;
const R_CHARGE: f32 = 50.0;
/// Q20's base-emitter drop and emitter resistance while it follows C41.
const Q20_VBE: f32 = 0.62;
const R_Q20: f32 = 250.0;
const Q20_BETA: f32 = 300.0;

// The envelope network's unknowns.
const E1: usize = 0;
const E37: usize = 1;
const E2A: usize = 2;
const E2: usize = 3;
const B41: usize = 4;
const E20: usize = 5;
const J: usize = 6;
const E3: usize = 7;
const ENV_UNKNOWNS: usize = 8;
const ENV_CAPS: usize = 6;
// Its switches: D6, D7 and D8 charging from Q19, and Q20 conducting.
const CHARGE_E1: usize = 0;
const CHARGE_E2A: usize = 1;
const CHARGE_B41: usize = 2;
const Q20_ON: usize = 3;
const ENV_SWITCHES: usize = 4;
const ENV_COMBOS: usize = 1 << ENV_SWITCHES;
// The resistors joining the envelopes to the VCA nodes.
const R94: f32 = 39.0e3;
const R90: f32 = 33.0e3;
const R91: f32 = 33.0e3;
const R104: f32 = 22.0e3;

// Each VCA network: the VCA node first.
const NODE: usize = 0;

const HIGH_UNKNOWNS: usize = 7;
const HIGH_OUT: usize = 5;
const HIGH_CAPS: usize = 5;
const MID_UNKNOWNS: usize = 6;
const MID_OUT: usize = 4;
const MID_CAPS: usize = 3;
const LOW_UNKNOWNS: usize = 5;
const LOW_OUT: usize = 3;
const LOW_CAPS: usize = 2;

// The tone stage: the three high-passes' outputs, driven in stiffly, into
// the tone network and IC6 with the level pot at maximum.
const T_A: usize = 0;
const T_B: usize = 1;
const T_C: usize = 2;
const T55: usize = 3;
const TT: usize = 4;
const V125: usize = 5;
const VV: usize = 6;
const T56: usize = 7;
const T58: usize = 8;
const F3: usize = 9;
const W: usize = 10;
const M6: usize = 11;
const TONE_OUT: usize = 12;
const TONE_OPAMP: usize = 13;
const TONE_UNKNOWNS: usize = 14;
const TONE_CAPS: usize = 6;
/// Conductance driving each input node to its source's voltage.
const STIFF: f32 = 10.0;

/// The decay pot, VR2: 2 MΩ linear, after R93.
const R93: f32 = 470.0e3;
const VR2: f32 = 2.0e6;
/// The tone pot, VR4: 20 kΩ linear, its wiper grounded. Each half keeps a
/// few ohms of track.
const VR4: f32 = 20.0e3;
const POT_END: f32 = 10.0;

/// Scales the output so an unaccented hit at stock settings peaks near 5 V.
const OUTPUT_GAIN: f32 = 2.27;
/// Below this running peak, some 77 dB under an unaccented hit, the
/// cymbal has faded out; C38 and C40 would otherwise drain through their
/// diodes' last microamps for seconds more.
const SETTLED: f32 = 3.0e-4;

const DEFAULT_DECAY: f32 = 2.5;
const DEFAULT_TONE: f32 = 2.5;

fn envelope_network(dt: f32) -> Mna<ENV_UNKNOWNS, ENV_CAPS, ENV_SWITCHES, ENV_COMBOS> {
    let mut b = MnaBuilder::<ENV_UNKNOWNS>::default();
    b.capacitor(E1, GROUND, 1.0e-6)
        .resistor(E1, E37, 22.0e3)
        .capacitor(E37, GROUND, 2.2e-6)
        .resistor(E1, GROUND, R94)
        .capacitor(E2A, GROUND, 1.0e-6)
        .resistor(E2A, E2, 33.0e3)
        .capacitor(E2, GROUND, 0.47e-6)
        .resistor(E2, GROUND, R90)
        .capacitor(B41, GROUND, 1.0e-6)
        .resistor(E20, J, 33.0e3)
        .resistor(J, GROUND, 10.0e3)
        .resistor(J, GROUND, R91)
        .resistor(E20, E3, 33.0e3)
        .capacitor(E3, GROUND, 2.2e-6)
        .resistor(E3, GROUND, R104);
    let charge = |node| Switch {
        node,
        other: GROUND,
        g: 1.0 / R_CHARGE,
        volts: 0.0,
    };
    b.build(
        dt,
        [
            charge(E1),
            charge(E2A),
            charge(B41),
            Switch {
                node: E20,
                other: GROUND,
                g: 1.0 / R_Q20,
                volts: 0.0,
            },
        ],
    )
}

/// Q16's node: C36 and C47 (the diode joins them while the VCA pulls the
/// node down), R94 from its envelope, and the third-order high-pass on
/// IC5a.
fn high_network(dt: f32) -> Mna<HIGH_UNKNOWNS, HIGH_CAPS, 0, 1> {
    let (h1, h2, h3, m5a, out, opamp) = (1, 2, 3, 4, HIGH_OUT, 6);
    let mut b = MnaBuilder::<HIGH_UNKNOWNS>::default();
    b.capacitor(NODE, GROUND, 0.0015e-6)
        .capacitor(NODE, GROUND, 0.0015e-6)
        .resistor(NODE, GROUND, R94)
        .capacitor(NODE, h1, 0.0033e-6)
        .resistor(h1, GROUND, 6.8e3)
        .capacitor(h1, h2, 0.001e-6)
        .resistor(h2, out, 18.0e3)
        .capacitor(h2, h3, 0.001e-6)
        .resistor(h3, GROUND, 18.0e3)
        .resistor(m5a, GROUND, 22.0e3)
        .resistor(m5a, out, 39.0e3)
        .opamp(h3, m5a, out, opamp);
    b.build(dt, [])
}

/// Q17's node: C43 and C50, R90 from its envelope and R91 from Q20, and the
/// high-pass on IC5b.
fn mid_network(dt: f32) -> Mna<MID_UNKNOWNS, MID_CAPS, 0, 1> {
    let (g1, g2, m5b, out, opamp) = (1, 2, 3, MID_OUT, 5);
    let mut b = MnaBuilder::<MID_UNKNOWNS>::default();
    b.capacitor(NODE, GROUND, 0.003e-6)
        .resistor(NODE, GROUND, R90)
        .resistor(NODE, GROUND, R91)
        .capacitor(NODE, g1, 0.001e-6)
        .capacitor(g1, g2, 0.001e-6)
        .resistor(g2, GROUND, 18.0e3)
        .resistor(g1, out, 18.0e3)
        .resistor(m5b, GROUND, 39.0e3)
        .resistor(m5b, out, 39.0e3)
        .opamp(g2, m5b, out, opamp);
    b.build(dt, [])
}

/// Q18's node: R104 from its envelope, and Q25's Sallen-Key high-pass with
/// Q25 as a unity follower.
fn low_network(dt: f32) -> Mna<LOW_UNKNOWNS, LOW_CAPS, 0, 1> {
    let (f1, f2, out, follower) = (1, 2, LOW_OUT, 4);
    let mut b = MnaBuilder::<LOW_UNKNOWNS>::default();
    b.resistor(NODE, GROUND, R104)
        .capacitor(NODE, f1, 0.0015e-6)
        .capacitor(f1, f2, 0.0015e-6)
        .resistor(f1, out, 22.0e3)
        .resistor(f2, GROUND, 82.0e3)
        .opamp(f2, out, out, follower);
    b.build(dt, [])
}

/// The tone stage and IC6, with the level pot at maximum. The tone pot's
/// halves are run-time shunts at `TT` and `V125`.
fn tone_network(dt: f32) -> Mna<TONE_UNKNOWNS, TONE_CAPS, 0, 1> {
    let mut b = MnaBuilder::<TONE_UNKNOWNS>::default();
    b.resistor(T_A, GROUND, 1.0 / STIFF)
        .resistor(T_B, GROUND, 1.0 / STIFF)
        .resistor(T_C, GROUND, 1.0 / STIFF)
        .capacitor(T_A, T55, 0.01e-6)
        .resistor(T55, TT, 22.0e3)
        .resistor(V125, VV, 2.2e3)
        .capacitor(T_B, T56, 0.01e-6)
        .resistor(T56, VV, 10.0e3)
        .capacitor(T_C, T58, 0.01e-6)
        .resistor(T58, F3, 100.0e3)
        .capacitor(F3, GROUND, 0.0033e-6)
        .resistor(F3, VV, 10.0e3)
        .resistor(TT, W, 22.0e3)
        .resistor(VV, W, 15.0e3)
        .capacitor(W, M6, 0.01e-6)
        .resistor(M6, TONE_OUT, 82.5e3)
        .capacitor(M6, TONE_OUT, 220.0e-12)
        .opamp(GROUND, M6, TONE_OUT, TONE_OPAMP);
    b.build(dt, [])
}

/// The cymbal's circuit state.
#[derive(Clone, Copy, Default)]
struct Cymbal {
    pulse: Pulse,
    smooth: f32,
    env: MnaState<ENV_CAPS>,
    /// The envelope nodes, C41, Q20's emitter and Q20's emitter current on
    /// the last sample.
    feeds: [f32; 4],
    b41: f32,
    e20: f32,
    q20: f32,
    high: MnaState<HIGH_CAPS>,
    mid: MnaState<MID_CAPS>,
    low: MnaState<LOW_CAPS>,
    bases: [SwingState; 3],
    /// The VCA nodes on the last sample.
    nodes: [f32; 3],
    tone: MnaState<TONE_CAPS>,
    /// False until the first strike sets the bases at rest.
    primed: bool,
    quiet: Quiet,
}

#[derive(Default)]
struct CymbalShared {
    timing: Timing,
    dt: f32,
    smooth_coeff: f32,
    env: Mna<ENV_UNKNOWNS, ENV_CAPS, ENV_SWITCHES, ENV_COMBOS>,
    high: Mna<HIGH_UNKNOWNS, HIGH_CAPS, 0, 1>,
    mid: Mna<MID_UNKNOWNS, MID_CAPS, 0, 1>,
    low: Mna<LOW_UNKNOWNS, LOW_CAPS, 0, 1>,
    tone: Mna<TONE_UNKNOWNS, TONE_CAPS, 0, 1>,
    rest: SwingState,
}

struct CymbalControls {
    high: f32,
    low: f32,
    /// Conductances of the decay pot and the tone pot's two halves.
    decay: f32,
    tone: [f32; 2],
}

impl Circuit for Cymbal {
    type Shared = CymbalShared;
    type Controls = CymbalControls;

    fn fire(&mut self, accent: f32, shared: &CymbalShared) {
        let volts = pulse_volts(accent);
        if !self.primed {
            self.bases = [shared.rest; 3];
            self.primed = true;
        }
        self.pulse
            .fire(metal_pulse(volts), shared.timing.pulse_samples);
    }

    fn settled(&self) -> bool {
        !self.pulse.active() && self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &CymbalShared, c: &CymbalControls) -> f32 {
        // Q19's smoothed pulse charges the envelopes through D6–D8 while it
        // stands above them.
        let vtrig = self.pulse.next();
        if vtrig > 0.0 {
            self.smooth += (vtrig - SMOOTH_VBE - self.smooth) * sh.smooth_coeff;
        } else {
            self.smooth = 0.0;
        }
        let charge = self.smooth - CHARGE_DROP;
        let [e1, e2a, _, _] = self.feeds;
        let [n47, n50, n48] = self.nodes;
        let mut closed = 0;
        let mut injected = [0.0f32; ENV_UNKNOWNS];
        for (bit, node, now) in [
            (CHARGE_E1, E1, e1),
            (CHARGE_E2A, E2A, e2a),
            (CHARGE_B41, B41, self.b41),
        ] {
            if charge > now {
                closed |= 1 << bit;
                injected[node] += charge / R_CHARGE;
            }
        }
        // Q20 follows C41 a base-emitter drop down while it can, drawing
        // its base current from C41.
        let follow = self.b41 - Q20_VBE;
        if follow > self.e20 {
            closed |= 1 << Q20_ON;
            injected[E20] += follow / R_Q20;
        }
        injected[B41] -= self.q20 / (Q20_BETA + 1.0);
        injected[E1] += n47 / R94;
        injected[E2] += n50 / R90;
        injected[J] += n50 / R91;
        injected[E3] += n48 / R104;
        let x = sh.env.open(&self.env, closed, &injected);
        // The decay pot shunts C41.
        let z = sh.env.impedance(closed, B41, B41);
        let decay_current = -c.decay * x[B41] / (1.0 + z * c.decay);
        let x = sh
            .env
            .close(&mut self.env, closed, x, &[(B41, decay_current)]);
        self.q20 = if closed & (1 << Q20_ON) != 0 {
            (follow - x[E20]) / R_Q20
        } else {
            0.0
        };
        self.b41 = x[B41];
        self.e20 = x[E20];
        self.feeds = [x[E1], x[E2A], x[E2], x[E3]];
        let (e1, e2, e3, j) = (x[E1], x[E2], x[E3], x[J]);

        let dt = sh.dt;
        let mut inj = [0.0f32; HIGH_UNKNOWNS];
        inj[NODE] = e1 / R94;
        let [last_h, last_m, last_l] = self.nodes;
        let hx = vca(
            &sh.high,
            &mut self.high,
            0,
            NODE,
            &mut self.bases[0],
            c.high,
            &inj,
            last_h,
            dt,
        );
        let mut inj = [0.0f32; MID_UNKNOWNS];
        inj[NODE] = e2 / R90 + j / R91;
        let mx = vca(
            &sh.mid,
            &mut self.mid,
            0,
            NODE,
            &mut self.bases[1],
            c.high,
            &inj,
            last_m,
            dt,
        );
        let mut inj = [0.0f32; LOW_UNKNOWNS];
        inj[NODE] = e3 / R104;
        let lx = vca(
            &sh.low,
            &mut self.low,
            0,
            NODE,
            &mut self.bases[2],
            c.low,
            &inj,
            last_l,
            dt,
        );
        self.nodes = [hx[NODE], mx[NODE], lx[NODE]];

        let mut inj = [0.0f32; TONE_UNKNOWNS];
        inj[T_A] = hx[HIGH_OUT] * STIFF;
        inj[T_B] = mx[MID_OUT] * STIFF;
        inj[T_C] = lx[LOW_OUT] * STIFF;
        let x = sh.tone.open(&self.tone, 0, &inj);
        // The tone pot's halves shunt TT and V125: solve the two-port.
        let [g1, g3] = c.tone;
        let z = |a, b| sh.tone.impedance(0, a, b);
        let (a11, a12) = (1.0 + z(TT, TT) * g1, z(TT, V125) * g3);
        let (a21, a22) = (z(V125, TT) * g1, 1.0 + z(V125, V125) * g3);
        let det = a11 * a22 - a12 * a21;
        let v_tt = (a22 * x[TT] - a12 * x[V125]) / det;
        let v_125 = (a11 * x[V125] - a21 * x[TT]) / det;
        let x = sh.tone.close(
            &mut self.tone,
            0,
            x,
            &[(TT, -g1 * v_tt), (V125, -g3 * v_125)],
        );
        let out = x[TONE_OUT];
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}
