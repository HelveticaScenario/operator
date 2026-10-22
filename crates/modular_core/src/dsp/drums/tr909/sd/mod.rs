//! TR-909 snare drum, simulated from the 909 service notes' voicing board
//! schematic and circuit description. Part names (Q50, C69, …) are the
//! service notes' designators.
//!
//! Two triangle VCOs, each a comparator (IC37a, IC38a) driving 4069
//! inverters into an integrator (IC37b, IC38b), run at a rate set by the
//! inverters' supply: IC35 holds it at ±the tune voltage, and the trigger
//! lifts it for some 20 ms, bending the pitch. The trigger also resets both
//! VCOs so their waveforms start together. Diodes round each triangle, and
//! transistor VCAs Q50 and Q51, each fed its own envelope, pass them to the
//! output stage IC40b. White noise, low-passed on IC40a and high-passed
//! again on IC39a, reaches the output through two more VCAs: Q48, whose
//! envelope's length the tone pot sets, and Q47, the snappy, both summed on
//! IC39b through the snappy pot.

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{DIODE_DROP, RAIL, Shaper, TRIG_V, Timing, VCA, VSAT, accent_volts, vca_port};
use super::noise::Noise;
use crate::dsp::drums::analog::Junction;
use crate::dsp::drums::group::{Channels, Drum, Groups, Run};
use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState};
use crate::dsp::drums::voice::{Circuit, Fire, Pulse, Quiet, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal};

mod module;
mod vco;

use vco::Vco;

pub use module::Sd909;

// IC34b scales the accent voltage by 1 + R241/R240.
const ACCENT_GAIN: f32 = 1.0 + 10.0e3 / 4.7e3;
/// The accent voltage of an unaccented hit.
const ACCENT_LOW: f32 = 1.8;

// The tune voltage: R287, the tune pot VR6 and R284 divide the rail; its
// wiper feeds IC35a through R249, and C66 through R249 and R248 in series.
// The trigger charges C66 through D56 and R247.
const R287: f32 = 100.0e3;
const VR6: f32 = 10.0e3;
const R284: f32 = 10.0e3;
const R249: f32 = 100.0e3;
const R248: f32 = 470.0e3;
const R247: f32 = 47.0e3;
const C66: f32 = 0.01e-6;

// The VCOs: each comparator's thresholds are its swing halved by R260 and
// R262 (IC38a: R272, R271); the inverters drive the integrators through
// R261 into C69 and R264 into C71. Q44 and Q45 empty C69 and C71 through
// R263 and R268 while the trigger lasts.
const THRESHOLD: f32 = 0.5 * VSAT;
const VCO1_RC: f32 = 47.0e3 * 0.01e-6;
const VCO2_RC: f32 = 47.0e3 * 0.0068e-6;
const VCO1_RESET: f32 = 10.0e3 * 0.01e-6;
const VCO2_RESET: f32 = 10.0e3 * 0.0068e-6;

// The shapers into the drum VCAs: VCO-1 through R283, D65 and D66 and R288
// into Q50 (R289 to ground); VCO-2 through R305, D69 and D70 and R304
// into Q51 (R303).
const SHAPER1: Shaper = Shaper {
    r_in: 10.0e3,
    r_out: 1.5e3,
    r_load: 47.0,
};
const SHAPER2: Shaper = Shaper {
    r_in: 10.0e3,
    r_out: 2.2e3,
    r_load: 47.0,
};

// The drum envelopes, each charged through a diode from the gated accent
// for the trigger's length. ENV3: C83 through R291 into C70, then R290
// into Q50's base. ENV2: C82 through R295 into C79, then R293 into Q51's
// base. Q46 and Q49 hold C70 and C79 empty while the trigger lasts.
const C83: f32 = 0.1e-6;
const R291: f32 = 220.0e3;
const C70: f32 = 0.0047e-6;
const R290: f32 = 22.0e3;
const C82: f32 = 0.047e-6;
const R295: f32 = 220.0e3;
const C79: f32 = 0.01e-6;
const R293: f32 = 220.0e3;

