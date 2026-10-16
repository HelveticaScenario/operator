//! TR-808 rim shot and claves, simulated from the 808 service notes' voicing
//! board schematic. Part names (R308, C119, …) are the service notes'
//! designators; the RS/CL switch picks between the two voices.
//!
//! Both start from IC20a, a bridged-T resonator struck through C114 and D64.
//! For the claves an inverting buffer (IC20b) feeds the ring back through
//! R313 for a high Q, and its output is the voice. For the rim shot C118
//! lowers IC20a's ring, IC21 adds a second, lower resonator, and the sum of
//! both drives Q62, which chops a short envelope on C24. A resonant
//! Sallen-Key high-pass (Q63) turns the chopped envelope into the rim
//! shot's crack.

use std::sync::OnceLock;

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{Arms, BridgedT, DiodeCoupler, SILENCE, T60_TO_RATE, Timing, pulse_volts};
use crate::dsp::drums::analog::{VT, high_pass, one_pole_coeff, rc_hz};
use crate::dsp::drums::voice::{Circuit, Pulse, Voices};
use crate::dsp::utils::{sanitize, voct_to_hz};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

// IC20a's input: C114 into R306, D64 into the R310/R311 divider.
const IC20_IN: DiodeCoupler = DiodeCoupler {
    c: 0.001e-6,
    r_shunt: 22.0e3,
    r_series: 33.0e3,
    r_ground: 10.0e3,
};
// IC20a's bridged-T: C119 and C117 with R308, R312 to ground. The rim shot
// switches C118 across C117.
const R312: f32 = 1.0e3;
const CLAVES_T: BridgedT = BridgedT {
    c_a: 0.0022e-6,
    c_b: 0.0022e-6,
    r_bridge: 820.0e3,
    g_fb: 0.0,
};
const RIM_HIGH_T: BridgedT = BridgedT {
    c_a: 0.0022e-6,
    c_b: 0.0022e-6 + 0.0022e-6,
    r_bridge: 820.0e3,
    g_fb: 0.0,
};
// IC21: C111 into R307, D62 into R301/R302, and C116, C115, R316 and R315.
const IC21_IN: DiodeCoupler = DiodeCoupler {
    c: 0.01e-6,
    r_shunt: 100.0e3,
    r_series: 47.0e3,
    r_ground: 10.0e3,
};
const R315: f32 = 5.6e3;
const RIM_LOW_T: BridgedT = BridgedT {
    c_a: 0.0047e-6,
    c_b: 0.0047e-6,
    r_bridge: 1.0e6,
    g_fb: 0.0,
};

// Q62's base: IC21 through R318 and IC20b through R320 meet R317 to ground,
// then C120 into the base, which R321 biases from the 15 V rail.
const R317: f32 = 10.0e3;
const R318: f32 = 220.0e3;
const R320: f32 = 22.0e3;
const C120: f32 = 10.0e-6;
const R321: f32 = 2.2e6;
const RAIL: f32 = 15.0;
// Q62's Ebers-Moll transport parameters.
const Q62_IS: f32 = 10.0e-15;
const Q62_BETA_F: f32 = 300.0;
const Q62_BETA_R: f32 = 1.0;
/// Q62's base voltage at rest, its collector open: R321's current all flows
/// out through the base, split between both junctions.
const Q62_REST: f32 = 0.542_676;
/// Q62's collector voltage at rest, VT·ln 2, where its two junctions'
/// currents cancel at the collector. C121 holds it.
const E_REST: f32 = 0.017_918;
// The envelope: the pulse charges C24 through D61 and R107; C24 drains
// through R108 and D63 into Q62's collector.
const R107: f32 = 1.0e3;
const R108: f32 = 10.0e3;
const C24: f32 = 0.47e-6;
const DIODE_DROP: f32 = 0.6;
const D63_IS: f32 = 2.52e-9;
/// Below this C24 drains through D63 too slowly to move the high-pass, and
/// the rim shot is spent.
const C24_SPENT: f32 = 0.45;
/// Rate the rim shot circuit is simulated at, Hz.
const RIM_RATE: f32 = 96_000.0;
/// C120 and C121 relax back to Q62's bias through megohms; within this they
/// have settled.
const C120_SETTLED: f32 = 1.0e-3;
const D63_N: f32 = 1.752;
// Q63's Sallen-Key high-pass (C121, C122, R322 from their junction to Q63's
// emitter, R325 from its base to ground), then R324 and C125
// into R326 and IC19. R324's value is not legible in the service notes;
// 22 kΩ is assumed.
const C121: f32 = 0.0047e-6;
const C122: f32 = 0.0047e-6;
const R322: f32 = 10.0e3;
const R325: f32 = 330.0e3;
const R324: f32 = 22.0e3;
const C125: f32 = 0.01e-6;
const R326: f32 = 10.0e3;
// The claves leave IC20b through R319 into C125 and R326.
const R319: f32 = 220.0e3;
// IC19's feedback: R330 ∥ the level pot at maximum (100 kΩ).
const R_LEVEL: f32 = 470.0e3 * 100.0e3 / (470.0e3 + 100.0e3);

