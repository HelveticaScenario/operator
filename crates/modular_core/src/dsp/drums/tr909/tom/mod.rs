//! TR-909 toms, simulated from the 909 service notes' voicing board
//! schematic. Part names (Q19, C22, …) are the low tom's designators; the
//! three toms share the circuit and differ in capacitor values.
//!
//! Three triangle VCOs, each a comparator driving 4069 inverters into an
//! integrator, run at rates set by the inverters' supply: IC14a holds VDD
//! at the tune voltage, and IC14b holds VSS at its inverse, pulled further
//! down at the strike by C16 and C17, so the pitch bends and each triangle
//! leans. The trigger resets all three. VCO-1 passes through Q18, whose
//! base current fades with C24, clipping it ever harder until it stops;
//! diodes round each VCO toward a sine, and transistor VCAs Q19, Q20 and
//! Q21, each with its own envelope, pass them to the output stage IC18a.
//! The decay pot sets VCO-2's envelope, which VCO-1's drains into. A burst
//! of high-passed noise from the shared tom noise stage rides on VCO-3's
//! envelope.

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{
    COLLECTOR, DIODE, DIODE_DROP, RAIL, Shaper, TRIG_V, Timing, VCA, VSAT, accent_volts,
    diode_pair, vca_port,
};
use super::noise::Noise;
use crate::dsp::drums::analog::{Exp, Junction, VT, converged};
use crate::dsp::drums::group::{Channels, Drum, Groups, Run};
use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState};
use crate::dsp::drums::voice::{Circuit, Fire, Pulse, Quiet, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal};

mod modules;
mod vco;

pub use modules::{Ht909, Lt909, Mt909};
use vco::*;

/// The capacitors that set a tom apart.
#[derive(Clone, Copy)]
struct TomParts {
    /// The VCOs' integrator capacitors: VCO-1 (C18), VCO-2 (C19), VCO-3
    /// (C20).
    vco: [f32; 3],
    /// C24, timing Q18's clipping.
    clip: f32,
    /// C22 (VCO-1's envelope), C23 and C21 (VCO-2's, either side of
    /// R100), and C25 (VCO-3's).
    env1: f32,
    env2: f32,
    env2_pot: f32,
    env3: f32,
}

const LOW: TomParts = TomParts {
    vco: [0.022e-6, 0.033e-6, 0.012e-6],
    clip: 0.01e-6,
    env1: 0.22e-6,
    env2: 0.68e-6,
    env2_pot: 0.68e-6,
    env3: 0.056e-6,
};

const MID: TomParts = TomParts {
    vco: [0.018e-6, 0.027e-6, 0.01e-6],
    clip: 0.0068e-6,
    env1: 0.15e-6,
    env2: 0.47e-6,
    env2_pot: 0.47e-6,
    env3: 0.056e-6,
};

const HIGH: TomParts = TomParts {
    vco: [0.015e-6, 0.022e-6, 0.0082e-6],
    clip: 0.0056e-6,
    env1: 0.15e-6,
    env2: 0.47e-6,
    env2_pot: 0.47e-6,
    env3: 0.047e-6,
};

// IC18b scales the accent voltage by 1 + R119/R120.
const ACCENT_GAIN: f32 = 1.0 + 10.0e3 / 47.0e3;
/// The accent voltage of an unaccented hit.
const ACCENT_LOW: f32 = 2.6;

// The tune voltage: R113, the tune pot VR10 at mid-travel and R112
// divide the rail.
const R113: f32 = 68.0e3;
const VR10: f32 = 10.0e3;
const R112: f32 = 10.0e3;
const TUNE: f32 = RAIL * (R112 + 0.5 * VR10) / (R113 + VR10 + R112);
// IC14b: R66 feedback over R64 from VDD, R60 from C16 and R61 from C17,
// which the trigger charges through D14 and D15.
const R60: f32 = 2.2e6;
const C16: f32 = 0.1e-6;
const R61: f32 = 470.0e3;
const C17: f32 = 0.047e-6;

