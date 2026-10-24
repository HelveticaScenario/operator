//! TR-808 bass drum, simulated block by block from its schematic after
//! Werner, Abel and Smith, "A Physically-Informed, Circuit-Bendable, Digital
//! Model of the Roland TR-808 Bass Drum Circuit" (DAFx-14). Part names (R161,
//! C41, …) are the 808 service notes' designators.
//!
//! The signal path is: a 1 ms trigger pulse → the pulse shaper (a low shelf
//! with a diode clamping its falling edge) → the bridged-T band-pass, kept
//! ringing by the feedback buffer → a passive tone low-pass → the AC-coupled
//! level and output stages. An envelope generator raises the bridged-T's
//! frequency for the first few milliseconds and then kicks it again through
//! the retriggering pulse, and leakage through Q43 makes negative swings ring
//! faster, which gives the note its falling "sigh".

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{
    DIODE_DROP, SILENCE, T60_TO_RATE, Timing, VSAT, decay_secs, pulse_volts, trapezoid,
};
use super::levels;
use crate::dsp::drums::analog::{high_pass, low_pass, one_pole_coeff};
use crate::dsp::drums::group::{Channels, Drum, Groups, Run};
use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::voice::{Circuit, Fire, Pulse, Voices};
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PolyOutput, PolySignal};

mod module;

pub use module::Bd808;

// Pulse shaper: C40 ∥ R163 in series, R162 to ground, D53 clamping negatives.
const R162: f32 = 4.7e3;
const R163: f32 = 100.0e3;
const C40: f32 = 15.0e-9;

// Bridged-T: C41 and C42 form the arms, R167 the bridge, and R165 + R166 the
// path to ground. The retriggering pulse enters through R161 and the feedback
// buffer through R170.
const R161: f32 = 1.0e6;
const R165: f32 = 47.0e3;
const R166: f32 = 6.8e3;
const R167: f32 = 1.0e6;
const R170: f32 = 470.0e3;
const C41_42: f32 = 15.0e-9;
// The feedback buffer is an inverting amp of gain (R169 ∥ VR6) / R164, which
// the decay knob sweeps from 0 to 0.914. Its C43 only shapes sub-audio
// frequencies and is left out.

// Envelope generator: after the trigger pulse ends, C38 charges through R156
// from the 15 V rail until Q42 turns on across this base-voltage window,
// pulling Venv down from its high level.
const RAIL: f32 = 15.0;
const R156_C38: f32 = 1.0e6 * 0.1e-6;
const Q42_ON_START: f32 = 0.5;
const Q42_ON_WIDTH: f32 = 0.2;
/// Venv while Q42 is off: the rail through R160 (22 kΩ), loaded by R159
/// (100 kΩ) into Q43's base.
const VENV_HIGH: f32 = 12.4;

// Retriggering pulse: Venv through C39 into R161, with D52 clamping positives.
const C39: f32 = 33.0e-9;

// Q43's leakage current as a function of Vcomm, fitted in the paper:
// iC = −(M / ALPHA) · ln(1 + e^(ALPHA · (−Vcomm − V0))).
const LEAK_M: f32 = 1.4765e-5;
const LEAK_ALPHA: f32 = 14.315;
const LEAK_V0: f32 = 0.556;
/// Above this Vcomm the leakage current is negligible.
const LEAK_FLOOR: f32 = -0.05;

// Output stages: tone low-pass R171 + (R172 ∥ VR5) into C45, level high-pass
// C47 into VR4, output high-pass C49 into R176.
const R171: f32 = 220.0;
const R172: f32 = 10.0e3;
const VR5: f32 = 10.0e3;
const C45: f32 = 0.1e-6;
const LEVEL_HP_HZ: f32 = 1.0 / (std::f32::consts::TAU * 10.0e3 * 0.47e-6);
const OUTPUT_HP_HZ: f32 = 1.0 / (std::f32::consts::TAU * 100.0e3 * 0.47e-6);

/// The stock decay knob's span of ring times, seconds; the circuit can ring
/// no shorter than the low end at stock pitch.
const DECAY_SHORTEST: f32 = 0.09;
const DECAY_LONGEST: f32 = 3.8;