/// Scale the outputs so an unaccented hit peaks near 5 V.
const RIM_GAIN: f32 = 0.519;
const CLAVES_GAIN: f32 = 3.62;

/// Conductance of Q62's base mix, and each resonator's share of it.
const MIX_G: f32 = 1.0 / R317 + 1.0 / R318 + 1.0 / R320;

/// Claves ring time to −60 dB: three times the service notes' 25 ms decay
/// to −20 dB. R313 sits so close to the T's damping that a stock 808 rings
/// for as long as its tolerances allow; Q74 mutes IC19 after ≈110 ms to
/// hide the leak.
const CLAVES_DECAY: f32 = 0.075;
const MIN_DECAY: f32 = 0.005;
const MAX_DECAY: f32 = 10.0;

/// Largest voltage across D63 and R108 the table covers, and its steps.
const D63_MAX: f32 = 16.0;
const D63_STEPS: usize = 4096;

/// Current through D63 (a 1S2473) and R108 for voltages 0–[`D63_MAX`]
/// across the pair.
static D63: OnceLock<Box<[f32]>> = OnceLock::new();

fn d63_table() -> &'static [f32] {
    D63.get_or_init(|| {
        (0..=D63_STEPS)
            .map(|j| d63_exact(D63_MAX * j as f32 / D63_STEPS as f32))
            .collect()
    })
}

/// Solves v = n·VT·ln(1 + i/IS) + i·R108 for i by Newton on the diode
/// voltage, which converges from zero for any `v`.
fn d63_exact(v: f32) -> f32 {
    let (v, nvt, is, r) = (v as f64, (D63_N * VT) as f64, D63_IS as f64, R108 as f64);
    let mut vd = 0.0f64;
    for _ in 0..200 {
        let i = is * ((vd / nvt).exp() - 1.0);
        let f = vd + i * r - v;
        vd -= f / (1.0 + r * (i + is) / nvt);
    }
    (is * ((vd / nvt).exp() - 1.0)) as f32
}

/// D63's current for a voltage `v` across it and R108, and its slope.
#[inline]
fn d63(table: &[f32], v: f32) -> (f32, f32) {
    if v <= 0.0 {
        return (0.0, 0.0);
    }
    let pos = v.min(D63_MAX) * (D63_STEPS as f32 / D63_MAX);
    let j = (pos as usize).min(D63_STEPS - 1);
    let (lo, hi) = (table[j], table[j + 1]);
    (
        lo + (pos - j as f32) * (hi - lo),
        (hi - lo) * (D63_STEPS as f32 / D63_MAX),
    )
}

/// The rim shot's circuit state.
#[derive(Clone, Copy)]
struct Rim {
    pulse: Pulse,
    c114: f32,
    c111: f32,
    high: Arms,
    low: Arms,
    /// Voltage across C120, base side minus mix side.
    c120: f32,
    vbe: f32,
    /// Q62's collector voltage on the last sample.
    collector: f32,
    c24: f32,
    /// Voltages across C121 and C122.
    c121: f32,
    c122: f32,
    couple: f32,
}

impl Default for Rim {
    fn default() -> Self {
        Self {
            pulse: Pulse::default(),
            c114: 0.0,
            c111: 0.0,
            high: Arms::default(),
            low: Arms::default(),
            c120: Q62_REST,
            vbe: Q62_REST,
            collector: E_REST,
            c24: 0.0,
            c121: E_REST,
            c122: 0.0,
            couple: 0.0,
        }
    }
}

