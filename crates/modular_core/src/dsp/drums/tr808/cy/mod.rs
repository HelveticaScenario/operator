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
use crate::dsp::drums::group::{Channels, Drum, Groups, QUAD, Run};
use crate::dsp::drums::lanes::{Lanes, Lanewise, Mono, Real, c, lanewise};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState, Switch};
use crate::dsp::drums::voice::{Circuit, Fire, Pulse, Quiet, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal};

mod module;
mod networks;

pub use module::Cy808;
use networks::{envelope_network, high_network, low_network, mid_network, tone_network};

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
const OUTPUT_GAIN: f32 = 2.063;
/// Below this running peak, some 77 dB under an unaccented hit, the
/// cymbal has faded out; C38 and C40 would otherwise drain through their
/// diodes' last microamps for seconds more.
const SETTLED: f32 = 3.0e-4;

/// The cymbal's circuit state.
#[derive(Clone, Copy, Default)]
struct Cymbal<L: Lanes = Mono> {
    pulse: Pulse<L>,
    smooth: L::S,
    env: MnaState<ENV_CAPS, L>,
    /// The envelope nodes E1 and E2A, C41, Q20's emitter and Q20's emitter
    /// current on the last sample.
    feeds: [L::S; 2],
    b41: L::S,
    e20: L::S,
    q20: L::S,
    high: MnaState<HIGH_CAPS, L>,
    mid: MnaState<MID_CAPS, L>,
    low: MnaState<LOW_CAPS, L>,
    bases: [SwingState<L>; 3],
    /// The VCA nodes on the last sample.
    nodes: [L::S; 3],
    tone: MnaState<TONE_CAPS, L>,
    /// False until the first strike sets the bases at rest.
    primed: L::SM,
    quiet: Quiet<L>,
}

lanewise!(Cymbal {
    pulse,
    smooth,
    env,
    feeds,
    b41,
    e20,
    q20,
    high,
    mid,
    low,
    bases,
    nodes,
    tone,
    primed,
    quiet,
});

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

#[derive(Clone, Copy, Default)]
struct CymbalControls<L: Lanes = Mono> {
    high: L::S,
    low: L::S,
    /// Conductances of the decay pot and the tone pot's two halves.
    decay: L::S,
    tone: [L::S; 2],
}

lanewise!(CymbalControls {
    high,
    low,
    decay,
    tone
});

/// The cymbal at every width, with its oscillator bank.
struct CymbalDrum;

impl Drum for CymbalDrum {
    type Shared = CymbalShared;
    type Circuit<L: Lanes> = Cymbal<L>;
    type Controls<L: Lanes> = CymbalControls<L>;
    type Sources<L: Lanes> = MetalBank<L>;
    const QUAD: bool = true;
    const PAIR: bool = true;
}

/// Brings a fully accented hit to twice the level of an unaccented one.
const ACCENT_TRIM: f32 = 0.886;

impl Fire for Cymbal {
    fn accent_trim(_: &CymbalShared) -> f32 {
        ACCENT_TRIM
    }

    fn fire(&mut self, accent: f32, shared: &CymbalShared) {
        let volts = pulse_volts(accent);
        if !self.primed {
            self.bases = [shared.rest; 3];
            self.primed = true;
        }
        self.pulse
            .fire(metal_pulse(volts), shared.timing.pulse_samples);
    }
}

impl<L: Lanes> Circuit<L> for Cymbal<L> {
    type Shared = CymbalShared;
    type Controls = CymbalControls<L>;