/// Scales the circuit's output voltage so an unaccented hit at stock pitch
/// peaks near 5 V.
const OUTPUT_GAIN: f32 = 1.23;

/// The circuit's state for one hit sequence.
#[derive(Clone, Copy)]
struct Voice<L: Lanes = Mono> {
    pulse: Pulse<L>,
    /// Voltage across C40.
    shaper: L::S,
    /// Voltage across C38; charged at rest, so Venv starts low.
    c38: L::S,
    /// Voltage across C39, Venv side minus Vrp side.
    c39: L::S,
    /// Voltages across C41 (inverting input to Vcomm) and C42 (Vcomm to Vbt).
    arm1: L::S,
    arm2: L::S,
    vcomm: L::S,
    tone_lp: L::S,
    level_hp: L::S,
    output_hp: L::S,
}

lanewise!(Voice {
    pulse,
    shaper,
    c38,
    c39,
    arm1,
    arm2,
    vcomm,
    tone_lp,
    level_hp,
    output_hp,
});

impl<L: Lanes> Default for Voice<L> {
    fn default() -> Self {
        Self {
            pulse: Pulse::default(),
            shaper: c(0.0),
            c38: c(RAIL),
            c39: c(0.0),
            arm1: c(0.0),
            arm2: c(0.0),
            vcomm: c(0.0),
            tone_lp: c(0.0),
            level_hp: c(0.0),
            output_hp: c(0.0),
        }
    }
}

impl Fire for Voice {
    fn fire(&mut self, accent: f32, st: &Bd808State) {
        self.pulse
            .fire(pulse_volts(accent), st.timing.pulse_samples);
    }
}

impl<L: Lanes> Circuit<L> for Voice<L> {
    type Shared = Bd808State;
    type Controls = Controls<L>;

    fn settled(&self) -> L::SM {
        let silent = |v: L::S| v.abs().lt(c(SILENCE));
        (!self.pulse.active())
            .and_then(|| self.c38.gt(c(Q42_ON_START + Q42_ON_WIDTH)))
            .and_then(|| silent(self.shaper) & silent(self.c39))
            .and_then(|| silent(self.arm1) & silent(self.arm2))
            .and_then(|| silent(self.tone_lp) & silent(self.level_hp) & silent(self.output_hp))
    }

    fn step(&mut self, st: &Bd808State, controls: &Controls<L>) -> L::S {
        self.advance(st, controls) * controls.level
    }
}

/// The bass drum at every width.
struct BassDrum;

impl Drum for BassDrum {
    type Shared = Bd808State;
    type Circuit<L: Lanes> = Voice<L>;
    type Controls<L: Lanes> = Controls<L>;
    type Sources<L: Lanes> = ();
    const QUAD: bool = true;
    const PAIR: bool = false;
}

#[derive(Default)]
struct Bd808State {
    timing: Timing,
    dt: f32,
    /// Per-sample decay multipliers, which depend only on the sample rate.
    shaper_mul: f32,
    c38_coeff: f32,
    /// One sample's charge of C39 per volt across R161.
    c39_step: f32,
    level_hp_coeff: f32,
    output_hp_coeff: f32,
    sample_rate: f32,
    /// Centre frequency of the stock bridged-T at rest, Hz.
    rest_hz: f32,
}

/// Bridged-T coefficients for one `freq` and `decay`. Scaling both arm
/// capacitors by 1/r scales every frequency by r; the feedback buffer gain
/// (Vfb = −g·Vbt) then sets the decay rate.
#[derive(Clone, Copy)]
struct Network {
    freq: f32,
    decay: f32,
    /// Arm capacitance, F.
    c: f32,
    /// 1 / (R167·C).
    a: f32,
    g: f32,
    /// Output gain levelling `freq`.
    level: f32,
}

impl Default for Network {
    fn default() -> Self {
        // NaN never compares equal, so the first sample always computes.
        Self {
            freq: f32::NAN,
            decay: f32::NAN,
            c: C41_42,
            a: 0.0,
            g: 0.0,
            level: 1.0,
        }
    }
}

