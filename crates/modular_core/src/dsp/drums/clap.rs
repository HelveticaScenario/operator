//! The hand clap envelope Roland built into both the TR-808 and the TR-909,
//! part for part: four open-collector comparators (an AN6912), a capacitor
//! the trigger empties and a resistor refills to time a window, and inside
//! that window a capacitor that charges and is emptied again by a
//! transistor, over and over. Its voltage steers a differential pair whose
//! one collector current sets a BA662 OTA's gain, so the noise through the
//! OTA opens sharply and fades several times in a row; the window's close
//! empties the capacitor once more for the last, longest clap.
//!
//! Part names are the 808's (the 909's in brackets): comparators IC23a–d
//! (IC29a–d), C140 (C62) timing the window, C144 (C65) the sawtooth, Q73
//! (Q40) emptying it, Q71/Q72 (Q38) the pair.

use super::analog::VT;
use super::lanes::{Lanes, Mono, Real, c, lanewise};
use super::mna::{GROUND, Mna, MnaBuilder, MnaState, Switch};

const RAIL: f32 = 15.0;
const BETA: f32 = 300.0;
const DIODE_DROP: f32 = 0.6;

// The network's unknowns: C140 on IC23c's output, IC23b's output and its
// divider to IC23d, C141's far side, Q73's base, IC23d's output and its
// positive input, C144, Q73's collector, and Q72's base.
const OC: usize = 0;
const OB: usize = 1;
const MD: usize = 2;
const K141: usize = 3;
const B73: usize = 4;
const OD: usize = 5;
const PD: usize = 6;
const C144: usize = 7;
const C73: usize = 8;
const B72: usize = 9;
const UNKNOWNS: usize = 10;
const CAPS: usize = 4;
// Its switches: the open-collector outputs of IC23c, IC23b and IC23d
// sinking to −15 V; IC23a open, so R365 charges C144 through D71; D70
// clamping C141; Q73's base and collector conducting.
const SINK_C: usize = 0;
const SINK_B: usize = 1;
const SINK_D: usize = 2;
const CHARGE_144: usize = 3;
const CLAMP_70: usize = 4;
const Q73_BASE: usize = 5;
const Q73_COLLECTOR: usize = 6;
const SWITCHES: usize = 7;
const COMBOS: usize = 1 << SWITCHES;
/// An AN6912 output's resistance as it sinks.
const R_SINK: f32 = 200.0;
/// The comparators' thresholds: R353, R352 and R351 divide −15 V into
/// −5 V at `P` and −10 V at `Q`.
const P: f32 = -5.0;
const Q: f32 = -10.0;
const R365: f32 = 82.0e3;
/// D70's resistance while it clamps C141.
const R_CLAMP: f32 = 100.0;
const D70_CLAMP: f32 = -RAIL - DIODE_DROP;
/// Q73's base sits a junction drop above −15 V while it conducts, through
/// its spreading resistance; its collector saturates there through R367.
const Q73_VBE: f32 = 0.65;
const R_Q73_BASE: f32 = 1.0e3;
const Q73_SAT: f32 = 0.1;
const R_Q73_COLLECTOR: f32 = 10.0;

/// R366, feeding the differential pair from +15 V, and its base-emitter
/// drop.
const R366: f32 = 15.0e3;
const PAIR_VBE: f32 = 0.62;

/// The envelope generator's network, discretised for one time step.
#[derive(Default)]
pub struct EnvelopeNetwork {
    mna: Mna<UNKNOWNS, CAPS, SWITCHES, COMBOS>,
}