    fn settled(&self) -> L::SM {
        !self.pulse.active() & self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &CymbalShared, ctl: &CymbalControls<L>) -> L::S {
        // Q19's smoothed pulse charges the envelopes through D6–D8 while it
        // stands above them.
        let vtrig = self.pulse.next();
        let smoothed = self.smooth + (vtrig - SMOOTH_VBE - self.smooth) * sh.smooth_coeff;
        self.smooth = L::S::select(vtrig.gt(c(0.0)), smoothed, c(0.0));
        let charge = self.smooth - CHARGE_DROP;
        let [e1, e2a] = self.feeds;
        let [n47, n50, n48] = self.nodes;
        let mut switches = [L::SM::default(); ENV_SWITCHES];
        let mut injected = [c(0.0); ENV_UNKNOWNS];
        for (bit, node, now) in [
            (CHARGE_E1, E1, e1),
            (CHARGE_E2A, E2A, e2a),
            (CHARGE_B41, B41, self.b41),
        ] {
            let on = charge.gt(now);
            switches[bit] = on;
            injected[node] = L::S::select(on, injected[node] + charge / R_CHARGE, injected[node]);
        }
        // Q20 follows C41 a base-emitter drop down while it can, drawing
        // its base current from C41.
        let follow = self.b41 - Q20_VBE;
        let q20_on = follow.gt(self.e20);
        switches[Q20_ON] = q20_on;
        injected[E20] = L::S::select(q20_on, injected[E20] + follow / R_Q20, injected[E20]);
        injected[B41] -= self.q20 / (Q20_BETA + 1.0);
        injected[E1] += n47 / R94;
        injected[E2] += n50 / R90;
        injected[J] += n50 / R91;
        injected[E3] += n48 / R104;
        let closed = L::closed(switches);
        let x = sh.env.open(&self.env, closed, &injected);
        // The decay pot shunts C41.
        let z = sh.env.impedance::<L>(closed, B41, B41);
        let decay_current = -ctl.decay * x[B41] / (z * ctl.decay + 1.0);
        let x = sh
            .env
            .close(&mut self.env, closed, x, &[(B41, decay_current)]);
        self.q20 = L::S::select(q20_on, (follow - x[E20]) / R_Q20, c(0.0));
        self.b41 = x[B41];
        self.e20 = x[E20];
        self.feeds = [x[E1], x[E2A]];
        let (e1, e2, e3, j) = (x[E1], x[E2], x[E3], x[J]);

        let dt = sh.dt;
        let open = L::closed([]);
        let mut inj = [c(0.0); HIGH_UNKNOWNS];
        inj[NODE] = e1 / R94;
        let hx = vca(
            &sh.high,
            &mut self.high,
            open,
            NODE,
            &mut self.bases[0],
            ctl.high,
            &inj,
            dt,
        );
        let mut inj = [c(0.0); MID_UNKNOWNS];
        inj[NODE] = e2 / R90 + j / R91;
        let mx = vca(
            &sh.mid,
            &mut self.mid,
            open,
            NODE,
            &mut self.bases[1],
            ctl.high,
            &inj,
            dt,
        );
        let mut inj = [c(0.0); LOW_UNKNOWNS];
        inj[NODE] = e3 / R104;
        let lx = vca(
            &sh.low,
            &mut self.low,
            open,
            NODE,
            &mut self.bases[2],
            ctl.low,
            &inj,
            dt,
        );
        self.nodes = [hx[NODE], mx[NODE], lx[NODE]];

        let mut inj = [c(0.0); TONE_UNKNOWNS];
        inj[T_A] = hx[HIGH_OUT] * STIFF;
        inj[T_B] = mx[MID_OUT] * STIFF;
        inj[T_C] = lx[LOW_OUT] * STIFF;
        let x = sh.tone.open(&self.tone, open, &inj);
        // The tone pot's halves shunt TT and V125: solve the two-port.
        let [g1, g3] = ctl.tone;
        let z = |a, b| sh.tone.impedance::<Mono>(0, a, b);
        let (a11, a12) = (g1 * z(TT, TT) + 1.0, g3 * z(TT, V125));
        let (a21, a22) = (g1 * z(V125, TT), g3 * z(V125, V125) + 1.0);
        let det = a11 * a22 - a12 * a21;
        let v_tt = (a22 * x[TT] - a12 * x[V125]) / det;
        let v_125 = (a11 * x[V125] - a21 * x[TT]) / det;
        let x = sh.tone.close(
            &mut self.tone,
            open,
            x,
            &[(TT, -g1 * v_tt), (V125, -g3 * v_125)],
        );
        let out = x[TONE_OUT];
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}
