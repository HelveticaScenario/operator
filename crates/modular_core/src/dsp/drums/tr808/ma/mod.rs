//! TR-808 maracas, simulated from the 808 service notes' voicing board
//! schematic. Part names (Q65, C134, …) are the service notes' designators.
//!
//! Q65, its base biased through 1 MΩ and swung by white noise through
//! C130, sources a heavily clipped noise current into a node that Q66 pulls
//! down through R337. At rest C134 holds Q66 off and Q65 saturated, pinning
//! the node to the rail. A trigger charges C135, turning Q67 on to drain
//! C134 through R341; as Q66 follows it down the noise comes through,
//! until C135 runs out and C134 recharges through R343. A Sallen-Key
//! high-pass on Q68 leaves only the hiss.

use crate::dsp::drums::lanes::Mono;
use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{PULSE_V, Timing, WhiteNoise, pulse_volts};
use crate::dsp::drums::analog::{high_pass, one_pole_coeff, rc_hz};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState, Switch};
use crate::dsp::drums::voice::{Circuit, Fire, Level, Pulse, Quiet, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal};

mod module;

pub use module::Ma808;

const RAIL: f32 = 15.0;
const BETA: f32 = 300.0;
const VBE: f32 = 0.65;
const DIODE_DROP: f32 = 0.6;

// The envelope network: C135 and C134.
const E135: usize = 0;
const E134: usize = 1;
const ENV_UNKNOWNS: usize = 2;
const ENV_CAPS: usize = 2;
// Its switches: D67 charging C135; Q67 conducting, draining C134 through
// R341; D66 charging C134 from R343 while Q67 is off; and R343 with R341
// topping C134 up once D66 stops conducting.
const D67: usize = 0;
const Q67: usize = 1;
const D66: usize = 2;
const TOP_UP: usize = 3;
const ENV_SWITCHES: usize = 4;
const ENV_COMBOS: usize = 1 << ENV_SWITCHES;
/// The trigger line's source and D67 as they charge C135.
const R_CHARGE: f32 = 100.0;
const R343: f32 = 100.0e3;
const R341: f32 = 470.0e3;
const R344: f32 = 220.0e3;
/// Q67's saturation voltage.
const Q67_SAT: f32 = 0.1;
/// Q67 saturates once its base current lets it sink all R343 can pass.
const Q67_ON: f32 = VBE + R344 * RAIL / (R343 * BETA);

// The noise network: the clipped node, C131 into the high-pass on Q68,
// and the mixer IC19 with its level pot at maximum.
const N: usize = 0;
const S131: usize = 1;
const T132: usize = 2;
const U133: usize = 3;
const O68: usize = 4;
const P151: usize = 5;
const M19: usize = 6;
const OUT19: usize = 7;
const OP68: usize = 8;
const OP19: usize = 9;
const NOISE_UNKNOWNS: usize = 10;
const NOISE_CAPS: usize = 5;
/// Its switch: Q66 conducting, pulling the node toward its emitter through
/// R337.
const Q66: usize = 0;
const R337: f32 = 10.0e3;
/// IC19's feedback: 470 kΩ with the 100 kΩ level pot across it.
const R_FEEDBACK: f32 = 470.0e3 * 100.0e3 / 570.0e3;
/// The highest the node reaches: Q65 saturated, less D65's drop.
const N_MAX: f32 = RAIL - 0.1 - DIODE_DROP;

// Q65's base: R336 to ground carries its bias while its base sits a
// junction drop below the rail; the noise reaches it through C130 and
// R335.
const R336: f32 = 1.0e6;
const R335: f32 = 6.8e3;
const C130: f32 = 0.047e-6;

/// Scales the output so an unaccented shake peaks near 5 V.
const OUTPUT_GAIN: f32 = 1.879;
/// Below this running peak the maracas have faded out.
const SETTLED: f32 = 3.0e-4;

fn envelope_network(dt: f32) -> Mna<ENV_UNKNOWNS, ENV_CAPS, ENV_SWITCHES, ENV_COMBOS> {
    let mut b = MnaBuilder::<ENV_UNKNOWNS>::default();
    b.capacitor(E135, GROUND, 0.1e-6)
        .resistor(E135, GROUND, 150.0e3)
        .capacitor(E134, GROUND, 0.033e-6);
    b.build(
        dt,
        [
            Switch {
                node: E135,
                other: GROUND,
                g: 1.0 / R_CHARGE,
                volts: 0.0,
            },
            Switch {
                node: E134,
                other: GROUND,
                g: 1.0 / R341,
                volts: Q67_SAT,
            },
            Switch {
                node: E134,
                other: GROUND,
                g: 1.0 / R343,
                volts: RAIL - DIODE_DROP,
            },
            Switch {
                node: E134,
                other: GROUND,
                g: 1.0 / (R343 + R341),
                volts: RAIL,
            },
        ],
    )
}