// The VCOs: comparator thresholds at half the swing, integrators through
// 47 kΩ, and reset through 10 kΩ while the trigger lasts.
const THRESHOLD: f32 = 0.5 * VSAT;
const R_INTEGRATOR: f32 = 47.0e3;
const R_RESET: f32 = 10.0e3;

// VCO-1 through R102 into Q18's collector; Q18's emitter through R103 to
// D20 and D21, then R104 into Q19. C24 feeds Q18's base through R101.
const R102: f64 = 1.0e3;
const R103: f64 = 10.0e3;
const R104: f64 = 2.2e3;
const R101: f32 = 1.0e6;

// VCO-2 through R105, D22 and D23, R106 into Q20; VCO-3 through R117, D28
// and D27, R116 into Q21.
const SHAPER: Shaper = Shaper {
    r_in: 10.0e3,
    r_out: 2.2e3,
    r_load: 47.0,
    vca: VCA,
};
const R_COLLECTOR: f64 = 47.0;
/// Below this base current a transistor passes nothing that reaches the
/// output.
const OFF: f64 = 1.0e-10;

// The envelopes' base resistors: R107 (ENV1 into Q19), R109 (ENV2 into
// Q20), R110 (ENV3 into Q21). R108 drains ENV1 into ENV2, R100 joins
// ENV2's two capacitors, and the decay pot VR11 with R111 drains C21.
const R107: f32 = 470.0e3;
const R109: f32 = 220.0e3;
const R110: f32 = 470.0e3;
const R108: f32 = 220.0e3;
const R100: f32 = 100.0e3;
const VR11: f32 = 500.0e3;
const R111: f32 = 56.0e3;

// The tom noise stage: the generator through R190 onto C161, the
// Sallen-Key high-pass on IC27b (C51, C52, R191, R192), C53 and R195 into
// Q33 (R199 to ground), whose base C54 feeds through R200; IC27a sums Q33
// through R193 and passes the burst through R194 to VCO-3's VCA.
const NOISE_IN: usize = 0;
const N1: usize = 1;
const N2: usize = 2;
const N3: usize = 3;
const HIGH_OUT: usize = 4;
const N53: usize = 5;
const OP27: usize = 6;
const NOISE_UNKNOWNS: usize = 7;
const NOISE_CAPS: usize = 4;
const STIFF: f32 = 10.0;
const R195: f64 = 1.0e3;
const R199: f64 = 47.0;
const C54: f32 = 0.0047e-6;
const R200: f32 = 100.0e3;
const R193: f64 = 22.0e3;
const R194: f64 = 100.0e3;

/// IC18a's feedback with the level pot at maximum.
const R_LEVEL: f64 = 50.0e3;

/// Scales the output so an unaccented hit peaks near 5 V.
const OUTPUT_GAIN: f32 = 5.215;
/// Below this running peak, some 84 dB under an unaccented hit, the drum
/// has faded out.
const SETTLED: f32 = 3.0e-4;
/// Every envelope below this has let its VCA all but close; VCO-3's drains
/// only through Q21's base, ever more slowly.
const ENV_SETTLED: f32 = 0.65;

const NOISE_MEAN: f32 = 0.5 * RAIL;
const DC_STEP: f32 = 1.0e3;
const DC_ITERATIONS: usize = 4;

/// A tom's circuit state.
#[derive(Clone, Copy, Default)]
struct Tom<L: Lanes = Mono> {
    trigger: Pulse<L>,
    accent: L::S,
    c16: L::S,
    c17: L::S,
    vcos: [Vco<L>; 3],
    c24: L::S,
    env1: L::S,
    env2: L::S,
    env2_pot: L::S,
    env3: L::S,
    c54: L::S,
    noise: MnaState<NOISE_CAPS, L>,
    clip: Clipper<L>,
    vc: [Junction<L>; 4],
    shape: [Junction<L>; 2],
    vbe: [Junction<L>; 5],
    primed: L::SM,
    quiet: Quiet<L>,
}

