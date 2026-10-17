//! The TR-808 hi-hats' circuits, from the 808 service notes' voicing board
//! schematic. Part names (Q28, C63, …) are the service notes' designators.
//!
//! Both hats take the upper band of the shared oscillator bank into a
//! swing-type VCA (Q27 for the open hat, Q30 for the closed), then a
//! Sallen-Key high-pass on an emitter follower and an inverting stage on
//! IC7. The closed hat's envelope is C63, charged through Q28 and drained by
//! its VCA; while it is up it also turns on Q23, which empties the open
//! hat's envelope, so a closed hat chokes an open one.

use super::circuit::{Timing, pulse_volts};
use super::metal::metal_pulse;
use super::swing::{SwingState, vca};
use crate::dsp::drums::analog::VT;
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState, Switch};
use crate::dsp::drums::voice::{Circuit, Pulse, Quiet};

/// The PNP trigger switches' (Q7, Q8) saturation voltage, and the emitter
/// followers' (Q24, Q28) base-emitter drop as they charge an envelope.
const SWITCH_SAT: f32 = 0.1;
const FOLLOWER_VBE: f32 = 0.65;
/// A follower's charging resistance: its collector resistor and its base
/// resistor divided by β.
const R_Q24: f32 = 470.0 + 10.0e3 / 301.0;
const R_Q28: f32 = 470.0 + 33.0e3 / 301.0;

/// The 2SC945's saturation current and forward current gain.
const IS: f64 = 10.0e-15;
const BETA: f32 = 300.0;

/// Below this running peak, some 77 dB under an unaccented hit, a hat has
/// faded out.
pub const SETTLED: f32 = 3.0e-4;

/// The voltage an envelope follower charges toward during a trigger pulse
/// of `volts` on the common trigger line.
fn charge_target(volts: f32) -> f32 {
    metal_pulse(volts) - SWITCH_SAT - FOLLOWER_VBE
}

/// The base current of a transistor fed from `volts` through `ohms`, by
/// Newton on its base-emitter voltage from `guess`.
pub fn base_current(volts: f32, ohms: f32, guess: &mut f64) -> f32 {
    let (v, r, vt) = (volts as f64, ohms as f64, VT as f64);
    let is_b = IS / BETA as f64;
    if v < 0.3 {
        *guess = v.max(0.0);
        return 0.0;
    }
    // From a cold start, begin near a conducting junction's drop: from
    // zero, Newton would climb the exponential a thermal voltage at a time.
    let mut vbe = if *guess > 0.3 {
        guess.clamp(0.0, v)
    } else {
        v.min(0.6)
    };
    for _ in 0..30 {
        let e = (vbe / vt).min(80.0).exp();
        // v = vbe + r·Ib(vbe)
        let f = vbe + r * is_b * (e - 1.0) - v;
        let df = 1.0 + r * is_b * e / vt;
        let next = (vbe - f / df).clamp(0.0, v);
        let moved = (next - vbe).abs();
        vbe = next;
        if moved < 1.0e-7 {
            break;
        }
    }
    *guess = vbe;
    ((v - vbe) / r) as f32
}

/// The output stage both hats share in form: the VCA node, a Sallen-Key
/// high-pass on an emitter follower, and an inverting stage on IC7 with its
/// level pot at maximum. Unknowns past `first` hold the stage.
struct Stage {
    node_c: f32,
    hp_c: f32,
    r_in: f32,
}

const HP_FEEDBACK: f32 = 2.7e3;
const HP_GROUND: f32 = 68.0e3;
const C_IN: f32 = 470.0e-12;
/// IC7's feedback: 470 kΩ with the 100 kΩ level pot across it, and 220 pF.
const R_FEEDBACK: f32 = 470.0e3 * 100.0e3 / 570.0e3;
const C_FEEDBACK: f32 = 220.0e-12;

