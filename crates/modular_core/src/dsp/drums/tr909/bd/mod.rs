//! TR-909 bass drum, simulated from the 909 service notes' voicing board
//! schematic. Part names (Q12, C9, …) are the service notes' designators.
//!
//! A triangle VCO (integrator IC13a, Schmitt trigger IC13b, with IC12b
//! flipping the sign of its control voltage) runs at a rate set by IC12a:
//! a fixed offset from the rail through R27 plus the pitch envelope on C9,
//! which the trigger charges and the tune pot drains. The trigger also
//! stops the VCO for its 2 ms through Q11, so every hit starts from the
//! same point. Two diodes round the triangle toward a sine, and transistor
//! Q12, its base fed from the decay envelope on C8, passes it into the
//! output stage IC11a. Alongside, Q6 passes the attack: a ringing click
//! from the pulse shaper Q8/Q9 and a burst of low-passed noise, gated by a
//! short envelope on C12.

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{
    DIODE_DROP, RAIL, Shaper, TRIG_V, TaperedPot, Timing, VCA, VSAT, accent_volts, vca_port,
};
use super::noise::Noise;
use crate::dsp::drums::analog::Junction;
use crate::dsp::drums::group::{self, Channels, Groups, Run};
use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState, Switch};
use crate::dsp::drums::voice::{Circuit, Fire, Pulse, Quiet, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal};

mod module;
mod networks;

pub use module::Bd909;
use networks::{attack_network, envelope_network};

// The pitch envelope: R21 from the trigger through D7 into C9, drained by
// R57 and the tune pot VR2 to ground and by R26 into IC12a's virtual
// ground.
const R21: f32 = 2.2e3;
const C9: f32 = 0.33e-6;
const R57: f32 = 22.0e3;
const VR2: f32 = 100.0e3;
const R26: f32 = 330.0e3;
/// IC12a's feedback, and R27 from the +15 V rail.
const R28: f32 = 150.0e3;
const R27: f32 = 1.5e6;

// The VCO: IC12b's output, ±the control voltage, through R59 into C14;
// IC13b's thresholds are its swing divided by R49 and R50.
const R59: f32 = 47.0e3;
const C14: f32 = 0.033e-6;
const THRESHOLD: f32 = VSAT * 47.0e3 / (100.0e3 + 47.0e3);
/// Q11 holds C14 through R51 while the trigger lasts.
const R51: f32 = 2.2e3;

// The shaper: R52 into D10 and D11 to ground, then R53 into Q12's
// collector, with R43 to ground.
const SHAPER: Shaper = Shaper {
    r_in: 10.0e3,
    r_out: 2.2e3,
    r_load: 47.0,
};
/// R34, from the decay envelope to Q12's base.
const R34: f32 = 470.0e3;

// The decay envelope: Q4 gates the accent voltage through R35 into node X,
// with R36 and R22 dividing it into node Y, D8 from X to Y, and D9 from X
// into C8; the decay pot VR5 and R58 drain C8.
const DV: usize = 0;
const X: usize = 1;
const Y: usize = 2;
const E1: usize = 3;
const ENV_UNKNOWNS: usize = 4;
const ENV_CAPS: usize = 1;
const GATE: usize = 0;
const D8: usize = 1;
const D9: usize = 2;
const ENV_SWITCHES: usize = 3;
const ENV_COMBOS: usize = 1 << ENV_SWITCHES;
/// Q4's saturated resistance, and a diode's while it conducts.
const R_SWITCH: f32 = 20.0;
const R_DIODE: f32 = 30.0;
const VR5: f32 = 1.0e6;
const R58: f32 = 47.0e3;

// Q4 stays on while Q3 conducts: C7, charged by the trigger through R20
// and D6, drains through R18 into Q3's base, with R17 to ground.
const R20: f32 = 6.8e3;
const C7: f32 = 0.033e-6;
const R18: f32 = 470.0e3;
const R17: f32 = 470.0e3;
/// R32, through which Q3 pulls Q4's base.
const R32: f32 = 47.0e3;
const VBE: f32 = 0.6;
const BETA: f32 = 300.0;