// The noise envelopes. ENV4: C67 through D60, drained by the tone pot VR7
// and R254, and by R297 into Q48's base. ENV5: C73 through D64, drained by
// R280 into Q47's base, which Q43 holds off while the trigger lasts.
const C67: f32 = 0.47e-6;
const VR7: f32 = 500.0e3;
const R254: f32 = 100.0e3;
const R297: f32 = 220.0e3;
const C73: f32 = 0.47e-6;
const R280: f32 = 220.0e3;

// The noise network: the generator through R286 and R285, C85 with R302,
// the Sallen-Key low-pass on IC40a (R301, R300, C81, C84); from its output
// C80 and R299 toward Q48, and the Sallen-Key high-pass on IC39a (C76,
// C75, R276, R277) then R278 toward Q47.
const NOISE_IN: usize = 0;
const N1: usize = 1;
const N2: usize = 2;
const N3: usize = 3;
const N4: usize = 4;
const LOW: usize = 5;
const T80: usize = 6;
const H1: usize = 7;
const H2: usize = 8;
const HIGH: usize = 9;
const OP_LOW: usize = 10;
const OP_HIGH: usize = 11;
const NOISE_UNKNOWNS: usize = 12;
const NOISE_CAPS: usize = 6;
/// Conductance driving the noise input to the generator's voltage.
const STIFF: f32 = 10.0;
/// R299 and R278 into Q48's and Q47's collectors, each with 47 Ω to
/// ground.
const R299: f64 = 4.7e3;
const R278: f64 = 2.2e3;
const R_COLLECTOR: f64 = 47.0;
/// Below this base current a transistor passes nothing that reaches the
/// output.
const OFF: f64 = 1.0e-10;

// The outputs: Q47 and Q48 into IC39b, its feedback the snappy pot VR9;
// IC39b through C78 and R294 into IC40b, whose feedback is the level pot
// at maximum, with Q50 and Q51.
const VR9: f64 = 10.0e3;
const R294: f32 = 47.0e3;
const C78: f32 = 0.47e-6;
const R_LEVEL: f64 = 50.0e3;

/// Scales the output so an unaccented hit peaks near 5 V.
const OUTPUT_GAIN: f32 = 1.116;
/// Below this running peak, some 64 dB under an unaccented hit, the drum
/// has faded out: its envelopes drain only through the VCAs' bases, ever
/// more slowly, and pass a faint leak for seconds more.
const SETTLED: f32 = 3.0e-3;
/// Every envelope below this has let its VCA all but close.
const ENV_SETTLED: f32 = 0.65;

const NOISE_MEAN: f32 = 0.5 * RAIL;
const DC_STEP: f32 = 1.0e3;
const DC_ITERATIONS: usize = 4;

fn noise_network(dt: f32) -> Mna<NOISE_UNKNOWNS, NOISE_CAPS, 0, 1> {
    let mut b = MnaBuilder::<NOISE_UNKNOWNS>::default();
    b.resistor(NOISE_IN, GROUND, 1.0 / STIFF)
        .resistor(NOISE_IN, N1, 10.0e3)
        .resistor(N1, GROUND, 3.3e3)
        .capacitor(N1, N2, 0.015e-6)
        .resistor(N2, GROUND, 220.0e3)
        .resistor(N2, N3, 22.0e3)
        .resistor(N3, N4, 22.0e3)
        .capacitor(N3, LOW, 0.0033e-6)
        .capacitor(N4, GROUND, 220.0e-12)
        .opamp(N4, LOW, LOW, OP_LOW)
        .capacitor(LOW, T80, 0.022e-6)
        .capacitor(LOW, H1, 0.0022e-6)
        .capacitor(H1, H2, 0.0022e-6)
        .resistor(H1, HIGH, 6.8e3)
        .resistor(H2, GROUND, 47.0e3)
        .opamp(H2, HIGH, HIGH, OP_HIGH);
    b.build(dt, [])
}

/// The snare's circuit state.
#[derive(Clone, Copy, Default)]
struct Snare<L: Lanes = Mono> {
    trigger: Pulse<L>,
    accent: L::S,
    c66: L::S,
    vco1: Vco<L>,
    vco2: Vco<L>,
    c83: L::S,
    c70: L::S,
    c82: L::S,
    c79: L::S,
    c67: L::S,
    c73: L::S,
    noise: MnaState<NOISE_CAPS, L>,
    c78: L::S,
    /// Shaper and collector nodes, and base-emitter voltages for Newton.
    d1: Junction<L>,
    vc50: Junction<L>,
    d2: Junction<L>,
    vc51: Junction<L>,
    vc48: Junction<L>,
    vc47: Junction<L>,
    vbe: [Junction<L>; 4],
    primed: L::SM,
    quiet: Quiet<L>,
}