impl Stage {
    /// Adds the stage on `node`, using the [`STAGE_UNKNOWNS`] unknowns from
    /// `first`, with the stage's output at `first + 4`.
    fn build<const N: usize>(&self, b: &mut MnaBuilder<N>, node: usize, first: usize) {
        let (h1, h2, follower, a, m, out, opamp, op_follower) = (
            first,
            first + 1,
            first + 2,
            first + 3,
            first + 5,
            first + 4,
            first + 6,
            first + 7,
        );
        b.capacitor(node, GROUND, self.node_c)
            .capacitor(node, h1, self.hp_c)
            .resistor(h1, follower, HP_FEEDBACK)
            .capacitor(h1, h2, self.hp_c)
            .resistor(h2, GROUND, HP_GROUND)
            .opamp(h2, follower, follower, op_follower)
            .capacitor(follower, a, C_IN)
            .resistor(a, m, self.r_in)
            .resistor(m, out, R_FEEDBACK)
            .capacitor(m, out, C_FEEDBACK)
            .opamp(GROUND, m, out, opamp);
    }
}

/// Unknowns a [`Stage`] adds, and their capacitors.
const STAGE_UNKNOWNS: usize = 8;
const STAGE_CAPS: usize = 5;

// The closed hat: C63, the VCA node, then the stage.
const E63: usize = 0;
const CH_NODE: usize = 1;
const CH_OUT: usize = 2 + 4;
const CH_UNKNOWNS: usize = 2 + STAGE_UNKNOWNS;
const CH_CAPS: usize = 1 + STAGE_CAPS;
const C63: f32 = 0.47e-6;
const R142: f32 = 33.0e3;
/// R173, from C63 to Q23's base.
const R173: f32 = 330.0e3;

const CH_STAGE: Stage = Stage {
    // C69, with C70 joined to it through D14 while the VCA conducts.
    node_c: 0.003e-6,
    hp_c: 0.001e-6,
    r_in: 33.0e3,
};

// The open hat's envelope network: C62 alone.
const E62: usize = 0;
const C62: f32 = 0.47e-6;
const R139: f32 = 1.0e6;
/// R134, from C62 to Q22's and Q23's collectors.
const R134: f32 = 100.0e3;
/// Q22's and Q23's saturation voltage as they drain C62.
const DRAIN_SAT: f32 = 0.1;
// The open hat's VCA network: its node, then the stage.
const OH_NODE: usize = 0;
const OH_OUT: usize = 1 + 4;
const OH_UNKNOWNS: usize = 1 + STAGE_UNKNOWNS;
const OH_CAPS: usize = STAGE_CAPS;
/// R144, from Q29's emitter to the VCA node.
const R144: f32 = 33.0e3;
/// R143, Q29's emitter resistor.
const R143: f32 = 33.0e3;
/// Q29's base-emitter drop as it follows C62.
const Q29_VBE: f32 = 0.62;

const OH_STAGE: Stage = Stage {
    // C64, with C65 joined to it through D13 while the VCA conducts.
    node_c: 0.003e-6,
    hp_c: 0.0015e-6,
    r_in: 39.0e3,
};

/// C60, timing when Q22 starts to drain the open hat: it charges from the
/// 15 V rail through the decay pot and R132, and Q21 empties it on each
/// strike.
const C60: f32 = 10.0e-6;
const RAIL: f32 = 15.0;
/// R133, from C60 to Q22's base.
const R133: f32 = 10.0e3;
/// Q21's saturated resistance as it empties C60.
const R_Q21: f32 = 20.0;

/// Networks fixed by the sample rate.
#[derive(Default)]
pub struct HatShared {
    pub timing: Timing,
    dt: f32,
    closed: Mna<CH_UNKNOWNS, CH_CAPS, 1, 2>,
    envelope: Mna<1, 1, 1, 2>,
    open: Mna<OH_UNKNOWNS, OH_CAPS, 0, 1>,
    rest: SwingState,
}