impl Network {
    fn new(freq: f32, decay: f32, rest_hz: f32) -> Self {
        let r = voct_to_hz(freq) / rest_hz;
        let c = C41_42 / r;
        let a = 1.0 / (R167 * c);
        // The ring decays at (a − (g/R170 − 1/R167)/C) / 2, plus the loss of
        // R161 in series with C39, which looks like a lossy capacitance at
        // Vcomm: its susceptance b damps the ring by a·b / (2·ω·C).
        let w = std::f32::consts::TAU * rest_hz * r;
        let x = 1.0 / (w * C39);
        let b = x / (R161 * R161 + x * x);
        let branch_rate = a * b / (2.0 * w * c);
        let rate = T60_TO_RATE / decay_secs(decay, DECAY_SHORTEST, DECAY_LONGEST) - branch_rate;
        let g_crit = R170 * (1.0 / R167 + c * a);
        let g = (R170 * (1.0 / R167 + c * (a - 2.0 * rate))).clamp(0.0, g_crit * 0.999);
        Self {
            freq,
            decay,
            c,
            a,
            g,
            level: levels::KICK.gain(freq),
        }
    }
}

/// The settings for one sample: the bridged-T tuned for `freq` and
/// `decay` (its arm capacitance, 1 / (R167·C), and the feedback gain), and
/// the tone low-pass coefficient, and the output gain levelling `freq`.
#[derive(Clone, Copy, Default)]
struct Controls<L: Lanes = Mono> {
    c: L::S,
    a: L::S,
    g: L::S,
    tone_coeff: L::S,
    level: L::S,
}

lanewise!(Controls {
    c,
    a,
    g,
    tone_coeff,
    level
});

