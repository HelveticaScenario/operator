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
    DIODE_DROP, Noise, RAIL, Shaper, TRIG_V, Timing, VCA, VSAT, accent_volts, vca_port,
};
use crate::dsp::drums::analog::VT;
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState};
use crate::dsp::drums::voice::{Circuit, Pulse, Quiet, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

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

// The tune voltage: R113, the tune pot VR10 and R112 divide the rail.
const R113: f32 = 68.0e3;
const VR10: f32 = 10.0e3;
const R112: f32 = 10.0e3;
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
const DIODE_IS: f64 = 2.52e-9;
const DIODE_NVT: f64 = 1.752 * VT as f64;

// VCO-2 through R105, D22 and D23, R106 into Q20; VCO-3 through R117, D28
// and D27, R116 into Q21.
const SHAPER: Shaper = Shaper {
    r_in: 10.0e3,
    r_out: 2.2e3,
    r_load: 47.0,
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
const OUTPUT_GAIN: f32 = 6.3;
/// Below this running peak, some 84 dB under an unaccented hit, the drum
/// has faded out.
const SETTLED: f32 = 3.0e-4;
/// Every envelope below this has let its VCA all but close; VCO-3's drains
/// only through Q21's base, ever more slowly.
const ENV_SETTLED: f32 = 0.65;

const DEFAULT_TUNE: f32 = 2.5;
const DEFAULT_DECAY: f32 = 2.5;

const NOISE_MEAN: f32 = 0.5 * RAIL;
const DC_STEP: f32 = 1.0e3;
const DC_ITERATIONS: usize = 4;

fn noise_network(dt: f32) -> Mna<NOISE_UNKNOWNS, NOISE_CAPS, 0, 1> {
    let mut b = MnaBuilder::<NOISE_UNKNOWNS>::default();
    b.resistor(NOISE_IN, GROUND, 1.0 / STIFF)
        .resistor(NOISE_IN, N1, 22.0e3)
        .capacitor(N1, GROUND, 0.0047e-6)
        .capacitor(N1, N2, 0.0047e-6)
        .capacitor(N2, N3, 0.0047e-6)
        .resistor(N2, HIGH_OUT, 10.0e3)
        .resistor(N3, GROUND, 220.0e3)
        .opamp(N3, HIGH_OUT, HIGH_OUT, OP27)
        .capacitor(HIGH_OUT, N53, 0.047e-6);
    b.build(dt, [])
}

/// A tom's circuit state.
#[derive(Clone, Copy, Default)]
struct Tom {
    trigger: Pulse,
    accent: f32,
    c16: f32,
    c17: f32,
    vcos: [Vco; 3],
    c24: f32,
    env1: f32,
    env2: f32,
    env2_pot: f32,
    env3: f32,
    c54: f32,
    noise: MnaState<NOISE_CAPS>,
    clip: [f64; 3],
    vc: [f64; 4],
    shape: [f64; 2],
    vbe: [f64; 5],
    primed: bool,
    quiet: Quiet,
}

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

struct TomControls {
    tune: f32,
    /// Conductance draining C21 through the decay pot and R111.
    decay: f32,
    noise: f32,
}

impl Circuit for Tom {
    type Shared = TomShared;
    type Controls = TomControls;

    fn fire(&mut self, accent: f32, shared: &TomShared) {
        if !self.primed {
            self.noise = shared.noise_rest;
            self.primed = true;
        }
        self.accent = ACCENT_GAIN * accent_volts(ACCENT_LOW, accent);
        self.trigger.fire(TRIG_V, shared.timing.trig_samples);
    }

    fn settled(&self) -> bool {
        !self.trigger.active()
            && self.env1.max(self.env2).max(self.env3) < ENV_SETTLED
            && self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &TomShared, c: &TomControls) -> f32 {
        let Some(parts) = sh.parts else {
            return 0.0;
        };
        let dt = sh.dt;
        let trig = self.trigger.next();
        let triggered = trig > 0.0;

        // The trigger charges C16, C17, C24 and C54 through diodes and the
        // envelopes from the gated accent; each drains through its own
        // resistors meanwhile and after.
        let top = trig - DIODE_DROP;
        let charge = self.accent - DIODE_DROP;
        if triggered {
            self.c16 = self.c16.max(top);
            self.c17 = self.c17.max(top);
            self.c24 = self.c24.max(top);
            self.c54 = self.c54.max(top);
            self.env1 = self.env1.max(charge);
            self.env2_pot = self.env2_pot.max(charge);
            self.env3 = self.env3.max(charge);
        }
        self.c16 *= sh.fall16;
        self.c17 *= sh.fall17;

        // The inverters' supply and the VCOs.
        let vdd = c.tune;
        let vss = vdd + (100.0e3 / R60) * self.c16 + (100.0e3 / R61) * self.c17;
        for (k, vco) in self.vcos.iter_mut().enumerate() {
            if triggered {
                vco.tri *= sh.reset[k];
                vco.rising = false;
            } else {
                let rc = R_INTEGRATOR * parts.vco[k];
                vco.run(vss / rc, vdd / rc, dt);
            }
        }

        // Base currents, each drawing on its capacitor.
        let ib18 = VCA.base_current(self.c24, R101, &mut self.vbe[0]);
        let ib19 = VCA.base_current(self.env1, R107, &mut self.vbe[1]);
        let ib20 = VCA.base_current(self.env2, R109, &mut self.vbe[2]);
        let ib21 = VCA.base_current(self.env3, R110, &mut self.vbe[3]);
        self.c24 -= ib18 as f32 * dt / parts.clip;
        self.env3 -= ib21 as f32 * dt / parts.env3;
        // ENV1 drains into Q19 and through R108 into C23; C21 drains
        // through the decay pot and R100 into C23, which feeds Q20.
        let i108 = (self.env1 - self.env2) / R108;
        let i100 = (self.env2_pot - self.env2) / R100;
        self.env1 -= (i108 + ib19 as f32) * dt / parts.env1;
        self.env2 += (i108 + i100 - ib20 as f32) * dt / parts.env2;
        self.env2_pot -= (i100 + c.decay * self.env2_pot) * dt / parts.env2_pot;

        // The tom noise burst.
        let ib33 = VCA.base_current(self.c54, R200, &mut self.vbe[4]);
        self.c54 -= ib33 as f32 * dt / C54;
        let mut injected = [0.0f32; NOISE_UNKNOWNS];
        injected[NOISE_IN] = c.noise * STIFF;
        let x = sh.noise.open(&self.noise, 0, &injected);
        let z = sh.noise.impedance(0, N53, N53) as f64;
        let open53 = x[N53] as f64;
        let (ie33, drawn) = if ib33 > OFF {
            vca_port(&mut self.vc[3], open53, z + R195, R199, ib33)
        } else {
            (0.0, open53 / (z + R195 + R199))
        };
        sh.noise
            .close(&mut self.noise, 0, x, &[(N53, -drawn as f32)]);
        let burst = (-R193 * ie33).clamp(-VSAT as f64, VSAT as f64);

        // VCO-1 through the clipper into Q19; VCO-2 and VCO-3 through the
        // shapers into Q20 and Q21, the burst joining Q21's collector. A
        // transistor without base current passes nothing.
        let ie19 = if ib18 > OFF && ib19 > OFF {
            let [a, b, d] = &mut self.clip;
            let into19 = clipper(a, b, d, self.vcos[0].tri as f64, ib18);
            vca_port(&mut self.vc[0], into19 * R104, R104, R_COLLECTOR, ib19).0
        } else {
            0.0
        };
        let [s2, s3] = &mut self.shape;
        let ie20 = if ib20 > OFF {
            SHAPER.solve(s2, &mut self.vc[1], self.vcos[1].tri as f64, ib20)
        } else {
            0.0
        };
        let ie21 = if ib21 > OFF {
            let open21 = self.vcos[2].tri as f64;
            shaped_with_burst(s3, &mut self.vc[2], open21, burst, ib21)
        } else {
            0.0
        };

        let out = (-(R_LEVEL * (ie19 + ie20 + ie21)) as f32).clamp(-VSAT, VSAT);
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}

#[derive(Default)]
struct TomState {
    shared: TomShared,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Tom>,
    noise: Noise,
}

/// Sets up a tom of `parts` at `sample_rate`.
fn init(state: &mut TomState, parts: TomParts, sample_rate: f32) {
    let dt = 1.0 / sample_rate;
    let sh = &mut state.shared;
    sh.parts = Some(parts);
    sh.timing = Timing::new(sample_rate);
    sh.dt = dt;
    sh.noise_rate = Noise::rate(sample_rate);
    // The noise network at DC: discretised at a step long enough that its
    // capacitors carry no current.
    let dc = noise_network(DC_STEP);
    let mut injected = [0.0f32; NOISE_UNKNOWNS];
    injected[NOISE_IN] = NOISE_MEAN * STIFF;
    let mut rest = MnaState::default();
    for _ in 0..DC_ITERATIONS {
        dc.step(&mut rest, 0, &injected);
    }
    sh.noise_rest = rest;
    sh.noise = noise_network(dt);
    sh.reset = parts.vco.map(|c| (-dt / (R_RESET * c)).exp());
    sh.fall16 = (-dt / (R60 * C16)).exp();
    sh.fall17 = (-dt / (R61 * C17)).exp();
}

/// The inputs every tom reads.
struct TomInputs<'a> {
    trigger: &'a PolySignal,
    accent: &'a Option<PolySignal>,
    tune: &'a Option<PolySignal>,
    decay: &'a Option<PolySignal>,
    choke: bool,
}

/// Runs one sample for every channel of a tom.
fn update(state: &TomState, p: TomInputs, channels: &mut [Channel], out: &mut PolyOutput) {
    let sh = &state.shared;
    for (ch, cs) in channels.iter_mut().enumerate() {
        let knob =
            |s: &Option<PolySignal>, default: f32| s.value_or(ch, default).clamp(0.0, 5.0) / 5.0;
        let tune = knob(p.tune, DEFAULT_TUNE);
        let controls = TomControls {
            tune: RAIL * (R112 + VR10 * tune) / (R113 + VR10 + R112),
            decay: 1.0 / (R111 + VR11 * knob(p.decay, DEFAULT_DECAY)),
            noise: cs.noise.next(sh.noise_rate),
        };
        let v = cs.voices.process(
            p.trigger.get_value(ch),
            p.accent,
            ch,
            p.choke,
            sh.timing.choke_samples,
            sh,
            &controls,
        );
        out.set(ch, v * OUTPUT_GAIN);
    }
}