lanewise!(Snare {
    trigger,
    accent,
    c66,
    vco1,
    vco2,
    c83,
    c70,
    c82,
    c79,
    c67,
    c73,
    noise,
    c78,
    d1,
    vc50,
    d2,
    vc51,
    vc48,
    vc47,
    vbe,
    primed,
    quiet,
});

#[derive(Default)]
struct SnareShared {
    timing: Timing,
    dt: f32,
    noise_rate: f32,
    noise: Mna<NOISE_UNKNOWNS, NOISE_CAPS, 0, 1>,
    noise_rest: MnaState<NOISE_CAPS>,
    reset1: f32,
    reset2: f32,
    /// C66's per-sample approach to its target while the trigger charges it
    /// through R247, and through the tune divider alone.
    c66_charge: f32,
    c66_bleed: f32,
}

#[derive(Clone, Copy, Default)]
struct SnareControls<L: Lanes = Mono> {
    /// The tune pot's wiper voltage.
    tune: L::S,
    /// C67's fall per sample as the tone pot and R254 drain it.
    tone_fall: L::S,
    /// The snappy pot's resistance.
    snappy: L::D,
    noise: L::S,
}

lanewise!(SnareControls {
    tune,
    tone_fall,
    snappy,
    noise
});

/// The snare at every width.
struct SnareDrum;

impl Drum for SnareDrum {
    type Shared = SnareShared;
    type Circuit<L: Lanes> = Snare<L>;
    type Controls<L: Lanes> = SnareControls<L>;
    type Sources<L: Lanes> = ();
    const QUAD: bool = true;
    const PAIR: bool = true;
}

impl Fire for Snare {
    fn fire(&mut self, accent: f32, shared: &SnareShared) {
        if !self.primed {
            self.noise = shared.noise_rest;
            self.primed = true;
        }
        self.accent = (ACCENT_GAIN * accent_volts(ACCENT_LOW, accent)).min(VSAT);
        self.trigger.fire(TRIG_V, shared.timing.trig_samples);
    }
}

impl<L: Lanes> Circuit<L> for Snare<L> {
    type Shared = SnareShared;
    type Controls = SnareControls<L>;

