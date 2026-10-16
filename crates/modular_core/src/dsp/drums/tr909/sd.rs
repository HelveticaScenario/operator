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

use super::circuit::{
    DIODE_DROP, Noise, RAIL, Shaper, TRIG_V, Timing, VCA, VSAT, accent_volts, vca_port,
};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState};
use crate::dsp::drums::voice::{Circuit, Pulse, Quiet, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

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
const OUTPUT_GAIN: f32 = 1.0;
/// Below this running peak, some 64 dB under an unaccented hit, the drum
/// has faded out: its envelopes drain only through the VCAs' bases, ever
/// more slowly, and pass a faint leak for seconds more.
const SETTLED: f32 = 3.0e-3;
/// Every envelope below this has let its VCA all but close.
const ENV_SETTLED: f32 = 0.65;

const DEFAULT_TUNE: f32 = 2.5;
const DEFAULT_TONE: f32 = 2.5;
const DEFAULT_SNAPPY: f32 = 2.5;

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

/// One triangle VCO: the integrator's voltage and the comparator's state.
#[derive(Clone, Copy, Default)]
struct Vco {
    tri: f32,
    rising: bool,
}

impl Vco {
    /// Advances `dt` at slope `slope` V/s, reflecting off the comparator's
    /// thresholds.
    #[inline]
    fn run(&mut self, slope: f32, dt: f32) {
        let mut travel = slope * dt;
        while travel > 0.0 {
            let room = if self.rising {
                THRESHOLD - self.tri
            } else {
                self.tri + THRESHOLD
            };
            if travel < room {
                self.tri += if self.rising { travel } else { -travel };
                break;
            }
            self.tri = if self.rising { THRESHOLD } else { -THRESHOLD };
            travel -= room.max(0.0);
            self.rising = !self.rising;
        }
    }

    /// Holds the integrator toward zero with `coeff` per sample and the
    /// comparator low, so the triangle starts downward.
    #[inline]
    fn reset(&mut self, coeff: f32) {
        self.tri *= coeff;
        self.rising = false;
    }
}

/// The snare's circuit state.
#[derive(Clone, Copy, Default)]
struct Snare {
    trigger: Pulse,
    accent: f32,
    c66: f32,
    vco1: Vco,
    vco2: Vco,
    c83: f32,
    c70: f32,
    c82: f32,
    c79: f32,
    c67: f32,
    c73: f32,
    noise: MnaState<NOISE_CAPS>,
    c78: f32,
    /// Shaper and collector nodes, and base-emitter voltages for Newton.
    d1: f64,
    vc50: f64,
    d2: f64,
    vc51: f64,
    vc48: f64,
    vc47: f64,
    vbe: [f64; 4],
    primed: bool,
    quiet: Quiet,
}

#[derive(Default)]
struct SnareShared {
    timing: Timing,
    dt: f32,
    noise_rate: f32,
    noise: Mna<NOISE_UNKNOWNS, NOISE_CAPS, 0, 1>,
    noise_rest: MnaState<NOISE_CAPS>,
    reset1: f32,
    reset2: f32,
}

struct SnareControls {
    /// The tune pot's wiper voltage.
    tune: f32,
    /// Conductance draining C67 through the tone pot and R254.
    tone: f32,
    /// The snappy pot's resistance.
    snappy: f64,
    noise: f32,
}

impl Circuit for Snare {
    type Shared = SnareShared;
    type Controls = SnareControls;

    fn fire(&mut self, accent: f32, shared: &SnareShared) {
        if !self.primed {
            self.noise = shared.noise_rest;
            self.primed = true;
        }
        self.accent = (ACCENT_GAIN * accent_volts(ACCENT_LOW, accent)).min(VSAT);
        self.trigger.fire(TRIG_V, shared.timing.trig_samples);
    }

    fn settled(&self) -> bool {
        !self.trigger.active()
            && self.c83.max(self.c82).max(self.c67).max(self.c73) < ENV_SETTLED
            && self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &SnareShared, c: &SnareControls) -> f32 {
        let dt = sh.dt;
        let trig = self.trigger.next();
        let triggered = trig > 0.0;

        // The inverters' supply: the tune voltage, lifted by C66.
        let pulse = if triggered && trig - DIODE_DROP > self.c66 {
            1.0 / R247
        } else {
            0.0
        };
        let g_tune = 1.0 / (R248 + R249);
        let total = pulse + g_tune;
        let settle = (pulse * (trig - DIODE_DROP) + g_tune * c.tune) / total;
        self.c66 = settle + (self.c66 - settle) * (-total * dt / C66).exp();
        let vdd = (c.tune * R248 + self.c66 * R249) / (R248 + R249);

        if triggered {
            self.vco1.reset(sh.reset1);
            self.vco2.reset(sh.reset2);
        } else {
            self.vco1.run(vdd / VCO1_RC, dt);
            self.vco2.run(vdd / VCO2_RC, dt);
        }

        // The envelopes: charged from the gated accent while the trigger
        // lasts, the drum VCAs' second stages held empty meanwhile.
        let charge = self.accent - DIODE_DROP;
        if triggered {
            for v in [&mut self.c83, &mut self.c82, &mut self.c67, &mut self.c73] {
                *v = v.max(charge);
            }
            self.c70 = 0.0;
            self.c79 = 0.0;
        }
        let mut ib = [0.0f64; 4];
        // ENV3: C83 into C70 through R291, C70 into Q50's base.
        ib[0] = if triggered {
            0.0
        } else {
            VCA.base_current(self.c70, R290, &mut self.vbe[0])
        };
        let i291 = (self.c83 - self.c70) / R291;
        self.c83 -= i291 * dt / C83;
        self.c70 += (i291 - ib[0] as f32) * dt / C70;
        // ENV2: C82 into C79 through R295, C79 into Q51's base.
        ib[1] = if triggered {
            0.0
        } else {
            VCA.base_current(self.c79, R293, &mut self.vbe[1])
        };
        let i295 = (self.c82 - self.c79) / R295;
        self.c82 -= i295 * dt / C82;
        self.c79 += (i295 - ib[1] as f32) * dt / C79;
        // ENV4: the tone pot drains C67; R297 feeds Q48's base.
        ib[2] = VCA.base_current(self.c67, R297, &mut self.vbe[2]);
        self.c67 *= (-c.tone * dt / C67).exp();
        self.c67 -= ib[2] as f32 * dt / C67;
        // ENV5: R280 feeds Q47's base, which Q43 holds off meanwhile.
        ib[3] = if triggered {
            0.0
        } else {
            VCA.base_current(self.c73, R280, &mut self.vbe[3])
        };
        self.c73 -= ib[3] as f32 * dt / C73;

        // The noise network and its two VCAs.
        let mut injected = [0.0f32; NOISE_UNKNOWNS];
        injected[NOISE_IN] = c.noise * STIFF;
        let x = sh.noise.open(&self.noise, 0, &injected);
        let z = sh.noise.impedance(0, T80, T80) as f64;
        let open80 = x[T80] as f64;
        let (ie48, i48) = if ib[2] > OFF {
            vca_port(&mut self.vc48, open80, z + R299, R_COLLECTOR, ib[2])
        } else {
            (0.0, open80 / (z + R299 + R_COLLECTOR))
        };
        // IC39a's output drives R278 without moving.
        let ie47 = if ib[3] > OFF {
            vca_port(&mut self.vc47, x[HIGH] as f64, R278, R_COLLECTOR, ib[3]).0
        } else {
            0.0
        };
        sh.noise.close(&mut self.noise, 0, x, &[(T80, -i48 as f32)]);

        let snappy = -(c.snappy * (ie47 + ie48)) as f32;
        let snappy = snappy.clamp(-VSAT, VSAT);
        // C78 couples IC39b into IC40b through R294.
        let i294 = (snappy - self.c78) / R294;
        self.c78 += i294 * dt / C78;

        let ie50 = if ib[0] > OFF {
            SHAPER1.solve(&mut self.d1, &mut self.vc50, self.vco1.tri as f64, ib[0])
        } else {
            0.0
        };
        let ie51 = if ib[1] > OFF {
            SHAPER2.solve(&mut self.d2, &mut self.vc51, self.vco2.tri as f64, ib[1])
        } else {
            0.0
        };
        let out = (-(R_LEVEL * (ie50 + ie51 + i294 as f64)) as f32).clamp(-VSAT, VSAT);
        self.quiet.track(out);
        out
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Sd909Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the accent voltage, for a hit up to twice as loud (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// tune control, 0–5 V: the drum's pitch (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tune: Option<PolySignal>,
    /// tone control, 0–5 V: how long the low noise lasts (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// snappy control, 0–5 V: the level of the noise (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    snappy: Option<PolySignal>,
    /// fade out a still-sounding hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the envelopes, as
    /// on the hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Sd909Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-909 snare drum.
///
/// Each rising edge on **trigger** fires the 909's trigger into a circuit
/// model of its snare: two triangle VCOs rounded toward sines, their pitch
/// bent up at the strike, over filtered noise. **tune** sets the pitch,
/// **tone** how long the low noise lasts, **snappy** the noise's level, and
/// **accent**, sampled at each strike, drives it harder.
///
/// An unaccented hit peaks near **±5 V**.
///
/// ```js
/// $tr909.sd($clock.beatTrigger).out()
/// ```
#[module(name = "$tr909.sd", has_init, patch_update, args(trigger))]
pub struct Sd909 {
    outputs: Sd909Outputs,
    params: Sd909Params,
    state: SdState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct SdState {
    shared: SnareShared,
    seeded: bool,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Snare>,
    noise: Noise,
}

impl Sd909 {
    fn init(&mut self, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        let sh = &mut self.state.shared;
        sh.timing = Timing::new(sample_rate);
        sh.dt = dt;
        sh.noise_rate = Noise::rate(sample_rate);
        // The noise network at DC: discretised at a step long enough that
        // its capacitors carry no current.
        let dc = noise_network(DC_STEP);
        let mut injected = [0.0f32; NOISE_UNKNOWNS];
        injected[NOISE_IN] = NOISE_MEAN * STIFF;
        let mut rest = MnaState::default();
        for _ in 0..DC_ITERATIONS {
            dc.step(&mut rest, 0, &injected);
        }
        sh.noise_rest = rest;
        sh.noise = noise_network(dt);
        sh.reset1 = (-dt / VCO1_RESET).exp();
        sh.reset2 = (-dt / VCO2_RESET).exp();
    }

    fn update(&mut self, _sample_rate: f32) {
        let sh = &self.state.shared;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];
            let knob = |s: &Option<PolySignal>, default: f32| {
                s.value_or(ch, default).clamp(0.0, 5.0) / 5.0
            };
            let tune = knob(&p.tune, DEFAULT_TUNE);
            let controls = SnareControls {
                tune: RAIL * (R284 + VR6 * tune) / (R287 + VR6 + R284),
                tone: 1.0 / (R254 + VR7 * knob(&p.tone, DEFAULT_TONE)),
                snappy: VR9 * knob(&p.snappy, DEFAULT_SNAPPY) as f64,
                noise: cs.noise.next(sh.noise_rate),
            };
            let out = cs.voices.process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                sh.timing.choke_samples,
                sh,
                &controls,
            );
            self.outputs.sample.set(ch, out * OUTPUT_GAIN);
        }
    }
}

impl crate::types::PatchUpdateHandler for Sd909 {
    fn on_patch_update(&mut self) {
        if !self.state.seeded {
            let base = seed_base(self);
            for (ch, cs) in self.channel_state.iter_mut().enumerate() {
                cs.noise.seed(base, ch);
            }
            self.state.seeded = true;
        }
    }
}

message_handlers!(impl Sd909 {});