// IC11b scales the accent voltage by 1 + R37/R38.
const ACCENT_GAIN: f32 = 1.0 + 10.0e3 / 4.7e3;
/// The accent voltage of an unaccented hit.
const ACCENT_LOW: f32 = 2.1;

// The attack envelope: Q5 passes the scaled accent through R41 into C12
// for the trigger's length; R493 drains it, and R42 feeds Q6's base.
const R41: f32 = 22.0e3;
const C12: f32 = 0.033e-6;
const R493: f32 = 150.0e3;
const R42: f32 = 470.0e3;

// The attack network: the noise through R10 and R9, C15, R46, C13 and R45;
// the pulse shaper; both into node NB (R56 to ground), then through the
// attack pot VR3 into Q6's collector with R55 to ground.
const NOISE_IN: usize = 0;
const N1: usize = 1;
const N2: usize = 2;
const N3: usize = 3;
const NB: usize = 4;
const P0: usize = 5;
const P1: usize = 6;
const PA: usize = 7;
const PB: usize = 8;
const PE: usize = 9;
const P4: usize = 10;
const OP9: usize = 11;
const ATTACK_UNKNOWNS: usize = 12;
const ATTACK_CAPS: usize = 7;
/// Q8 conducting, holding the pulse shaper's input at the rail.
const Q8_ON: usize = 0;
const ATTACK_SWITCHES: usize = 1;
const ATTACK_COMBOS: usize = 1 << ATTACK_SWITCHES;
/// Conductance driving the noise input to the generator's voltage.
const STIFF: f32 = 10.0;
const VR3: f32 = 500.0;
const R55: f64 = 47.0;

/// The noise generator's mean voltage: high half the time.
const NOISE_MEAN: f32 = 0.5 * RAIL;
const DC_STEP: f32 = 1.0e3;
const DC_ITERATIONS: usize = 4;

/// IC11a's feedback with the level pot at maximum.
const R_LEVEL: f64 = 100.0e3;

/// Scales the output so an unaccented hit peaks near 5 V.
const OUTPUT_GAIN: f32 = 1.467;
/// Below this running peak the drum has faded out.
const SETTLED: f32 = 1.0e-4;
/// The decay envelope at rest: Q12 passes nothing below it.
const E1_SETTLED: f32 = 0.3;

/// The bass drum's circuit state.
#[derive(Clone, Copy, Default)]
struct BassDrum<L: Lanes = Mono> {
    trigger: Pulse<L>,
    /// The scaled accent voltage for this hit.
    accent: L::S,
    c9: L::S,
    c7: L::S,
    c12: L::S,
    /// The triangle, and the Schmitt trigger's output: rising while true.
    tri: L::S,
    rising: L::SM,
    envelope: MnaState<ENV_CAPS, L>,
    x: L::S,
    y: L::S,
    e1: L::S,
    attack: MnaState<ATTACK_CAPS, L>,
    /// The shaper's diode node and Q12's collector, Q6's collector.
    d: Junction<L>,
    c12_node: Junction<L>,
    c6_node: Junction<L>,
    /// Base-emitter junctions carried between samples for Newton.
    q12_vbe: Junction<L>,
    q6_vbe: Junction<L>,
    q12_base: L::S,
    q6_base: L::S,
    /// False until the first strike sets the attack network at rest.
    primed: L::SM,
    quiet: Quiet<L>,
}

lanewise!(BassDrum {
    trigger,
    accent,
    c9,
    c7,
    c12,
    tri,
    rising,
    envelope,
    x,
    y,
    e1,
    attack,
    d,
    c12_node,
    c6_node,
    q12_vbe,
    q6_vbe,
    q12_base,
    q6_base,
    primed,
    quiet,
});

