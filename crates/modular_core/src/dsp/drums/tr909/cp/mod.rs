//! TR-909 hand clap, simulated from the 909 service notes' voicing board
//! schematic. Part names (Q37, C59, …) are the service notes' designators.
//!
//! The noise generator through IC26b's band-pass feeds two VCAs. The
//! first, a BA662 OTA, is gated by the clap envelope in
//! [`crate::dsp::drums::clap`], the same circuit as the 808's. The second,
//! transistor Q37, passes a low-passed share of the noise as the "reverb",
//! its base fed from C61, which fills through R233 from C59 after the
//! trigger charges C59 and empties more slowly.

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{DIODE_DROP, RAIL, TRIG_V, VCA, VSAT, accent_volts, vca_port};
use super::noise::Noise;
use crate::dsp::drums::analog::Junction;
use crate::dsp::drums::clap::{Envelope, EnvelopeNetwork, ota_current};
use crate::dsp::drums::group::{ChannelInputs, Drum, Groups, Run};
use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState};
use crate::dsp::drums::voice::{Circuit, Fire, Level, Pulse, Quiet, Timing, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal};

// IC26a scales the accent voltage by 1 + R212/R206.
const ACCENT_GAIN: f32 = 1.0 + 10.0e3 / 4.7e3;
/// The accent voltage of an unaccented hit.
const ACCENT_LOW: f32 = 2.1;

// R213 and R218 divide the gated accent against −15 V into IC29c.
const R213: f32 = 22.0e3;
const R218: f32 = 22.0e3;

/// The offset trimmer feeds Q38's base through R494 against R230; it is
/// set, as for the 808, so the OTA nearly closes between claps.
const TM_WIPER: f32 = -0.58 * RAIL;
const R494: f32 = 47.0e3;
const R230: f32 = 2.2e3;

// The noise network: the generator driven in stiffly, IC26b's
// multiple-feedback band-pass (R208, R209, C42, C43, R207); from its
// output C56, R202 and R201 into the OTA, and C55, R204 with C57, and
// R205 into Q37's collector with R203 to ground.
const NOISE_IN: usize = 0;
const X26: usize = 1;
const INV26: usize = 2;
const OUT26: usize = 3;
const Y56: usize = 4;
const OTA_IN: usize = 5;
const A55: usize = 6;
const B57: usize = 7;
const Q37C: usize = 8;
const OP26: usize = 9;
const NOISE_UNKNOWNS: usize = 10;
const NOISE_CAPS: usize = 5;
const STIFF: f32 = 10.0;

// The reverb envelope: C59, charged from the gated accent through D32,
// fills C61 through R233; C61 feeds Q37's base through R234, and D33 holds
// it above a diode drop below ground.
const C59: f32 = 0.1e-6;
const R233: f32 = 470.0e3;
const C61: f32 = 0.1e-6;
const R234: f32 = 470.0e3;

// The output: the OTA's current output loaded by R237 and coupled through
// C58, and Q37's emitter, into IC29a, whose feedback is the level pot.
const OTA_OUT: usize = 0;
const M29: usize = 1;
const OUT29: usize = 2;
const OP29: usize = 3;
const OUT_UNKNOWNS: usize = 4;
const OUT_CAPS: usize = 2;
/// The level pot, VR20 (50 kΩ), turned down to 8.4 kΩ, where even a fully
/// accented clap stays clear of IC29a's swing.
const LEVEL: f32 = 8.4e3;
const R237: f32 = 82.0e3;
/// The OTA's output stays within this of ground.
const OTA_COMPLIANCE: f32 = 13.5;

/// Scales the output so an unaccented clap peaks near 5 V.
const OUTPUT_GAIN: f32 = 1.275;
/// Below this running peak, some 84 dB under an unaccented hit, the drum
/// has faded out.
const SETTLED: f32 = 3.0e-4;
/// The envelope below this has let its VCA all but close; it drains only
/// through the VCA's base, ever more slowly.
const ENV_SETTLED: f32 = 0.65;

const NOISE_MEAN: f32 = 0.5 * RAIL;
const DC_STEP: f32 = 1.0e3;
const DC_ITERATIONS: usize = 20;

fn noise_network(dt: f32) -> Mna<NOISE_UNKNOWNS, NOISE_CAPS, 0, 1> {
    let mut b = MnaBuilder::<NOISE_UNKNOWNS>::default();
    b.resistor(NOISE_IN, GROUND, 1.0 / STIFF)
        .resistor(NOISE_IN, X26, 47.0e3)
        .resistor(X26, GROUND, 10.0e3)
        .capacitor(X26, INV26, 0.0047e-6)
        .capacitor(X26, OUT26, 0.0047e-6)
        .resistor(OUT26, INV26, 150.0e3)
        .opamp(GROUND, INV26, OUT26, OP26)
        .capacitor(OUT26, Y56, 0.022e-6)
        .resistor(Y56, OTA_IN, 39.0e3)
        .resistor(OTA_IN, GROUND, 1.0e3)
        .capacitor(OUT26, A55, 0.47e-6)
        .resistor(A55, B57, 1.0e3)
        .capacitor(B57, GROUND, 0.47e-6)
        .resistor(B57, Q37C, 3.3e3)
        .resistor(Q37C, GROUND, 47.0);
    b.build(dt, [])
}