    fn settled(&self) -> L::SM {
        !self.trigger.active()
            & self
                .c83
                .max(self.c82)
                .max(self.c67)
                .max(self.c73)
                .lt(c(ENV_SETTLED))
            & self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &SnareShared, ctl: &SnareControls<L>) -> L::S {
        let dt = sh.dt;
        let trig = self.trigger.next();
        let triggered = trig.gt(c(0.0));

        // The inverters' supply: the tune voltage, lifted by C66.
        let charging = triggered & (trig - DIODE_DROP).gt(self.c66);
        let pulse = L::S::select(charging, c(1.0 / R247), c(0.0));
        let mul = L::S::select(charging, c(sh.c66_charge), c(sh.c66_bleed));
        let g_tune = 1.0 / (R248 + R249);
        let total = pulse + g_tune;
        let settle = (pulse * (trig - DIODE_DROP) + ctl.tune * g_tune) / total;
        self.c66 = settle + (self.c66 - settle) * mul;
        let vdd = (ctl.tune * R248 + self.c66 * R249) / (R248 + R249);

        self.vco1.advance(triggered, vdd / VCO1_RC, dt, sh.reset1);
        self.vco2.advance(triggered, vdd / VCO2_RC, dt, sh.reset2);

        // The envelopes: charged from the gated accent while the trigger
        // lasts, the drum VCAs' second stages held empty meanwhile.
        if triggered.any() {
            let charge = self.accent - DIODE_DROP;
            for v in [&mut self.c83, &mut self.c82, &mut self.c67, &mut self.c73] {
                *v = L::S::select(triggered, v.max(charge), *v);
            }
            self.c70 = L::S::select(triggered, c(0.0), self.c70);
            self.c79 = L::S::select(triggered, c(0.0), self.c79);
        }
        let idle = !L::wide_mask(triggered);
        let mut ib = [c::<L::D>(0.0); 4];
        // ENV3: C83 into C70 through R291, C70 into Q50's base.
        ib[0] = VCA.base_current_where(idle, self.c70, R290, &mut self.vbe[0]);
        let i291 = (self.c83 - self.c70) / R291;
        self.c83 -= i291 * dt / C83;
        self.c70 += (i291 - L::narrow(ib[0])) * dt / C70;
        // ENV2: C82 into C79 through R295, C79 into Q51's base.
        ib[1] = VCA.base_current_where(idle, self.c79, R293, &mut self.vbe[1]);
        let i295 = (self.c82 - self.c79) / R295;
        self.c82 -= i295 * dt / C82;
        self.c79 += (i295 - L::narrow(ib[1])) * dt / C79;
        // ENV4: the tone pot drains C67; R297 feeds Q48's base.
        ib[2] = VCA.base_current(self.c67, R297, &mut self.vbe[2]);
        self.c67 *= ctl.tone_fall;
        self.c67 -= L::narrow(ib[2]) * dt / C67;
        // ENV5: R280 feeds Q47's base, which Q43 holds off meanwhile.
        ib[3] = VCA.base_current_where(idle, self.c73, R280, &mut self.vbe[3]);
        self.c73 -= L::narrow(ib[3]) * dt / C73;

        // The noise network and its two VCAs.
        let mut injected = [c(0.0); NOISE_UNKNOWNS];
        injected[NOISE_IN] = ctl.noise * STIFF;
        let open = L::closed([]);
        let x = sh.noise.open(&self.noise, open, &injected);
        let z = L::wide(sh.noise.impedance::<L>(open, T80, T80));
        let open80 = L::wide(x[T80]);
        let on48 = ib[2].gt(c(OFF));
        let (ie48, i48) = if on48.all() {
            vca_port(on48, &mut self.vc48, open80, z + R299, R_COLLECTOR, ib[2])
        } else {
            let idle = open80 / (z + R299 + R_COLLECTOR);
            if on48.any() {
                let (ie, i) = vca_port(on48, &mut self.vc48, open80, z + R299, R_COLLECTOR, ib[2]);
                (L::D::select(on48, ie, c(0.0)), L::D::select(on48, i, idle))
            } else {
                (c(0.0), idle)
            }
        };
        // IC39a's output drives R278 without moving.
        let on47 = ib[3].gt(c(OFF));
        let ie47 = if on47.any() {
            let high = L::wide(x[HIGH]);
            let ie = vca_port(on47, &mut self.vc47, high, c(R278), R_COLLECTOR, ib[3]).0;
            L::D::select(on47, ie, c(0.0))
        } else {
            c(0.0)
        };
        sh.noise
            .close(&mut self.noise, open, x, &[(T80, -L::narrow(i48))]);

        let snappy = L::narrow(-((ie47 + ie48) * ctl.snappy)).clamp(c(-VSAT), c(VSAT));
        // C78 couples IC39b into IC40b through R294.
        let i294 = (snappy - self.c78) / R294;
        self.c78 += i294 * dt / C78;

        let on50 = ib[0].gt(c(OFF));
        let ie50 = if on50.any() {
            let tri = L::wide(self.vco1.tri);
            let ie = SHAPER1.solve(on50, &mut self.d1, &mut self.vc50, tri, ib[0]);
            L::D::select(on50, ie, c(0.0))
        } else {
            c(0.0)
        };
        let on51 = ib[1].gt(c(OFF));
        let ie51 = if on51.any() {
            let tri = L::wide(self.vco2.tri);
            let ie = SHAPER2.solve(on51, &mut self.d2, &mut self.vc51, tri, ib[1]);
            L::D::select(on51, ie, c(0.0))
        } else {
            c(0.0)
        };
        let out = L::narrow(-((ie50 + ie51 + L::wide(i294)) * R_LEVEL)).clamp(c(-VSAT), c(VSAT));
        self.quiet.track(out, sh.timing.quiet_fall);
        out
    }
}