#[derive(Default)]
struct RimShared {
    timing: Timing,
    /// The circuit runs at a multiple of the sample rate near
    /// [`RIM_RATE`], `dt` apart: its crack is set by transients only tens of
    /// microseconds long.
    substeps: u32,
    dt: f32,
    c24_charge: f32,
    d63: &'static [f32],
    /// Backward Euler on (C121, C122) is linear in the collector voltage E:
    /// the new voltages are `crack_inv·old + E·crack_dir`.
    crack_inv: [f32; 4],
    crack_dir: (f32, f32),
    couple_coeff: f32,
}

impl RimShared {
    /// C121 and C122's new voltages for a collector voltage `e`.
    #[inline]
    fn crack(&self, old: (f32, f32), e: f32) -> (f32, f32) {
        let m = &self.crack_inv;
        (
            m[0] * old.0 + m[1] * old.1 + e * self.crack_dir.0,
            m[2] * old.0 + m[3] * old.1 + e * self.crack_dir.1,
        )
    }
}

struct RimControls {
    high: BridgedT,
    low: BridgedT,
}

impl Circuit for Rim {
    type Shared = RimShared;
    type Controls = RimControls;

    fn fire(&mut self, accent: f32, shared: &RimShared) {
        let volts = pulse_volts(accent);
        self.pulse.fire(volts, shared.timing.pulse_samples);
    }

    fn settled(&self) -> bool {
        !self.pulse.active()
            && self.high.settled()
            && self.low.settled()
            && (self.c120 - Q62_REST).abs() < C120_SETTLED
            && self.c24 < C24_SPENT
            && (self.c121 - E_REST).abs() < C120_SETTLED
            && [self.c114, self.c111, self.c122, self.couple]
                .iter()
                .all(|v| v.abs() < SILENCE)
    }

    fn step(&mut self, sh: &RimShared, c: &RimControls) -> f32 {
        let vtrig = self.pulse.next();
        let mut sum = 0.0;
        for _ in 0..sh.substeps {
            sum += self.substep(sh, c, vtrig);
        }
        sum / sh.substeps as f32
    }
}