lanewise!(Tom {
    trigger,
    accent,
    c16,
    c17,
    vcos,
    c24,
    env1,
    env2,
    env2_pot,
    env3,
    c54,
    noise,
    clip,
    vc,
    shape,
    vbe,
    primed,
    quiet,
});

#[derive(Default)]
struct TomShared {
    parts: Option<TomParts>,
    timing: Timing,
    dt: f32,
    noise_rate: f32,
    noise: Mna<NOISE_UNKNOWNS, NOISE_CAPS, 0, 1>,
    noise_rest: MnaState<NOISE_CAPS>,
    reset: [f32; 3],
    /// C16's and C17's fall per sample as they drain into IC14b.
    fall16: f32,
    fall17: f32,
}

#[derive(Clone, Copy, Default)]
struct TomControls<L: Lanes = Mono> {
    /// The VCOs' rate relative to stock.
    rate: L::S,
    /// Conductance draining C21 through the decay pot and R111.
    decay: L::S,
    noise: L::S,
}

lanewise!(TomControls { rate, decay, noise });

impl Fire for Tom {
    fn fire(&mut self, accent: f32, shared: &TomShared) {
        if !self.primed {
            self.noise = shared.noise_rest;
            self.primed = true;
        }
        self.accent = ACCENT_GAIN * accent_volts(ACCENT_LOW, accent);
        self.trigger.fire(TRIG_V, shared.timing.trig_samples);
    }
}

impl<L: Lanes> Circuit<L> for Tom<L> {
    type Shared = TomShared;
    type Controls = TomControls<L>;