fn noise_network(dt: f32) -> Mna<NOISE_UNKNOWNS, NOISE_CAPS, 1, 2> {
    let mut b = MnaBuilder::<NOISE_UNKNOWNS>::default();
    b.capacitor(N, S131, 0.01e-6)
        .resistor(S131, GROUND, 15.0e3)
        .capacitor(S131, T132, 0.001e-6)
        .resistor(T132, O68, 3.3e3)
        .capacitor(T132, U133, 0.001e-6)
        .resistor(U133, GROUND, 68.0e3)
        .opamp(U133, O68, O68, OP68)
        .capacitor(O68, P151, 0.01e-6)
        .resistor(P151, M19, 100.0e3)
        .resistor(M19, OUT19, R_FEEDBACK)
        .capacitor(M19, OUT19, 220.0e-12)
        .opamp(GROUND, M19, OUT19, OP19);
    b.build(
        dt,
        [Switch {
            node: N,
            other: GROUND,
            g: 1.0 / R337,
            volts: 0.0,
        }],
    )
}

/// The maracas' circuit state.
#[derive(Clone, Copy, Default)]
struct Shake {
    pulse: Pulse,
    envelope: MnaState<ENV_CAPS>,
    noise: MnaState<NOISE_CAPS>,
    e135: f32,
    e134: f32,
    n: f32,
    /// Q66's base current, drawn from C134.
    q66_base: f32,
    /// C130's high-pass on the noise.
    c130: f32,
    /// Output scale for the hit's accent.
    level: Level,
    primed: bool,
    quiet: Quiet,
}

#[derive(Default)]
struct ShakeShared {
    timing: Timing,
    noise_scale: f32,
    envelope: Mna<ENV_UNKNOWNS, ENV_CAPS, ENV_SWITCHES, ENV_COMBOS>,
    noise: Mna<NOISE_UNKNOWNS, NOISE_CAPS, 1, 2>,
    c130_coeff: f32,
    rest: Shake,
}

impl Shake {
    /// Advances one sample with the trigger line at `line` and white noise
    /// `white`.
    #[inline]
    fn advance(&mut self, sh: &ShakeShared, line: f32, white: f32) -> f32 {
        let mut closed = 0;
        let mut injected = [0.0f32; ENV_UNKNOWNS];
        let target = line - DIODE_DROP;
        if line > 0.0 && target > self.e135 {
            closed |= 1 << D67;
            injected[E135] = target / R_CHARGE;
        }
        // Q67's base draws from C135 through R344.
        injected[E135] -= (self.e135 - VBE).max(0.0) / R344;
        if self.e135 > Q67_ON {
            closed |= 1 << Q67;
        } else if self.e134 < RAIL - DIODE_DROP {
            closed |= 1 << D66;
        } else {
            closed |= 1 << TOP_UP;
        }
        injected[E134] -= self.q66_base;
        let x = sh.envelope.step(&mut self.envelope, closed, &injected);
        self.e135 = x[E135];
        self.e134 = x[E134];

        // Q65's base current: its bias less the noise current through C130,
        // cut off below zero.
        let swing = high_pass(&mut self.c130, white, sh.c130_coeff) / R335;
        let q65 = BETA * ((RAIL - VBE) / R336 - swing).max(0.0);
        // Q66 follows C134 up a junction drop and pulls on the node while it
        // stands below the node.
        let emitter = self.e134 + VBE;
        let mut closed = 0;
        let mut injected = [0.0f32; NOISE_UNKNOWNS];
        if emitter < self.n {
            closed = 1 << Q66;
            injected[N] = emitter / R337;
        }
        let x = sh.noise.open(&self.noise, closed, &injected);
        // Q65 saturates once the node reaches the rail.
        let z = sh.noise.impedance::<Mono>(closed, N, N);
        let source = q65.min(((N_MAX - x[N]) / z).max(0.0));
        let x = sh.noise.close(&mut self.noise, closed, x, &[(N, source)]);
        self.n = x[N];
        self.q66_base = if closed != 0 {
            (self.n - emitter) / R337 / (BETA + 1.0)
        } else {
            0.0
        };
        x[OUT19]
    }
}

/// Brings a fully accented hit to twice the level of an unaccented one.
const ACCENT_TRIM: f32 = 0.798;

impl Fire for Shake {
    fn accent_trim(_: &ShakeShared) -> f32 {
        ACCENT_TRIM
    }

    fn fire(&mut self, accent: f32, shared: &ShakeShared) {
        let volts = pulse_volts(accent);
        let sounding = self.primed && !self.settled();
        if !self.primed {
            *self = shared.rest;
            self.primed = true;
        }
        self.level
            .set(volts / PULSE_V, sounding, shared.timing.fade_samples);
        self.pulse.fire(volts, shared.timing.pulse_samples);
    }
}

impl Circuit for Shake {
    type Shared = ShakeShared;
    /// The white noise generator's voltage.
    type Controls = f32;

    fn settled(&self) -> bool {
        !self.pulse.active() && self.e135 < Q67_ON && self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &ShakeShared, &white: &f32) -> f32 {
        let line = self.pulse.next();
        let out = self.advance(sh, line, white) * self.level.next();
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}