impl<L: Lanes> Voice<L> {
    /// Advances the circuit one sample and returns the output stage's voltage.
    #[inline]
    fn advance(&mut self, st: &Bd808State, controls: &Controls<L>) -> L::S {
        let dt = st.dt;
        let vtrig = self.pulse.next();

        // Pulse shaper: the shelf passes the edges, and D53 stops the
        // falling edge a diode drop below ground by discharging C40.
        let mut vplus = vtrig - self.shaper;
        let clamped = vplus.lt(c(0.0));
        if clamped.any() {
            let e = (vplus / DIODE_DROP).map_where(clamped, c(0.0), f32::exp);
            vplus = L::S::select(clamped, (c::<L::S>(1.0) - e) * -DIODE_DROP, vplus);
            self.shaper = L::S::select(clamped, vtrig - vplus, self.shaper);
        }
        let shaper_rest = vtrig * R163 / (R162 + R163);
        self.shaper = shaper_rest + (self.shaper - shaper_rest) * st.shaper_mul;

        // Envelope generator: the trigger pulse holds C38 empty, and Q43
        // grounds the R165/R166 junction for as long as Venv stays high.
        let charging = self.c38 + (c::<L::S>(RAIL) - self.c38) * st.c38_coeff;
        self.c38 = L::S::select(vtrig.gt(c(0.0)), c(0.0), charging);
        let q42 = ((self.c38 - Q42_ON_START) / Q42_ON_WIDTH).clamp(c(0.0), c(1.0));
        let attack = c::<L::S>(1.0) - q42 * q42 * (c::<L::S>(3.0) - q42 * 2.0);
        let venv = attack * VENV_HIGH;

        // Retriggering pulse: D52 shaves Venv's rising edge, so its
        // falling edge reaches the bridged-T as a large negative pulse.
        let vrp = venv - self.c39;
        let shaved = vrp.gt(c(DIODE_DROP));
        let vrp = L::S::select(shaved, c(DIODE_DROP), vrp);
        self.c39 = L::S::select(shaved, venv - DIODE_DROP, self.c39);
        self.c39 += (vrp - self.vcomm) * st.c39_step;

        // Q43 leakage on negative swings feeds Vcomm a current I(Vcomm)
        // that pulls it back towards ground. Linearised about the last
        // Vcomm as I0 − G·(Vcomm − V0), its slope G joins the implicit
        // solve as a conductance, which keeps it stable at any pitch.
        let leaking = self.vcomm.lt(c(LEAK_FLOOR));
        let (leak_g, leak_i) = if leaking.any() {
            let x = (-self.vcomm - LEAK_V0) * LEAK_ALPHA;
            let soft = !x.gt(c(20.0));
            let softplus = x.map_where(soft, x, |x| x.exp().ln_1p());
            let share = (c::<L::S>(1.0) - attack) * R165 / (R165 + R166);
            let current = share * (LEAK_M / LEAK_ALPHA) * softplus;
            let slope = share * LEAK_M / ((-x).map(f32::exp) + 1.0);
            (
                L::S::select(leaking, slope, c(0.0)),
                L::S::select(leaking, current + slope * self.vcomm, c(0.0)),
            )
        } else {
            (c(0.0), c(0.0))
        };
        let g_ground = (c::<L::S>(1.0) - attack) / (R165 + R166) + attack / R166 + leak_g;

        let Controls { c: cap, a, g, .. } = *controls;
        let g_total = g_ground + (1.0 / R161 + 1.0 / R170);

        // States u = (arm1, arm2), the voltages across C41 and C42, with
        // u1' = −a·(u1 + u2) and C·u2' the current into Vcomm less the
        // current through R167. While the op-amp is linear its inverting
        // input follows V+, so Vcomm = V+ − u1 and Vbt = Vcomm − u2:
        //   C·u2' = −(u1 + u2)/R167 + Vrp/R161 + I − (V+ − u1)·Gt
        //           − (g/R170)·(V+ − u1 − u2)
        // Past the rail Vbt is pinned at ±VSAT, so Vcomm = Vbt + u2:
        //   C·u2' = −(u1 + u2)/R167 + Vrp/R161 + I − (Vbt + u2)·Gt
        //           − (g/R170)·Vbt
        let fb = g / R170;
        let drive = vrp / R161 + leak_i;
        let linear = trapezoid(
            (self.arm1, self.arm2),
            [
                -a,
                -a,
                (g_total - 1.0 / R167 + fb) / cap,
                (fb - 1.0 / R167) / cap,
            ],
            (drive - vplus * (g_total + fb)) / cap,
            dt,
        );
        let mut vcomm = vplus - linear.0;
        let mut vbt = vcomm - linear.1;
        let over = vbt.abs().gt(c(VSAT));
        let (arm1, arm2) = if over.any() {
            let pinned_bt = L::S::select(vbt.lt(c(0.0)), c(-VSAT), c(VSAT));
            let pinned = trapezoid(
                (self.arm1, self.arm2),
                [
                    -a,
                    -a,
                    c::<L::S>(-1.0) / (cap * R167),
                    (c::<L::S>(-1.0 / R167) - g_total) / cap,
                ],
                (drive - pinned_bt * (g_total + fb)) / cap,
                dt,
            );
            vcomm = L::S::select(over, pinned_bt + pinned.1, vcomm);
            vbt = L::S::select(over, pinned_bt, vbt);
            (
                L::S::select(over, pinned.0, linear.0),
                L::S::select(over, pinned.1, linear.1),
            )
        } else {
            linear
        };
        self.arm1 = arm1.sanitize();
        self.arm2 = arm2.sanitize();
        self.vcomm = vcomm;

        // Tone low-pass, then the level and output coupling high-passes.
        let lp = low_pass(&mut self.tone_lp, vbt, controls.tone_coeff);
        let level = high_pass(&mut self.level_hp, lp, c(st.level_hp_coeff));
        high_pass(&mut self.output_hp, level, c(st.output_hp_coeff))
    }
}

/// Cutoff of the tone low-pass for a `tone` of 0–5 V. VR5 is a reverse-log
/// pot, so turning it up removes resistance quickly at first.
fn tone_hz(tone: f32) -> f32 {
    let rotation = 1.0 - tone.clamp(0.0, 5.0) / 5.0;
    let vr5 = VR5 * (10f32.powf(2.0 * rotation) - 1.0) / 99.0;
    let r_eq = R171 + R172 * vr5 / (R172 + vr5);
    1.0 / (std::f32::consts::TAU * r_eq * C45)
}