impl Rim {
    /// Advances the circuit one substep with the trigger pulse at `vtrig`,
    /// returning IC19's output.
    fn substep(&mut self, sh: &RimShared, c: &RimControls, vtrig: f32) -> f32 {
        let dt = sh.dt;

        let vp = IC20_IN.step(&mut self.c114, vtrig, dt);
        let (high, arms) = c.high.step(self.high, vp, 1.0 / R312, 0.0, dt);
        self.high = arms;
        let vp = IC21_IN.step(&mut self.c111, vtrig, dt);
        let (low, arms) = c.low.step(self.low, vp, 1.0 / R315, 0.0, dt);
        self.low = arms;

        let mix = (low / R318 - high / R320) / MIX_G;

        // The pulse charges C24 through D61.
        let head = vtrig - DIODE_DROP;
        if head > self.c24 {
            self.c24 += (head - self.c24) * sh.c24_charge;
        }
        let old = (self.c121, self.c122);
        let (n1, n2) = sh.crack(old, 0.0);
        let i0 = n2 / R322 - (n1 + n2) / R325;
        let k = sh.crack_dir.1 / R322 + (1.0 - sh.crack_dir.0 - sh.crack_dir.1) / R325;

        // Q62 as an Ebers-Moll transistor, its base voltage and collector
        // node E solved together by Newton with SPICE-style step limits.
        // The base sees the resonators' mix (IC20b inverts IC20a) through its
        // source resistance and C120, plus R321's bias. E is fed by D63 from
        // C24, sunk by Q62's collector, and coupled through C121 into the
        // Sallen-Key high-pass, whose positive feedback through R322 drives
        // E whenever both let go; the current into C121 is affine in E.
        let mut vb = self.vbe;
        let mut e = self.collector;
        for _ in 0..12 {
            let x = (vb / VT).exp();
            let r = ((vb - e) / VT).min(40.0).exp();
            let ib = Q62_IS * ((x - 1.0) / Q62_BETA_F + (r - 1.0) / Q62_BETA_R);
            let ic = Q62_IS * (x - r - (r - 1.0) / Q62_BETA_R);
            let (feed, dfeed) = d63(sh.d63, self.c24 - e);
            let f1 = (RAIL - vb) / R321 + (mix - vb + self.c120) * MIX_G - ib;
            let f2 = feed - ic - (i0 + k * e);
            let dr = Q62_IS * r / VT;
            let j11 = -1.0 / R321 - MIX_G - Q62_IS * x / (Q62_BETA_F * VT) - dr / Q62_BETA_R;
            let j12 = dr / Q62_BETA_R;
            let j21 = -(Q62_IS * x / VT - dr * (1.0 + 1.0 / Q62_BETA_R));
            let j22 = -dfeed - dr * (1.0 + 1.0 / Q62_BETA_R) - k;
            let det = j11 * j22 - j12 * j21;
            let dvb = ((j22 * f1 - j12 * f2) / det).clamp(-2.0 * VT, 2.0 * VT);
            let de = ((j11 * f2 - j21 * f1) / det).clamp(-0.1, 0.1);
            vb -= dvb;
            // No junction of Q62 forward-biases past 0.9 V.
            e = (e - de).max(vb - 0.9);
            if dvb.abs() < 1.0e-6 && de.abs() < 1.0e-6 {
                break;
            }
        }
        self.vbe = vb;
        self.c120 -= dt * (mix - vb + self.c120) * MIX_G / C120;
        self.collector = e;
        self.c24 -= dt * d63(sh.d63, self.c24 - e).0 / C24;
        let (c121, c122) = sh.crack(old, e);
        self.c121 = sanitize(c121);
        self.c122 = sanitize(c122);

        // Q63 follows the high-pass's output, E − C121 − C122.
        let crack = e - self.c121 - self.c122;
        high_pass(&mut self.couple, crack, sh.couple_coeff) / (R324 + R326) * -R_LEVEL
    }
}

/// The claves' circuit state.
#[derive(Clone, Copy, Default)]
struct Claves {
    pulse: Pulse,
    c114: f32,
    arms: Arms,
    couple: f32,
}

#[derive(Default)]
struct ClavesShared {
    timing: Timing,
    dt: f32,
    couple_coeff: f32,
}

impl Circuit for Claves {
    type Shared = ClavesShared;
    type Controls = BridgedT;

    fn fire(&mut self, accent: f32, shared: &ClavesShared) {
        let volts = pulse_volts(accent);
        self.pulse.fire(volts, shared.timing.pulse_samples);
    }

    fn settled(&self) -> bool {
        !self.pulse.active()
            && self.arms.settled()
            && self.c114.abs() < SILENCE
            && self.couple.abs() < SILENCE
    }