fn output_network(dt: f32) -> Mna<OUT_UNKNOWNS, OUT_CAPS, 0, 1> {
    let mut b = MnaBuilder::<OUT_UNKNOWNS>::default();
    b.resistor(OTA_OUT, GROUND, R237)
        .capacitor(OTA_OUT, M29, 0.001e-6)
        .resistor(M29, OUT29, LEVEL)
        .capacitor(M29, OUT29, 220.0e-12)
        .opamp(GROUND, M29, OUT29, OP29);
    b.build(dt, [])
}

/// The clap's circuit state.
#[derive(Clone, Copy, Default)]
struct Clap<L: Lanes = Mono> {
    trigger: Pulse<L>,
    accent: L::S,
    level: Level<L>,
    envelope: Envelope<L>,
    noise: MnaState<NOISE_CAPS, L>,
    output: MnaState<OUT_CAPS, L>,
    c59: L::S,
    c61: L::S,
    vc37: Junction<L>,
    vbe37: Junction<L>,
    primed: L::SM,
    quiet: Quiet<L>,
}

lanewise!(Clap {
    trigger,
    accent,
    level,
    envelope,
    noise,
    output,
    c59,
    c61,
    vc37,
    vbe37,
    primed,
    quiet,
});

#[derive(Default)]
struct ClapShared {
    timing: Timing,
    dt: f32,
    noise_rate: f32,
    noise: Mna<NOISE_UNKNOWNS, NOISE_CAPS, 0, 1>,
    envelope: EnvelopeNetwork,
    output: Mna<OUT_UNKNOWNS, OUT_CAPS, 0, 1>,
    /// The circuit at rest.
    rest: Clap,
}

/// The clap at every width.
struct ClapDrum;

impl Drum for ClapDrum {
    type Shared = ClapShared;
    type Circuit<L: Lanes> = Clap<L>;
    /// The noise generator's voltage.
    type Controls<L: Lanes> = L::S;
    type Sources<L: Lanes> = ();
    const RUNS_IN_QUADS: bool = true;
    const RUNS_IN_PAIRS: bool = true;
}

impl<L: Lanes> Clap<L> {
    /// Advances one sample with the generator's voltage at `noise`.
    #[inline]
    fn advance(&mut self, sh: &ClapShared, noise: L::S) -> L::S {
        let dt = sh.dt;
        let triggered = self.trigger.next().gt(c(0.0));

        // The reverb envelope.
        if triggered.any() {
            let top = self.accent - DIODE_DROP;
            self.c59 = L::S::select(triggered, self.c59.max(top), self.c59);
        }
        let ib37 = VCA.base_current(self.c61, R234, &mut self.vbe37);
        let i233 = (self.c59 - self.c61) / R233;
        self.c59 -= i233 * dt / C59;
        self.c61 = (self.c61 + (i233 - L::narrow(ib37)) * dt / C61).max(c(-DIODE_DROP));

        let mut injected = [c(0.0); NOISE_UNKNOWNS];
        injected[NOISE_IN] = noise * STIFF;
        let open = L::closed([]);
        let x = sh.noise.open(&self.noise, open, &injected);
        let z = sh.noise.impedance::<Mono>(0, Q37C, Q37C) as f64;
        // Q37's collector against the network's Thevenin equivalent: the
        // network already holds R203, so the port loads nothing more.
        let (ie37, drawn) = vca_port(
            &VCA,
            !L::DM::default(),
            &mut self.vc37,
            L::wide(x[Q37C]),
            c(z),
            f64::INFINITY,
            ib37,
        );
        let x = sh
            .noise
            .close(&mut self.noise, open, x, &[(Q37C, -L::narrow(drawn))]);

        // The clap envelope and the OTA.
        let m = L::S::select(
            triggered,
            (self.accent * R218 - RAIL * R213) / (R213 + R218),
            c(-RAIL),
        );
        let g = 1.0 / R494 + 1.0 / R230;
        let b71 = (self.envelope.q71_base() + TM_WIPER / R494) / g;
        let iabc = self.envelope.step(&sh.envelope, m, b71);
        let current = ota_current::<L>(iabc, x[OTA_IN]);

        // The OTA's current, held within its output's compliance.
        let mut injected = [c(0.0); OUT_UNKNOWNS];
        injected[M29] = L::narrow(ie37);
        let x = sh.output.open(&self.output, open, &injected);
        let z = sh.output.impedance::<Mono>(0, OTA_OUT, OTA_OUT);
        let node = (x[OTA_OUT] + current * z).clamp(c(-OTA_COMPLIANCE), c(OTA_COMPLIANCE));
        let current = (node - x[OTA_OUT]) / z;
        let x = sh
            .output
            .close(&mut self.output, open, x, &[(OTA_OUT, current)]);
        x[OUT29].clamp(c(-VSAT), c(VSAT))
    }
}

impl Fire for Clap {
    fn fire(&mut self, accent: f32, shared: &ClapShared) {
        let sounding = self.primed && !self.settled();
        if !self.primed {
            *self = shared.rest;
            self.primed = true;
        }
        self.accent = (ACCENT_GAIN * accent_volts(ACCENT_LOW, accent)).min(VSAT);
        self.level
            .set(1.0 + accent, sounding, shared.timing.fade_samples);
        self.trigger.fire(TRIG_V, shared.timing.trigger_samples);
    }
}

impl<L: Lanes> Circuit<L> for Clap<L> {
    type Shared = ClapShared;
    type Controls = L::S;

    fn settled(&self) -> L::SM {
        !self.trigger.active()
            & !self.envelope.window_open()
            & self.c61.lt(c(ENV_SETTLED))
            & self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &ClapShared, &noise: &L::S) -> L::S {
        let out = self.advance(sh, noise) * self.level.next();
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}

mod module;

pub use module::Cp909;