#[derive(Default)]
struct DrumShared {
    timing: Timing,
    dt: f32,
    noise_rate: f32,
    envelope: Mna<ENV_UNKNOWNS, ENV_CAPS, ENV_SWITCHES, ENV_COMBOS>,
    attack: Mna<ATTACK_UNKNOWNS, ATTACK_CAPS, ATTACK_SWITCHES, ATTACK_COMBOS>,
    /// The attack network at rest, Q8 on and the noise at its mean.
    attack_rest: MnaState<ATTACK_CAPS>,
    sync_coeff: f32,
    /// C7's per-sample approach to the trigger through R20, and its bleed
    /// through R18.
    c7_charge: f32,
    c7_bleed: f32,
    /// C12's per-sample approach to its target while the trigger charges it
    /// through R41, and with only R493 draining it.
    c12_charge: f32,
    c12_bleed: f32,
}

#[derive(Clone, Copy, Default)]
struct DrumControls<L: Lanes = Mono> {
    /// Conductances draining C9 and C8, VR3's resistance, and the noise.
    tune: L::S,
    decay: L::S,
    attack: L::S,
    noise: L::S,
}

lanewise!(DrumControls {
    tune,
    decay,
    attack,
    noise
});

/// The bass drum at every width.
struct BassDrumFamily;

impl group::Drum for BassDrumFamily {
    type Shared = DrumShared;
    type Circuit<L: Lanes> = BassDrum<L>;
    type Controls<L: Lanes> = DrumControls<L>;
    type Sources<L: Lanes> = ();
    const QUAD: bool = false;
    const PAIR: bool = false;
}

impl<L: Lanes> BassDrum<L> {
    /// Advances the triangle one sample at control voltage `cv` (its
    /// magnitude sets the slope), reflecting off IC13b's thresholds.
    #[inline]
    fn vco(&mut self, cv: L::S, dt: f32) {
        let mut travel = cv.abs() / (R59 * C14) * dt;
        let mut active = travel.gt(c(0.0));
        while active.any() {
            let room = L::S::select(
                self.rising,
                c::<L::S>(THRESHOLD) - self.tri,
                self.tri + THRESHOLD,
            );
            let finish = active & travel.lt(room);
            let moved = self.tri + L::S::select(self.rising, travel, -travel);
            let reflect = active & !finish;
            let edge = L::S::select(self.rising, c(THRESHOLD), c(-THRESHOLD));
            self.tri = L::S::select(finish, moved, L::S::select(reflect, edge, self.tri));
            travel = L::S::select(reflect, travel - room.max(c(0.0)), travel);
            self.rising = (reflect & !self.rising) | (!reflect & self.rising);
            active = reflect & travel.gt(c(0.0));
        }
    }

    /// C7's voltage below which Q3 can no longer hold Q4 on.
    fn gate_threshold(&self) -> L::S {
        let need = (self.accent - VBE).max(c(0.0)) / (R32 * BETA);
        (need + VBE / R17) * R18 + VBE
    }
}

impl Fire for BassDrum {
    fn fire(&mut self, accent: f32, shared: &DrumShared) {
        if !self.primed {
            self.attack = shared.attack_rest;
            self.primed = true;
        }
        self.accent = (ACCENT_GAIN * accent_volts(ACCENT_LOW, accent)).min(VSAT);
        self.trigger.fire(TRIG_V, shared.timing.trig_samples);
    }
}

impl<L: Lanes> Circuit<L> for BassDrum<L> {
    type Shared = DrumShared;
    type Controls = DrumControls<L>;

