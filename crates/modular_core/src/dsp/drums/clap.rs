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

/// The 808's BA662 load, R383, and the swing its output reaches.
const R383: f32 = 82.0e3;
const OTA_SWING: f32 = 13.5;

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
pub struct Envelope {
    net: MnaState<CAPS>,
    /// Node voltages on the last sample that set the switches.
    oc: f32,
    ob: f32,
    md: f32,
    k141: f32,
    od: f32,
    pd: f32,
    c144: f32,
    /// The pair's base currents.
    q72_base: f32,
    q71_base: f32,
}

impl Envelope {
    /// True while the window IC23b opens is open.
    pub fn window_open(&self) -> bool {
        self.ob < P
    }

    /// Q71's base current, which the caller's network supplies.
    pub fn q71_base(&self) -> f32 {
        self.q71_base
    }

    /// Advances one sample with IC23c's inverting input at `trigger` (the
    /// trigger line divided against −15 V) and Q71's base at `b71`,
    /// returning the OTA's control current.
    #[inline]
    pub fn step(&mut self, net: &EnvelopeNetwork, trigger: f32, b71: f32) -> f32 {
        // The comparators and Q73 switch on last sample's voltages.
        let mut closed = 0;
        if trigger > Q {
            closed |= 1 << SINK_C;
        }
        if P > self.oc {
            closed |= 1 << SINK_B;
        }
        if self.md > self.pd {
            closed |= 1 << SINK_D;
        }
        // IC23a sinks while IC23b's output stands above P; open, it lets
        // R365 charge C144 through D71.
        if self.ob < P && self.c144 < -DIODE_DROP {
            closed |= 1 << CHARGE_144;
        }
        if self.k141 < D70_CLAMP {
            closed |= 1 << CLAMP_70;
        }
        if 0.5 * (self.od + self.k141) > -RAIL + Q73_VBE {
            closed |= (1 << Q73_BASE) | (1 << Q73_COLLECTOR);
        }
        let mut injected = [0.0f32; UNKNOWNS];
        injected[B72] = self.q72_base;
        let x = net.mna.step(&mut self.net, closed, &injected);
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
        let tail = (RAIL - (b71.min(b72) + PAIR_VBE)).max(0.0) / R366;
        let share = 1.0 / (1.0 + ((b72 - b71) / VT).clamp(-80.0, 80.0).exp());
        let iabc = tail * share;
        self.q72_base = iabc / BETA;
        self.q71_base = (tail - iabc) / BETA;
        iabc
    }
}

/// The BA662's output current for control current `iabc` and voltage
/// `input` at its inverting input, the other grounded.
#[inline]
pub fn ota_current(iabc: f32, input: f32) -> f32 {
    iabc * (-input / (2.0 * VT)).tanh()
}

/// The BA662's output across the 808's load, R383, for control current
/// `iabc` and voltage `input` at its inverting input.
#[inline]
pub fn ota(iabc: f32, input: f32) -> f32 {
    (R383 * ota_current(iabc, input)).clamp(-OTA_SWING, OTA_SWING)
}