    fn settled(&self) -> L::SM {
        !self.trigger.active()
            & self.env1.max(self.env2).max(self.env3).lt(c(ENV_SETTLED))
            & self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &TomShared, ctl: &TomControls<L>) -> L::S {
        let Some(parts) = sh.parts else {
            return c(0.0);
        };
        let dt = sh.dt;
        let trig = self.trigger.next();
        let triggered = trig.gt(c(0.0));

        // The trigger charges C16, C17, C24 and C54 through diodes and the
        // envelopes from the gated accent; each drains through its own
        // resistors meanwhile and after.
        if triggered.any() {
            let top = trig - DIODE_DROP;
            let charge = self.accent - DIODE_DROP;
            let raise = |v: L::S, to: L::S| L::S::select(triggered, v.max(to), v);
            self.c16 = raise(self.c16, top);
            self.c17 = raise(self.c17, top);
            self.c24 = raise(self.c24, top);
            self.c54 = raise(self.c54, top);
            self.env1 = raise(self.env1, charge);
            self.env2_pot = raise(self.env2_pot, charge);
            self.env3 = raise(self.env3, charge);
        }
        self.c16 *= sh.fall16;
        self.c17 *= sh.fall17;

        // The inverters' supply and the VCOs.
        let vdd = c::<L::S>(TUNE);
        let vss = vdd + self.c16 * (100.0e3 / R60) + self.c17 * (100.0e3 / R61);
        for (k, vco) in self.vcos.iter_mut().enumerate() {
            let reset = Vco {
                tri: vco.tri * sh.reset[k],
                rising: L::SM::default(),
            };
            let run = if triggered.all() {
                reset
            } else {
                let rc = R_INTEGRATOR * parts.vco[k];
                let mut run = *vco;
                run.run(vss * ctl.rate / rc, vdd * ctl.rate / rc, dt);
                run
            };
            *vco = Vco::select(triggered, reset, run);
        }

        // Base currents, each drawing on its capacitor.
        let ib18 = VCA.base_current(self.c24, R101, &mut self.vbe[0]);
        let ib19 = VCA.base_current(self.env1, R107, &mut self.vbe[1]);
        let ib20 = VCA.base_current(self.env2, R109, &mut self.vbe[2]);
        let ib21 = VCA.base_current(self.env3, R110, &mut self.vbe[3]);
        self.c24 -= L::narrow(ib18) * dt / parts.clip;
        self.env3 -= L::narrow(ib21) * dt / parts.env3;
        // ENV1 drains into Q19 and through R108 into C23; C21 drains
        // through the decay pot and R100 into C23, which feeds Q20.
        let i108 = (self.env1 - self.env2) / R108;
        let i100 = (self.env2_pot - self.env2) / R100;
        self.env1 -= (i108 + L::narrow(ib19)) * dt / parts.env1;
        self.env2 += (i108 + i100 - L::narrow(ib20)) * dt / parts.env2;
        self.env2_pot -= (i100 + ctl.decay * self.env2_pot) * dt / parts.env2_pot;

        // The tom noise burst.
        let ib33 = VCA.base_current(self.c54, R200, &mut self.vbe[4]);
        self.c54 -= L::narrow(ib33) * dt / C54;
        let mut injected = [c(0.0); NOISE_UNKNOWNS];
        injected[NOISE_IN] = ctl.noise * STIFF;
        let open = L::closed([]);
        let x = sh.noise.open(&self.noise, open, &injected);
        let z = L::wide(sh.noise.impedance::<L>(open, N53, N53));
        let open53 = L::wide(x[N53]);
        let on33 = ib33.gt(c(OFF));
        let (ie33, drawn) = if on33.all() {
            vca_port(&VCA, on33, &mut self.vc[3], open53, z + R195, R199, ib33)
        } else {
            let idle = open53 / (z + R195 + R199);
            if on33.any() {
                let (ie, drawn) =
                    vca_port(&VCA, on33, &mut self.vc[3], open53, z + R195, R199, ib33);
                (
                    L::D::select(on33, ie, c(0.0)),
                    L::D::select(on33, drawn, idle),
                )
            } else {
                (c(0.0), idle)
            }
        };
        sh.noise
            .close(&mut self.noise, open, x, &[(N53, -L::narrow(drawn))]);
        let burst = (ie33 * -R193).clamp(c(-VSAT as f64), c(VSAT as f64));

        // VCO-1 through the clipper into Q19; VCO-2 and VCO-3 through the
        // shapers into Q20 and Q21, the burst joining Q21's collector. A
        // transistor without base current passes nothing.
        let on19 = ib18.gt(c(OFF)) & ib19.gt(c(OFF));
        let ie19 = if on19.any() {
            let into19 = self.clip.solve(on19, L::wide(self.vcos[0].tri), ib18);
            let port = vca_port(
                &VCA,
                on19,
                &mut self.vc[0],
                into19 * R104,
                c(R104),
                R_COLLECTOR,
                ib19,
            );
            L::D::select(on19, port.0, c(0.0))
        } else {
            c(0.0)
        };
        let [s2, s3] = &mut self.shape;
        let on20 = ib20.gt(c(OFF));
        let ie20 = if on20.any() {
            let tri = L::wide(self.vcos[1].tri);
            L::D::select(
                on20,
                SHAPER.solve(on20, s2, &mut self.vc[1], tri, ib20),
                c(0.0),
            )
        } else {
            c(0.0)
        };
        let on21 = ib21.gt(c(OFF));
        let ie21 = if on21.any() {
            let tri = L::wide(self.vcos[2].tri);
            let ie = SHAPER.solve_with(on21, s3, &mut self.vc[2], tri, burst, 1.0 / R194, ib21);
            L::D::select(on21, ie, c(0.0))
        } else {
            c(0.0)
        };

        let out = L::narrow(-((ie19 + ie20 + ie21) * R_LEVEL)).clamp(c(-VSAT), c(VSAT));
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}
