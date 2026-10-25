//! The TR-808 hi-hats' circuits, from the 808 service notes' voicing board
//! schematic. Part names (Q28, C63, …) are the service notes' designators.
//!
//! Both hats take the upper band of the shared oscillator bank into a
//! swing-type VCA (Q27 for the open hat, Q30 for the closed), then a
//! Sallen-Key high-pass on an emitter follower and an inverting stage on
//! IC7. The closed hat's envelope is C63, charged through Q28 and drained by
//! its VCA; while it is up it also turns on Q23, which empties the open
//! hat's envelope, so a closed hat chokes an open one.
//!
//! The cut control bends that choke. Below stock it raises Q23's base
//! resistor, R173, until Q23 never turns on; above stock it gives Q23 a
//! drain path of its own across R134, down to a twentieth of R134.

use super::circuit::{PULSE_SECS, pulse_volts};
use super::metal::metal_pulse;
use super::swing::{SwingState, vca};
use crate::dsp::drums::analog::{Exp, Junction, VT, converged};
use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState, Switch};
use crate::dsp::drums::voice::{Circuit, Fire, Pulse, Quiet, Timing};

mod module;
mod open;
mod sides;

pub use module::Hh808;
use open::{Open, OpenControls};

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
fn charge_target<S: Real<Elem = f32>>(volts: S) -> S {
    metal_pulse(volts) - SWITCH_SAT - FOLLOWER_VBE
}

/// The base current of a transistor fed from `volts` through conductance
/// `g`, by Newton on its base-emitter junction from its last solution.
#[inline(always)]
fn base_current<L: Lanes>(volts: L::S, g: L::S, base: &mut Junction<L>) -> L::S {
    const DONE: f64 = converged(1.0e-7, VT as f64);
    let (v, g, inv_vt) = (L::wide(volts), L::wide(g), 1.0 / VT as f64);
    let a = IS / BETA as f64;
    let on = !v.lt(c(0.3)) & g.gt(c(0.0));
    if !on.any() {
        base.v = v.max(c(0.0));
        return c(0.0);
    }
    // From a cold start, begin near a conducting junction's drop: from
    // zero, Newton would climb the exponential a thermal voltage at a time.
    let mut vbe = L::D::select(base.v.gt(c(0.3)), base.v.clamp(c(0.0), v), v.min(c(0.6)));
    let mut e = base.e.at(vbe, inv_vt);
    let mut done = !on;
    for _ in 0..30 {
        // g·(v − vbe) = Ib(vbe)
        let f = (vbe - v) * g + (e.value - 1.0) * a;
        let df = e.value * a * inv_vt + g;
        let next = (vbe - f / df).clamp(c(0.0), v);
        let moved = next - vbe;
        vbe = L::D::select(done, vbe, next);
        done = done | (moved * moved).lt(c(DONE));
        if done.all() {
            break;
        }
        e = e.at_where(!done, vbe, inv_vt);
    }
    *base = Junction {
        v: L::D::select(on, vbe, v.max(c(0.0))),
        e: Exp::select(on, e, base.e),
    };
    L::S::select(L::narrow_mask(on), L::narrow((v - vbe) * g), c(0.0))
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
            timing: Timing::new(PULSE_SECS, sample_rate),
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
pub struct Closed<L: Lanes = Mono> {
    pulse: Pulse<L>,
    net: MnaState<CH_CAPS, L>,
    base: SwingState<L>,
    e63: L::S,
    /// Q23's base current, drawn from C63, and its base-emitter voltage.
    q23: L::S,
    q23_vbe: Junction<L>,
    primed: L::SM,
    quiet: Quiet<L>,
}

lanewise!(Closed {
    pulse,
    net,
    base,
    e63,
    q23,
    q23_vbe,
    primed,
    quiet
});

impl<L: Lanes> Closed<L> {
    /// Q23's base current, which sets how hard it drains the open hat.
    fn q23(&self) -> L::S {
        self.q23
    }
}

/// The closed hat's settings read once per sample.
#[derive(Clone, Copy, Default)]
pub struct ClosedControls<L: Lanes = Mono> {
    /// The upper band.
    pub band: L::S,
    /// Conductance from C63 to Q23's base.
    pub q23: L::S,
}

lanewise!(ClosedControls { band, q23 });

/// The cut control's parts for `volts`, -5–5 V with 0 V stock: Q23's base
/// conductance, and the resistance it drains C62 through. Stock drives Q23
/// far past saturation, so the base conductance falls fast to [`FULL`] by
/// -1 V, then on to [`FAINTEST`] and to none at -5 V.
fn cut(volts: f32) -> (f32, f32) {
    let x = volts.clamp(-5.0, 5.0) / 5.0;
    let base = if x <= -1.0 {
        0.0
    } else if x < -0.2 {
        FULL * (FAINTEST / FULL).powf((-x - 0.2) / 0.8)
    } else {
        FULL.powf(-x.min(0.0) / 0.2)
    };
    (base / R173, R134 / SHARPEST.powf(x.max(0.0)))
}

/// Q23's base conductance, in stock's, at the weakest drive that still
/// empties the open hat, and at one that barely ducks it.
const FULL: f32 = 0.02;
const FAINTEST: f32 = 1.0e-3;
/// R134 over Q23's drain resistance at 5 V.
const SHARPEST: f32 = 20.0;

/// Brings a fully accented hit to twice the level of an unaccented one.
const ACCENT_TRIM: f32 = 1.161;

impl Fire for Closed {
    fn accent_trim(_: &HatShared) -> f32 {
        ACCENT_TRIM
    }

    fn fire(&mut self, accent: f32, shared: &HatShared) {
        let volts = pulse_volts(accent);
        if !self.primed {
            self.base = shared.rest;
            self.primed = true;
        }
        self.pulse.fire(volts, shared.timing.trigger_samples);
    }
}

impl<L: Lanes> Circuit<L> for Closed<L> {
    type Shared = HatShared;
    type Controls = ClosedControls<L>;

    /// C63 can hold Q23 on after the hat falls silent, so the circuit runs
    /// until Q23 lets go of the open hat.
    fn settled(&self) -> L::SM {
        !self.pulse.active() & self.quiet.below(SETTLED) & !self.q23.gt(c(0.0))
    }

    #[inline]
    fn step(&mut self, sh: &HatShared, ctl: &ClosedControls<L>) -> L::S {
        let vtrig = self.pulse.next();
        let striking = vtrig.gt(c(0.0));
        let mut charge = L::SM::default();
        let mut injected = [c(0.0); CH_UNKNOWNS];
        if striking.any() {
            let target = charge_target(vtrig);
            charge = striking & target.gt(self.e63);
            injected[E63] = L::S::select(charge, target / R_Q28, c(0.0));
        }
        injected[E63] -= self.q23;
        let x = vca(
            &sh.closed,
            &mut self.net,
            L::closed([charge]),
            CH_NODE,
            &mut self.base,
            ctl.band,
            &injected,
            sh.dt,
        );
        self.e63 = x[E63];
        self.q23 = base_current(self.e63, ctl.q23, &mut self.q23_vbe);
        let out = x[CH_OUT];
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}