    fn step(&mut self, sh: &ClavesShared, t: &BridgedT) -> f32 {
        let vtrig = self.pulse.next();
        let vp = IC20_IN.step(&mut self.c114, vtrig, sh.dt);
        let (out, arms) = t.step(self.arms, vp, 1.0 / R312, 0.0, sh.dt);
        self.arms = arms;
        // IC20b inverts the ring on its way out through R319, C125 and R326
        // into IC19, which inverts it back.
        high_pass(&mut self.couple, out, sh.couple_coeff) * R_LEVEL / (R319 + R326)
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Rs808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the trigger pulse from 4 V to 8 V, for a hit up to twice as loud
    /// (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// pitch of the upper resonator in V/Oct, scaling both; stock is
    /// ≈1.79 kHz (default 2.771)
    #[signal(type = pitch, default = 2.771, range = (1.5, 4.0))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// fade out a still-sounding hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the ring, as on the
    /// hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Rs808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 rim shot.
///
/// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into a
/// circuit model of its rim shot: two resonators near 1.8 kHz and 450 Hz
/// switch a transistor that chops a short envelope, and a resonant high-pass
/// turns the result into a bright, 10 ms crack. **accent**, sampled at each
/// strike, drives the circuit harder.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near
/// **±10 V**.
///
/// ```js
/// $tr808.rs($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.rs", has_init, args(trigger))]
pub struct Rs808 {
    outputs: Rs808Outputs,
    params: Rs808Params,
    state: RimShared,
    channel_state: Box<[Voices<Rim>]>,
}

/// The rim shot's upper resonator as built, Hz.
fn rim_rest_hz() -> f32 {
    RIM_HIGH_T.rest_hz(1.0 / R312)
}

impl Rs808 {
    fn init(&mut self, sample_rate: f32) {
        let st = &mut self.state;
        st.timing = Timing::new(sample_rate);
        st.substeps = (RIM_RATE / sample_rate).round().max(1.0) as u32;
        let rate = sample_rate * st.substeps as f32;
        st.dt = 1.0 / rate;
        st.c24_charge = 1.0 - (-st.dt / (R107 * C24)).exp();
        st.d63 = d63_table();
        // Backward Euler on the high-pass's capacitors, with
        // C121·v1' = v2/R322 + (E − v1 − v2)/R325 and
        // C122·v2' = (E − v1 − v2)/R325.
        let (a, b) = (st.dt / C121, st.dt / C122);
        let (m11, m12, m21, m22) = (
            1.0 + a / R325,
            a / R325 - a / R322,
            b / R325,
            1.0 + b / R325,
        );
        let det = m11 * m22 - m12 * m21;
        st.crack_inv = [m22 / det, -m12 / det, -m21 / det, m11 / det];
        let (d1, d2) = (a / R325, b / R325);
        st.crack_dir = ((m22 * d1 - m12 * d2) / det, (m11 * d2 - m21 * d1) / det);
        st.couple_coeff = one_pole_coeff(rc_hz(R324 + R326, C125), rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let st = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            // `freq` scales both resonators and keeps their stock decays.
            let r = voct_to_hz(p.freq.value_or(ch, 2.771)) / rim_rest_hz();
            let controls = RimControls {
                high: RIM_HIGH_T.tuned(r, T60_TO_RATE / RIM_HIGH_T.decay_rate()),
                low: RIM_LOW_T.tuned(r, T60_TO_RATE / RIM_LOW_T.decay_rate()),
            };
            let out = self.channel_state[ch].process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                st.timing.choke_samples,
                st,
                &controls,
            );
            self.outputs.sample.set(ch, out * RIM_GAIN);
        }
    }
}

message_handlers!(impl Rs808 {});

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Cl808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the trigger pulse from 4 V to 8 V, for a hit up to twice as loud
    /// (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// pitch of the resonator in V/Oct; stock is ≈2.53 kHz (default 3.273)
    #[signal(type = pitch, default = 3.273, range = (1.5, 4.5))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// seconds for the ring to fall 60 dB; stock is 0.075 s (default 0.075)
    #[signal(default = 0.075, range = (0.01, 1.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// fade out a still-ringing hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the ring, as on the
    /// hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Cl808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 claves.
///
/// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into a
/// circuit model of its claves: a high-Q bridged-T resonator rings at
/// ≈2.5 kHz. **decay** sets how long it rings, and **accent**, sampled at
/// each strike, drives it harder.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near
/// **±10 V**.
///
/// ```js
/// $tr808.cl($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.cl", has_init, args(trigger))]
pub struct Cl808 {
    outputs: Cl808Outputs,
    params: Cl808Params,
    state: ClavesShared,
    channel_state: Box<[Voices<Claves>]>,
}

impl Cl808 {
    fn init(&mut self, sample_rate: f32) {
        let st = &mut self.state;
        st.timing = Timing::new(sample_rate);
        st.dt = 1.0 / sample_rate;
        st.couple_coeff = one_pole_coeff(rc_hz(R319 + R326, C125), sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let st = &self.state;
        let rest_hz = CLAVES_T.rest_hz(1.0 / R312);
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let r = voct_to_hz(p.freq.value_or(ch, 3.273)) / rest_hz;
            let decay = p
                .decay
                .value_or(ch, CLAVES_DECAY)
                .clamp(MIN_DECAY, MAX_DECAY);
            let t = CLAVES_T.tuned(r, decay);
            let out = self.channel_state[ch].process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                st.timing.choke_samples,
                st,
                &t,
            );
            self.outputs.sample.set(ch, out * CLAVES_GAIN);
        }
    }
}

message_handlers!(impl Cl808 {});