impl EnvelopeNetwork {
    pub fn new(dt: f32) -> Self {
        let mut b = MnaBuilder::<UNKNOWNS>::default();
        b.resistor(OC, GROUND, 1.0e6)
            .capacitor(OC, GROUND, 0.027e-6)
            .resistor(OB, MD, 2.7e3)
            .resistor(MD, GROUND, 5.6e3)
            .capacitor(OB, K141, 0.001e-6)
            .resistor(K141, B73, 68.0e3)
            .resistor(OD, B73, 68.0e3)
            .resistor(OD, GROUND, 10.0e3)
            .capacitor(OD, PD, 220.0e-12)
            .resistor(C144, PD, 1.0e6)
            .capacitor(C144, GROUND, 0.47e-6)
            .resistor(C144, C73, 330.0)
            .resistor(C73, B72, 470.0e3)
            .resistor(B72, GROUND, 15.0e3);
        let to = |node, g: f32, volts| Switch {
            node,
            other: GROUND,
            g,
            volts,
        };
        Self {
            mna: b.build(
                dt,
                [
                    to(OC, 1.0 / R_SINK, -RAIL),
                    to(OB, 1.0 / R_SINK, -RAIL),
                    to(OD, 1.0 / R_SINK, -RAIL),
                    to(C144, 1.0 / R365, -DIODE_DROP),
                    to(K141, 1.0 / R_CLAMP, D70_CLAMP),
                    to(B73, 1.0 / R_Q73_BASE, -RAIL + Q73_VBE),
                    to(C73, 1.0 / R_Q73_COLLECTOR, -RAIL + Q73_SAT),
                ],
            ),
        }
    }
}

/// The envelope generator's state.
#[derive(Clone, Copy, Default)]
pub struct Envelope<L: Lanes = Mono> {
    net: MnaState<CAPS, L>,
    /// Node voltages on the last sample that set the switches.
    oc: L::S,
    ob: L::S,
    md: L::S,
    k141: L::S,
    od: L::S,
    pd: L::S,
    c144: L::S,
    /// The pair's base currents.
    q72_base: L::S,
    q71_base: L::S,
}

lanewise!(Envelope {
    net,
    oc,
    ob,
    md,
    k141,
    od,
    pd,
    c144,
    q72_base,
    q71_base
});

impl<L: Lanes> Envelope<L> {
    /// True while the window IC23b opens is open.
    pub fn window_open(&self) -> L::SM {
        self.ob.lt(c(P))
    }

    /// Q71's base current, which the caller's network supplies.
    pub fn q71_base(&self) -> L::S {
        self.q71_base
    }

    /// Advances one sample with IC23c's inverting input at `trigger` (the
    /// trigger line divided against −15 V) and Q71's base at `b71`,
    /// returning the OTA's control current.
    #[inline]
    pub fn step(&mut self, net: &EnvelopeNetwork, trigger: L::S, b71: L::S) -> L::S {
        // The comparators and Q73 switch on last sample's voltages. IC23a
        // sinks while IC23b's output stands above P; open, it lets R365
        // charge C144 through D71.
        let mut closed = [L::SM::default(); SWITCHES];
        closed[SINK_C] = trigger.gt(c(Q));
        closed[SINK_B] = self.oc.lt(c(P));
        closed[SINK_D] = self.md.gt(self.pd);
        closed[CHARGE_144] = self.ob.lt(c(P)) & self.c144.lt(c(-DIODE_DROP));
        closed[CLAMP_70] = self.k141.lt(c(D70_CLAMP));
        let q73 = ((self.od + self.k141) * 0.5).gt(c(-RAIL + Q73_VBE));
        closed[Q73_BASE] = q73;
        closed[Q73_COLLECTOR] = q73;
        let mut injected = [c(0.0); UNKNOWNS];
        injected[B72] = self.q72_base;
        let x = net.mna.step(&mut self.net, L::closed(closed), &injected);
        self.oc = x[OC];
        self.ob = x[OB];
        self.md = x[MD];
        self.k141 = x[K141];
        self.od = x[OD];
        self.pd = x[PD];
        self.c144 = x[C144];
        let b72 = x[B72];

        // Q71 and Q72 share R366's current; Q72's share is the OTA's
        // control current.
        let tail = (c::<L::S>(RAIL) - (b71.min(b72) + PAIR_VBE)).max(c(0.0)) / R366;
        let exp = ((b72 - b71) / VT).clamp(c(-80.0), c(80.0)).map(f32::exp);
        let share = c::<L::S>(1.0) / (exp + 1.0);
        let iabc = tail * share;
        self.q72_base = iabc / BETA;
        self.q71_base = (tail - iabc) / BETA;
        iabc
    }
}

/// The BA662's output current for control current `iabc` and voltage
/// `input` at its inverting input, the other grounded.
#[inline]
pub fn ota_current<L: Lanes>(iabc: L::S, input: L::S) -> L::S {
    iabc * (-input / (2.0 * VT)).map(f32::tanh)
}