impl HatShared {
    pub fn new(sample_rate: f32) -> Self {
        let dt = 1.0 / sample_rate;
        let mut b = MnaBuilder::<CH_UNKNOWNS>::default();
        b.capacitor(E63, GROUND, C63).resistor(E63, CH_NODE, R142);
        CH_STAGE.build(&mut b, CH_NODE, 2);
        let closed = b.build(
            dt,
            [Switch {
                node: E63,
                other: GROUND,
                g: 1.0 / R_Q28,
                volts: 0.0,
            }],
        );
        let mut b = MnaBuilder::<1>::default();
        b.capacitor(E62, GROUND, C62).resistor(E62, GROUND, R139);
        let envelope = b.build(
            dt,
            [Switch {
                node: E62,
                other: GROUND,
                g: 1.0 / R_Q24,
                volts: 0.0,
            }],
        );
        let mut b = MnaBuilder::<OH_UNKNOWNS>::default();
        b.resistor(OH_NODE, GROUND, R144);
        OH_STAGE.build(&mut b, OH_NODE, 1);
        Self {
            timing: Timing::new(sample_rate),
            dt,
            closed,
            envelope,
            open: b.build(dt, []),
            rest: SwingState::rest(),
        }
    }
}

/// The closed hat's circuit: the trigger pulse, C63's envelope, Q30's VCA
/// and the output stage.
#[derive(Clone, Copy, Default)]
pub struct Closed {
    pulse: Pulse,
    net: MnaState<CH_CAPS>,
    base: SwingState,
    e63: f32,
    node: f32,
    /// Q23's base current, drawn from C63, and its base-emitter voltage.
    q23: f32,
    q23_vbe: f64,
    primed: bool,
    quiet: Quiet,
}

impl Closed {
    /// Q23's base current, which sets how hard it drains the open hat.
    pub fn q23(&self) -> f32 {
        self.q23
    }
}

impl Circuit for Closed {
    type Shared = HatShared;
    /// The upper band.
    type Controls = f32;

    fn fire(&mut self, accent: f32, shared: &HatShared) {
        let volts = pulse_volts(accent);
        if !self.primed {
            self.base = shared.rest;
            self.primed = true;
        }
        self.pulse.fire(volts, shared.timing.pulse_samples);
    }

    fn settled(&self) -> bool {
        !self.pulse.active() && self.quiet.below(SETTLED)
    }

