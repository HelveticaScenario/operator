//! TR-909 bass drum, simulated from the 909 service notes' voicing board
//! schematic. Part names (Q12, C9, …) are the service notes' designators.
//!
//! A triangle VCO (integrator IC13a, Schmitt trigger IC13b, with IC12b
//! flipping the sign of its control voltage) runs at a rate set by IC12a:
//! a fixed offset from the rail through R27 plus the pitch envelope on C9,
//! which the trigger charges and the tune pot drains. The trigger also
//! stops the VCO for its 2 ms through Q11, so every hit starts from the
//! same point. Two diodes round the triangle toward a sine, and transistor
//! Q12, its base fed from the decay envelope on C8, passes it into the
//! output stage IC11a. Alongside, Q6 passes the attack: a ringing click
//! from the pulse shaper Q8/Q9 and a burst of low-passed noise, gated by a
//! short envelope on C12.

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{
    DIODE_DROP, Noise, RAIL, Shaper, TRIG_V, Timing, VCA, VSAT, accent_volts, log_taper, vca_port,
};
use crate::dsp::drums::mna::{GROUND, Mna, MnaBuilder, MnaState, Switch};
use crate::dsp::drums::voice::{Circuit, Pulse, Quiet, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

// The pitch envelope: R21 from the trigger through D7 into C9, drained by
// R57 and the tune pot VR2 to ground and by R26 into IC12a's virtual
// ground.
const R21: f32 = 2.2e3;
const C9: f32 = 0.33e-6;
const R57: f32 = 22.0e3;
const VR2: f32 = 100.0e3;
const R26: f32 = 330.0e3;
/// IC12a's feedback, and R27 from the +15 V rail.
const R28: f32 = 150.0e3;
const R27: f32 = 1.5e6;

// The VCO: IC12b's output, ±the control voltage, through R59 into C14;
// IC13b's thresholds are its swing divided by R49 and R50.
const R59: f32 = 47.0e3;
const C14: f32 = 0.033e-6;
const THRESHOLD: f32 = VSAT * 47.0e3 / (100.0e3 + 47.0e3);
/// Q11 holds C14 through R51 while the trigger lasts.
const R51: f32 = 2.2e3;

// The shaper: R52 into D10 and D11 to ground, then R53 into Q12's
// collector, with R43 to ground.
const SHAPER: Shaper = Shaper {
    r_in: 10.0e3,
    r_out: 2.2e3,
    r_load: 47.0,
};
/// R34, from the decay envelope to Q12's base.
const R34: f32 = 470.0e3;

// The decay envelope: Q4 gates the accent voltage through R35 into node X,
// with R36 and R22 dividing it into node Y, D8 from X to Y, and D9 from X
// into C8; the decay pot VR5 and R58 drain C8.
const DV: usize = 0;
const X: usize = 1;
const Y: usize = 2;
const E1: usize = 3;
const ENV_UNKNOWNS: usize = 4;
const ENV_CAPS: usize = 1;
const GATE: usize = 0;
const D8: usize = 1;
const D9: usize = 2;
const ENV_SWITCHES: usize = 3;
const ENV_COMBOS: usize = 1 << ENV_SWITCHES;
/// Q4's saturated resistance, and a diode's while it conducts.
const R_SWITCH: f32 = 20.0;
const R_DIODE: f32 = 30.0;
const VR5: f32 = 1.0e6;
const R58: f32 = 47.0e3;

// Q4 stays on while Q3 conducts: C7, charged by the trigger through R20
// and D6, drains through R18 into Q3's base, with R17 to ground.
const R20: f32 = 6.8e3;
const C7: f32 = 0.033e-6;
const R18: f32 = 470.0e3;
const R17: f32 = 470.0e3;
/// R32, through which Q3 pulls Q4's base.
const R32: f32 = 47.0e3;
const VBE: f32 = 0.6;
const BETA: f32 = 300.0;

// IC11b scales the accent voltage by 1 + R37/R38.
const ACCENT_GAIN: f32 = 1.0 + 10.0e3 / 4.7e3;
/// The accent voltage of an unaccented hit.
const ACCENT_LOW: f32 = 2.1;

// The attack envelope: Q5 passes the scaled accent through R41 into C12
// for the trigger's length; R493 drains it, and R42 feeds Q6's base.
const R41: f32 = 22.0e3;
const C12: f32 = 0.033e-6;
const R493: f32 = 150.0e3;
const R42: f32 = 470.0e3;

// The attack network: the noise through R10 and R9, C15, R46, C13 and R45;
// the pulse shaper; both into node NB (R56 to ground), then through the
// attack pot VR3 into Q6's collector with R55 to ground.
const NOISE_IN: usize = 0;
const N1: usize = 1;
const N2: usize = 2;
const N3: usize = 3;
const NB: usize = 4;
const P0: usize = 5;
const P1: usize = 6;
const PA: usize = 7;
const PB: usize = 8;
const PE: usize = 9;
const P4: usize = 10;
const OP9: usize = 11;
const ATTACK_UNKNOWNS: usize = 12;
const ATTACK_CAPS: usize = 7;
/// Q8 conducting, holding the pulse shaper's input at the rail.
const Q8_ON: usize = 0;
const ATTACK_SWITCHES: usize = 1;
const ATTACK_COMBOS: usize = 1 << ATTACK_SWITCHES;
/// Conductance driving the noise input to the generator's voltage.
const STIFF: f32 = 10.0;
const VR3: f32 = 500.0;
const R55: f64 = 47.0;

/// The noise generator's mean voltage: high half the time.
const NOISE_MEAN: f32 = 0.5 * RAIL;
const DC_STEP: f32 = 1.0e3;
const DC_ITERATIONS: usize = 4;

/// IC11a's feedback with the level pot at maximum.
const R_LEVEL: f64 = 100.0e3;

/// Scales the output so an unaccented hit peaks near 5 V.
const OUTPUT_GAIN: f32 = 1.95;
/// Below this running peak the drum has faded out.
const SETTLED: f32 = 1.0e-4;
/// The decay envelope at rest: Q12 passes nothing below it.
const E1_SETTLED: f32 = 0.3;

const DEFAULT_TUNE: f32 = 2.5;
const DEFAULT_ATTACK: f32 = 2.5;
const DEFAULT_DECAY: f32 = 2.5;

fn envelope_network(dt: f32) -> Mna<ENV_UNKNOWNS, ENV_CAPS, ENV_SWITCHES, ENV_COMBOS> {
    let mut b = MnaBuilder::<ENV_UNKNOWNS>::default();
    b.resistor(DV, X, 1.8e3)
        .resistor(DV, Y, 47.0e3)
        .resistor(Y, GROUND, 10.0e3)
        .capacitor(E1, GROUND, 0.33e-6);
    let diode = |from, to| Switch {
        node: to,
        other: from,
        g: 1.0 / R_DIODE,
        volts: -DIODE_DROP,
    };
    b.build(
        dt,
        [
            Switch {
                node: DV,
                other: GROUND,
                g: 1.0 / R_SWITCH,
                volts: 0.0,
            },
            diode(X, Y),
            diode(X, E1),
        ],
    )
}

fn attack_network(dt: f32) -> Mna<ATTACK_UNKNOWNS, ATTACK_CAPS, ATTACK_SWITCHES, ATTACK_COMBOS> {
    let mut b = MnaBuilder::<ATTACK_UNKNOWNS>::default();
    b.resistor(NOISE_IN, GROUND, 1.0 / STIFF)
        .resistor(NOISE_IN, N1, 12.0e3)
        .resistor(N1, GROUND, 4.7e3)
        .capacitor(N1, N2, 0.47e-6)
        .resistor(N2, N3, 4.7e3)
        .capacitor(N3, GROUND, 0.1e-6)
        .resistor(N3, NB, 4.7e3)
        .resistor(NB, GROUND, 330.0)
        // Q8's collector: C5 and R14, then C2 and R7 into the Sallen-Key
        // low-pass on Q9 (R15, R16, C3, C4), R8 on its emitter, and C11
        // and R44 into NB.
        .capacitor(P0, GROUND, 0.0068e-6)
        .resistor(P0, GROUND, 22.0e3)
        .capacitor(P0, P1, 0.0033e-6)
        .resistor(P1, PA, 100.0e3)
        .resistor(PA, GROUND, 100.0e3)
        .resistor(PA, PB, 39.0e3)
        .capacitor(PB, GROUND, 100.0e-12)
        .capacitor(PA, PE, 0.0047e-6)
        .opamp(PB, PE, PE, OP9)
        .resistor(PE, GROUND, 10.0e3)
        .capacitor(PE, P4, 0.47e-6)
        .resistor(P4, NB, 22.0e3);
    b.build(
        dt,
        [Switch {
            node: P0,
            other: GROUND,
            g: 1.0 / 100.0,
            volts: RAIL,
        }],
    )
}

/// The bass drum's circuit state.
#[derive(Clone, Copy, Default)]
struct Drum {
    trigger: Pulse,
    /// The scaled accent voltage for this hit.
    accent: f32,
    c9: f32,
    c7: f32,
    c12: f32,
    /// The triangle, and the Schmitt trigger's output: rising while true.
    tri: f32,
    rising: bool,
    envelope: MnaState<ENV_CAPS>,
    x: f32,
    y: f32,
    e1: f32,
    attack: MnaState<ATTACK_CAPS>,
    /// The shaper's diode node and Q12's collector, Q6's collector.
    d: f64,
    c12_node: f64,
    c6_node: f64,
    /// Base-emitter voltages carried between samples for Newton.
    q12_vbe: f64,
    q6_vbe: f64,
    q12_base: f32,
    q6_base: f32,
    /// False until the first strike sets the attack network at rest.
    primed: bool,
    quiet: Quiet,
}

#[derive(Default)]
struct DrumShared {
    timing: Timing,
    dt: f32,
    noise_rate: f32,
    envelope: Mna<ENV_UNKNOWNS, ENV_CAPS, ENV_SWITCHES, ENV_COMBOS>,
    attack: Mna<ATTACK_UNKNOWNS, ATTACK_CAPS, ATTACK_SWITCHES, ATTACK_COMBOS>,
    /// The attack network at rest, Q8 on and the noise at its mean.
    attack_rest: MnaState<ATTACK_CAPS>,
    sync_coeff: f32,
}

struct DrumControls {
    /// Conductances draining C9 and C8, VR3's resistance, and the noise.
    tune: f32,
    decay: f32,
    attack: f32,
    noise: f32,
}

impl Drum {
    /// Advances the triangle one sample at control voltage `cv` (its
    /// magnitude sets the slope), reflecting off IC13b's thresholds.
    #[inline]
    fn vco(&mut self, cv: f32, dt: f32) {
        let mut travel = cv.abs() / (R59 * C14) * dt;
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
}

impl Circuit for Drum {
    type Shared = DrumShared;
    type Controls = DrumControls;

    fn fire(&mut self, accent: f32, shared: &DrumShared) {
        if !self.primed {
            self.attack = shared.attack_rest;
            self.primed = true;
        }
        self.accent = (ACCENT_GAIN * accent_volts(ACCENT_LOW, accent)).min(VSAT);
        self.trigger.fire(TRIG_V, shared.timing.trig_samples);
    }

    fn settled(&self) -> bool {
        !self.trigger.active()
            && self.c7 < self.gate_threshold()
            && self.e1 < E1_SETTLED
            && self.quiet.below(SETTLED)
    }

    fn step(&mut self, sh: &DrumShared, c: &DrumControls) -> f32 {
        let dt = sh.dt;
        let trig = self.trigger.next();
        let triggered = trig > 0.0;

        // The pitch envelope and IC12a.
        let (g_charge, target) = if triggered && trig - DIODE_DROP > self.c9 {
            (1.0 / R21, trig - DIODE_DROP)
        } else {
            (0.0, 0.0)
        };
        let g = g_charge + c.tune + 1.0 / R26;
        let settle = g_charge * target / g;
        self.c9 = settle + (self.c9 - settle) * (-g * dt / C9).exp();
        let cv = -(R28 / R26) * self.c9 - (R28 / R27) * RAIL;

        // The VCO, held by Q11 while the trigger lasts.
        if triggered {
            self.tri *= sh.sync_coeff;
            self.rising = true;
        } else {
            self.vco(cv, dt);
        }

        // Q3 keeps the decay gate open while C7 holds up.
        if triggered {
            let top = trig - DIODE_DROP;
            self.c7 = top + (self.c7 - top) * (-dt / (R20 * C7)).exp();
        } else if self.c7 > VBE {
            self.c7 = VBE + (self.c7 - VBE) * (-dt / (R18 * C7)).exp();
        }
        let gate = triggered || self.c7 > self.gate_threshold();

        // The decay envelope.
        let mut closed = 0;
        let mut injected = [0.0f32; ENV_UNKNOWNS];
        if gate {
            closed |= 1 << GATE;
            injected[DV] = self.accent / R_SWITCH;
        }
        if self.x - self.y > DIODE_DROP {
            closed |= 1 << D8;
        }
        if self.x - self.e1 > DIODE_DROP {
            closed |= 1 << D9;
        }
        injected[E1] -= self.q12_base;
        let x = sh.envelope.open(&self.envelope, closed, &injected);
        let z = sh.envelope.impedance(closed, E1, E1);
        let drain = -c.decay * x[E1] / (1.0 + z * c.decay);
        let x = sh
            .envelope
            .close(&mut self.envelope, closed, x, &[(E1, drain)]);
        self.x = x[X];
        self.y = x[Y];
        self.e1 = x[E1];
        let ib12 = VCA.base_current(self.e1, R34, &mut self.q12_vbe);
        self.q12_base = ib12 as f32;

        // The attack envelope.
        let (g_charge, target) = if triggered {
            (1.0 / R41, self.accent)
        } else {
            (0.0, 0.0)
        };
        let g = g_charge + 1.0 / R493;
        let settle = (g_charge * target - self.q6_base) / g;
        self.c12 = settle + (self.c12 - settle) * (-g * dt / C12).exp();
        let ib6 = VCA.base_current(self.c12, R42, &mut self.q6_vbe);
        self.q6_base = ib6 as f32;

        // The attack network: noise and the pulse into Q6.
        let mut injected = [0.0f32; ATTACK_UNKNOWNS];
        injected[NOISE_IN] = c.noise * STIFF;
        injected[PE] = -RAIL / 10.0e3;
        let closed = if triggered { 0 } else { 1 << Q8_ON };
        let x = sh.attack.open(&self.attack, closed, &injected);
        let z = sh.attack.impedance(closed, NB, NB) as f64;
        let (ie6, drawn) = vca_port(
            &mut self.c6_node,
            x[NB] as f64,
            z + c.attack as f64,
            R55,
            ib6,
        );
        sh.attack
            .close(&mut self.attack, closed, x, &[(NB, -drawn as f32)]);

        let ie12 = SHAPER.solve(&mut self.d, &mut self.c12_node, self.tri as f64, ib12);
        let out = (-(R_LEVEL * (ie12 + ie6)) as f32).clamp(-VSAT, VSAT);
        self.quiet.track(out);
        out
    }
}

impl Drum {
    /// C7's voltage below which Q3 can no longer hold Q4 on.
    fn gate_threshold(&self) -> f32 {
        let need = (self.accent - VBE).max(0.0) / (R32 * BETA);
        VBE + R18 * (need + VBE / R17)
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Bd909Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the accent voltage, for a hit up to twice as loud (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// tune control, 0–5 V: how long the pitch takes to fall to rest
    /// (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tune: Option<PolySignal>,
    /// attack control, 0–5 V: the level of the click (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    attack: Option<PolySignal>,
    /// decay control, 0–5 V: how long the drum rings (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// fade out a still-sounding hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the envelopes, as
    /// on the hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Bd909Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-909 bass drum.
///
/// Each rising edge on **trigger** fires the 909's trigger into a circuit
/// model of its bass drum: a triangle VCO rounded toward a sine, its pitch
/// swept down from about 300 Hz to rest near 56 Hz, with a click and a
/// burst of noise at the attack. **tune** sets how long the sweep takes,
/// **attack** the click's level, **decay** how long the drum rings, and
/// **accent**, sampled at each strike, drives it harder.
///
/// An unaccented hit peaks near **±5 V**.
///
/// ```js
/// $tr909.bd($clock.beatTrigger).out()
/// ```
#[module(name = "$tr909.bd", has_init, patch_update, args(trigger))]
pub struct Bd909 {
    outputs: Bd909Outputs,
    params: Bd909Params,
    state: BdState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct BdState {
    shared: DrumShared,
    seeded: bool,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Drum>,
    noise: Noise,
}

impl Bd909 {
    fn init(&mut self, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        let sh = &mut self.state.shared;
        sh.timing = Timing::new(sample_rate);
        sh.dt = dt;
        sh.noise_rate = Noise::rate(sample_rate);
        sh.envelope = envelope_network(dt);
        // The attack network at DC: discretised at a step long enough that
        // its capacitors carry no current.
        let dc = attack_network(DC_STEP);
        let mut injected = [0.0f32; ATTACK_UNKNOWNS];
        injected[NOISE_IN] = NOISE_MEAN * STIFF;
        injected[PE] = -RAIL / 10.0e3;
        let mut rest = MnaState::default();
        for _ in 0..DC_ITERATIONS {
            dc.step(&mut rest, 1 << Q8_ON, &injected);
        }
        sh.attack_rest = rest;
        sh.attack = attack_network(dt);
        sh.sync_coeff = (-dt / (R51 * C14)).exp();
    }

    fn update(&mut self, _sample_rate: f32) {
        let sh = &self.state.shared;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];
            let knob = |s: &Option<PolySignal>, default: f32| {
                s.value_or(ch, default).clamp(0.0, 5.0) / 5.0
            };
            let controls = DrumControls {
                tune: 1.0 / (R57 + VR2 * log_taper(knob(&p.tune, DEFAULT_TUNE))),
                decay: 1.0 / (R58 + VR5 * log_taper(knob(&p.decay, DEFAULT_DECAY))),
                attack: VR3 * (1.0 - knob(&p.attack, DEFAULT_ATTACK)),
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

impl crate::types::PatchUpdateHandler for Bd909 {
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

message_handlers!(impl Bd909 {});