    fn settled(&self) -> L::SM {
        !self.trigger.active()
            & self.c7.lt(self.gate_threshold())
            & self.e1.lt(c(E1_SETTLED))
            & self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &DrumShared, ctl: &DrumControls<L>) -> L::S {
        let dt = sh.dt;
        let trig = self.trigger.next();
        let triggered = trig.gt(c(0.0));

        // The pitch envelope and IC12a.
        let charging = triggered & (trig - DIODE_DROP).gt(self.c9);
        let g_charge = L::S::select(charging, c(1.0 / R21), c(0.0));
        let target = L::S::select(charging, trig - DIODE_DROP, c(0.0));
        let g = g_charge + ctl.tune + 1.0 / R26;
        let settle = g_charge * target / g;
        self.c9 = settle + (self.c9 - settle) * (-g * dt / C9).map(f32::exp);
        let cv = self.c9 * -(R28 / R26) - (R28 / R27) * RAIL;

        // The VCO, held by Q11 while the trigger lasts.
        let held = self.tri * sh.sync_coeff;
        if !triggered.all() {
            self.vco(cv, dt);
        }
        self.tri = L::S::select(triggered, held, self.tri);
        self.rising = triggered | self.rising;

        // Q3 keeps the decay gate open while C7 holds up.
        let top = trig - DIODE_DROP;
        let charged = top + (self.c7 - top) * sh.c7_charge;
        let bled = (self.c7 - VBE) * sh.c7_bleed + VBE;
        self.c7 = L::S::select(
            triggered,
            charged,
            L::S::select(self.c7.gt(c(VBE)), bled, self.c7),
        );
        let gate = triggered | self.c7.gt(self.gate_threshold());

        // The decay envelope.
        let mut injected = [c(0.0); ENV_UNKNOWNS];
        injected[DV] = L::S::select(gate, self.accent / R_SWITCH, c(0.0));
        let d8 = (self.x - self.y).gt(c(DIODE_DROP));
        let d9 = (self.x - self.e1).gt(c(DIODE_DROP));
        let mut switches = [L::SM::default(); ENV_SWITCHES];
        switches[GATE] = gate;
        switches[D8] = d8;
        switches[D9] = d9;
        let closed = L::closed(switches);
        injected[E1] -= self.q12_base;
        let x = sh.envelope.open(&self.envelope, closed, &injected);
        let z = sh.envelope.impedance::<L>(closed, E1, E1);
        let drain = -ctl.decay * x[E1] / (z * ctl.decay + 1.0);
        let x = sh
            .envelope
            .close(&mut self.envelope, closed, x, &[(E1, drain)]);
        self.x = x[X];
        self.y = x[Y];
        self.e1 = x[E1];
        let ib12 = VCA.base_current(self.e1, R34, &mut self.q12_vbe);
        self.q12_base = L::narrow(ib12);

        // The attack envelope.
        let g_charge = L::S::select(triggered, c(1.0 / R41), c(0.0));
        let target = L::S::select(triggered, self.accent, c(0.0));
        let mul = L::S::select(triggered, c(sh.c12_charge), c(sh.c12_bleed));
        let g = g_charge + 1.0 / R493;
        let settle = (g_charge * target - self.q6_base) / g;
        self.c12 = settle + (self.c12 - settle) * mul;
        let ib6 = VCA.base_current(self.c12, R42, &mut self.q6_vbe);
        self.q6_base = L::narrow(ib6);

        // The attack network: noise and the pulse into Q6.
        let mut injected = [c(0.0); ATTACK_UNKNOWNS];
        injected[NOISE_IN] = ctl.noise * STIFF;
        injected[PE] = c(-RAIL / 10.0e3);
        let mut switches = [L::SM::default(); ATTACK_SWITCHES];
        switches[Q8_ON] = !triggered;
        let closed = L::closed(switches);
        let x = sh.attack.open(&self.attack, closed, &injected);
        let z = L::wide(sh.attack.impedance::<L>(closed, NB, NB));
        let all = !L::DM::default();
        let (ie6, drawn) = vca_port(
            all,
            &mut self.c6_node,
            L::wide(x[NB]),
            z + L::wide(ctl.attack),
            R55,
            ib6,
        );
        sh.attack
            .close(&mut self.attack, closed, x, &[(NB, -L::narrow(drawn))]);

        let tri = L::wide(self.tri);
        let ie12 = SHAPER.solve(all, &mut self.d, &mut self.c12_node, tri, ib12);
        let out = L::narrow(-((ie12 + ie6) * R_LEVEL)).clamp(c(-VSAT), c(VSAT));
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}