    #[inline]
    fn step(&mut self, sh: &HatShared, &band: &f32) -> f32 {
        let vtrig = self.pulse.next();
        let mut closed = 0;
        let mut injected = [0.0f32; CH_UNKNOWNS];
        if vtrig > 0.0 {
            let target = charge_target(vtrig);
            if target > self.e63 {
                closed = 1;
                injected[E63] = target / R_Q28;
            }
        }
        injected[E63] -= self.q23;
        let x = vca(
            &sh.closed,
            &mut self.net,
            closed,
            CH_NODE,
            &mut self.base,
            band,
            &injected,
            self.node,
            sh.dt,
        );
        self.e63 = x[E63];
        self.node = x[CH_NODE];
        self.q23 = base_current(self.e63, R173, &mut self.q23_vbe);
        let out = x[CH_OUT];
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}

/// The open hat's circuit: the trigger pulse, C60's timer, C62's envelope,
/// Q27's VCA and the output stage.
#[derive(Clone, Copy, Default)]
pub struct Open {
    pulse: Pulse,
    /// C60's voltage and Q22's base-emitter voltage.
    c60: f32,
    q22_vbe: f64,
    envelope: MnaState<1>,
    e62: f32,
    /// Q29's base current, drawn from C62.
    q29: f32,
    net: MnaState<OH_CAPS>,
    base: SwingState,
    node: f32,
    primed: bool,
    quiet: Quiet,
}

/// Settings read once per sample.
pub struct OpenControls {
    pub band: f32,
    /// Conductance from the rail to C60: the decay pot and R132.
    pub decay: f32,
    /// Q23's base current, from the closed hat's envelope.
    pub q23: f32,
}

impl Open {
    /// Advances C60 one sample by backward Euler, with Q21 emptying it while
    /// `striking`, and returns Q22's base current.
    #[inline]
    fn timer(&mut self, dt: f32, decay: f32, striking: bool) -> f32 {
        let (vt, is_b) = (VT as f64, IS / BETA as f64);
        let (c, g, r) = ((C60 / dt) as f64, decay as f64, R133 as f64);
        let g21 = if striking { 1.0 / R_Q21 as f64 } else { 0.0 };
        let old = self.c60 as f64;
        let rail = RAIL as f64;
        // Newton on Q22's base-emitter voltage; C60 sits R133·Ib above it.
        let mut vbe = self.q22_vbe;
        let mut v = old;
        let mut ib = 0.0;
        for _ in 0..30 {
            let e = (vbe / vt).min(80.0).exp();
            ib = is_b * (e - 1.0);
            let dib = is_b * e / vt;
            v = vbe + r * ib;
            let dv = 1.0 + r * dib;
            let f = c * (v - old) - g * (rail - v) + ib + g21 * v;
            let df = (c + g + g21) * dv + dib;
            let mut next = vbe - f / df;
            if next > 0.5 {
                next = vbe + (next - vbe).clamp(-0.4, 2.0 * vt);
            }
            let moved = (next - vbe).abs();
            vbe = next;
            if moved < 1.0e-7 {
                break;
            }
        }
        self.q22_vbe = vbe;
        self.c60 = v as f32;
        ib as f32
    }
}

impl Circuit for Open {
    type Shared = HatShared;
    type Controls = OpenControls;

    fn fire(&mut self, accent: f32, shared: &HatShared) {
        let volts = pulse_volts(accent);
        if !self.primed {
            self.base = shared.rest;
            self.primed = true;
        }
        self.pulse.fire(volts, shared.timing.pulse_samples);
    }

    fn settled(&self) -> bool {
        !self.pulse.active() && self.quiet.below(SETTLED)
    }

    #[inline]
    fn step(&mut self, sh: &HatShared, c: &OpenControls) -> f32 {
        let vtrig = self.pulse.next();
        let q22 = self.timer(sh.dt, c.decay, vtrig > 0.0);

        let mut closed = 0;
        let mut injected = [0.0f32; 1];
        if vtrig > 0.0 {
            let target = charge_target(vtrig);
            if target > self.e62 {
                closed = 1;
                injected[E62] = target / R_Q24;
            }
        }
        injected[E62] -= self.q29;
        let x = sh.envelope.open(&self.envelope, closed, &injected);
        // Q22 and Q23 drain C62 through R134 as hard as their base currents
        // let them, until they saturate.
        let z = sh.envelope.impedance(closed, E62, E62);
        let demand = BETA * (q22 + c.q23);
        let available = ((x[E62] - DRAIN_SAT) / (R134 + z)).max(0.0);
        let drain = demand.min(available);
        let x = sh
            .envelope
            .close(&mut self.envelope, closed, x, &[(E62, -drain)]);
        self.e62 = x[E62];

        // Q29 follows C62 into the VCA node through R144.
        let e29 = (self.e62 - Q29_VBE).max(0.0);
        let mut injected = [0.0f32; OH_UNKNOWNS];
        injected[OH_NODE] = e29 / R144;
        let x = vca(
            &sh.open,
            &mut self.net,
            0,
            OH_NODE,
            &mut self.base,
            c.band,
            &injected,
            self.node,
            sh.dt,
        );
        self.node = x[OH_NODE];
        self.q29 = if e29 > 0.0 {
            (e29 / R143 + (e29 - self.node) / R144) / (BETA + 1.0)
        } else {
            0.0
        };
        let out = x[OH_OUT];
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}
