//! TR-808 snare drum, simulated from the 808 service notes' voicing board
//! schematic. Part names (R197, C58, …) are the service notes' designators.
//!
//! The trigger pulse strikes two bridged-T resonators: IC14a through a
//! differentiating network, ringing near 250 Hz, and IC14b through a plain
//! divider, ringing near 500 Hz. The tone pot crossfades between them. The
//! "snappy" circuit charges C51 from the pulse through Q47; as it bleeds away
//! through R184 and R185, Q48, driven by white noise at its base, chops the
//! voltage at its collector into noise that a Sallen-Key high-pass (Q49)
//! thins out before it joins the drum.

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{
    Arms, Biquad, BiquadState, BridgedT, SILENCE, T60_TO_RATE, WhiteNoise, pulse_volts,
};
use super::levels;
use crate::dsp::drums::analog::{high_pass, low_pass, one_pole_coeff, rc_hz};
use crate::dsp::drums::group::{ChannelInputs, Drum, Groups, Run};
use crate::dsp::drums::lanes::{Lanes, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::voice::{Circuit, Fire, Pulse, Timing, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PolyOutput, PolySignal};

mod module;

pub use module::Sd808;

// Low resonator: the pulse reaches IC14a's input through R189 ∥ C57 against
// R190, a spike a few microseconds long; C58, C59, R196 and R197 form the T.
const R189: f32 = 100.0e3;
const R190: f32 = 680.0;
const C57: f32 = 0.0068e-6;
const LOW: BridgedT = BridgedT {
    c_a: 0.027e-6,
    c_b: 0.027e-6,
    r_bridge: 820.0e3,
    g_fb: 0.0,
};
const R196: f32 = 680.0;
// High resonator: the pulse divided by R191 and R192; C60, C61, R195 and
// R198 form the T.
const R191: f32 = 8.2e3;
const R192: f32 = 220.0;
const HIGH: BridgedT = BridgedT {
    c_a: 0.0068e-6,
    c_b: 0.0068e-6,
    r_bridge: 1.0e6,
    g_fb: 0.0,
};
const R195: f32 = 2.2e3;

// Tone pot VR8 (100 kΩ linear): the low resonator through C63 into one end,
// the high one through R200 and C65 into the other, the wiper through R204
// into IC13's virtual ground.
const C63: f32 = 0.047e-6;
const C65: f32 = 0.047e-6;
const R200: f32 = 47.0e3;
const R204: f32 = 100.0e3;
const VR8: f32 = 100.0e3;

// Snappy: the pulse through VR9 (10 kΩ linear over R199) and Q47 charges
// C51, which bleeds through R184, C52 and R185 into Q48's collector.
const R199: f32 = 1.0e3;
const VR9: f32 = 10.0e3;
const Q47_VBE: f32 = 0.6;
const C51: f32 = 0.47e-6;
const C52: f32 = 0.1e-6;
const R184: f32 = 15.0e3;
const R185: f32 = 15.0e3;
// Q48 sits in saturation, its collector voltage jittering with the noise at
// its base. Fitted to a transistor-level simulation: above `CHOP_V0` the
// collector node averages `CHOP_MEAN` of the way from `CHOP_V0` to C52's
// voltage, so R185 carries the rest, and its noise has an rms of `CHOP_RMS`
// times that span with a spectrum flat to `CHOP_HZ`. `LEAK` is what Q48
// still draws once C52 falls below `CHOP_V0`. R185's load aims `CHOP_SINK`
// lower so C52 crosses `CHOP_V0` in finite time, as the diode's soft knee
// lets it.
const CHOP_V0: f32 = 0.33;
const CHOP_SINK: f32 = 0.02;
const CHOP_MEAN: f32 = 0.53;
const CHOP_RMS: f32 = 0.18;
const CHOP_HZ: f32 = 8_000.0;
const LEAK: f32 = 390.0e3;
// Q49's Sallen-Key high-pass (C66, C67, R201, R202), then C69 and R205 into
// IC13's virtual ground.
const C66: f32 = 0.0018e-6;
const C67: f32 = 0.0018e-6;
const R201: f32 = 22.0e3;
const R202: f32 = 47.0e3;
const C69: f32 = 0.01e-6;
const R205: f32 = 27.0e3;

// Output mixer IC13: R210 ∥ the level pot at maximum (100 kΩ) ∥ C70.
const R_LEVEL: f32 = 470.0e3 * 100.0e3 / (470.0e3 + 100.0e3);
const C70: f32 = 220.0e-12;

/// Scales the output so an unaccented hit at stock settings peaks near 5 V.
const OUTPUT_GAIN: f32 = 2.925;

/// The circuit's state for one hit sequence.
#[derive(Clone, Copy, Default)]
struct Voice<L: Lanes = Mono> {
    pulse: Pulse<L>,
    /// Voltage across C57.
    c57: L::S,
    low: Arms<L>,
    high: Arms<L>,
    /// Voltages across C63 and C65.
    c63: L::S,
    c65: L::S,
    /// Voltages across C51 and C52.
    c51: L::S,
    c52: L::S,
    /// White noise shaped to the chopper's spectrum; it only reaches the
    /// output while C52 is above `CHOP_V0`.
    chop: L::S,
    snap_hp: BiquadState<L>,
    snap_couple: L::S,
    out_lp: L::S,
}

lanewise!(Voice {
    pulse,
    c57,
    low,
    high,
    c63,
    c65,
    c51,
    c52,
    chop,
    snap_hp,
    snap_couple,
    out_lp,
});

/// Brings a fully accented hit to twice the level of an unaccented one.
const ACCENT_TRIM: f32 = 0.937;

impl Fire for Voice {
    fn accent_trim(_: &Sd808State) -> f32 {
        ACCENT_TRIM
    }

    fn fire(&mut self, accent: f32, st: &Sd808State) {
        self.pulse
            .fire(pulse_volts(accent), st.timing.trigger_samples);
    }
}

impl<L: Lanes> Circuit<L> for Voice<L> {
    type Shared = Sd808State;
    type Controls = Controls<L>;

    fn settled(&self) -> L::SM {
        let silent = |v: L::S| v.abs().lt(c(SILENCE));
        (!self.pulse.active())
            .and_then(|| self.low.settled())
            .and_then(|| self.high.settled())
            .and_then(|| self.snap_hp.settled())
            .and_then(|| silent(self.c57) & silent(self.c63) & silent(self.c65))
            .and_then(|| silent(self.c51) & silent(self.c52))
            .and_then(|| silent(self.snap_couple) & silent(self.out_lp))
    }

    fn step(&mut self, st: &Sd808State, controls: &Controls<L>) -> L::S {
        self.advance(st, controls) * controls.level
    }
}

/// The snare at every width.
struct Snare;

impl Drum for Snare {
    type Shared = Sd808State;
    type Circuit<L: Lanes> = Voice<L>;
    type Controls<L: Lanes> = Controls<L>;
    type Sources<L: Lanes> = ();
    const RUNS_IN_QUADS: bool = true;
    const RUNS_IN_PAIRS: bool = false;
}

#[derive(Default)]
struct Sd808State {
    timing: Timing,
    dt: f32,
    /// The low resonator's resting frequency, Hz, and both resonators'
    /// stock ring times, seconds.
    rest_hz: f32,
    low_decay: f32,
    high_decay: f32,
    noise_scale: f32,
    /// C57's per-sample decay toward its resting share of the pulse, and the
    /// mean of that decay over one sample.
    c57_mul: f32,
    c57_avg: f32,
    chop_coeff: f32,
    /// Scales the shaped noise to unit rms.
    chop_norm: f32,
    snap_hp: Biquad,
    snap_couple_coeff: f32,
    out_lp_coeff: f32,
}

/// Both resonators tuned for one `freq`, and the output gain levelling it.
/// `freq` scales both Ts and keeps their stock ring times.
#[derive(Clone, Copy, Default)]
struct Tuning {
    low: BridgedT,
    high: BridgedT,
    level: f32,
}

impl Tuning {
    fn new(st: &Sd808State, freq: f32) -> Self {
        let r = voct_to_hz(freq) / st.rest_hz;
        Self {
            low: LOW.tuned(r, st.low_decay),
            high: HIGH.tuned(r, st.high_decay),
            level: levels::SNARE.gain(freq),
        }
    }
}

/// The front-panel settings for one sample.
#[derive(Clone, Copy, Default)]
struct Controls<L: Lanes = Mono> {
    low: BridgedT<L>,
    high: BridgedT<L>,
    /// Tone and snappy pot positions, 0–1.
    tone: L::S,
    snappy: L::S,
    /// The white noise generator's voltage.
    white: L::S,
    /// Output gain levelling `freq`.
    level: L::S,
}

lanewise!(Controls {
    low,
    high,
    tone,
    snappy,
    white,
    level
});

impl<L: Lanes> Voice<L> {
    /// Advances the circuit one sample, returning IC13's output.
    #[inline]
    fn advance(&mut self, st: &Sd808State, ctl: &Controls<L>) -> L::S {
        let dt = st.dt;
        let vtrig = self.pulse.next();

        // Low resonator. C57 settles within microseconds to its share of the
        // pulse across R189, so IC14a's input is the remainder: a spike at
        // each edge, averaged over the sample to keep its energy exact.
        let c57_rest = vtrig * R189 / (R189 + R190);
        let vp_low = vtrig - (c57_rest + (self.c57 - c57_rest) * st.c57_avg);
        self.c57 = c57_rest + (self.c57 - c57_rest) * st.c57_mul;
        let (low, arms) = ctl.low.step(self.low, vp_low, c(1.0 / R196), c(0.0), dt);
        self.low = arms;

        let vp_high = vtrig * R192 / (R191 + R192);
        let (high, arms) = ctl.high.step(self.high, vp_high, c(1.0 / R195), c(0.0), dt);
        self.high = arms;

        let tone_current = self.tone(ctl.tone, low, high, dt);

        // Snappy envelope: Q47 follows the pot's share of the pulse onto
        // C51, then C51 and C52 bleed into Q48's collector node.
        let wiper = vtrig * (ctl.snappy * VR9 + R199) / (R199 + VR9);
        self.c51 = self.c51.max(wiper - Q47_VBE);
        let chopping = (self.c52 - CHOP_V0).max(c(0.0)).gt(c(0.0));
        let g_load = L::S::select(chopping, c((1.0 - CHOP_MEAN) / R185), c(0.0));
        let g_leak = L::S::select(chopping, c(0.0), c(1.0 / LEAK));
        // Backward Euler on (C51, C52), with R185's share of the load as a
        // conductance just below `CHOP_V0`, or past it Q48's leak to ground.
        let (h1, h2) = (dt / (R184 * C51), dt / (R184 * C52));
        let g2 = (g_load + g_leak) * dt / C52;
        let (a11, a12, a21, a22) = (1.0 + h1, -h1, -h2, g2 + (1.0 + h2));
        let sink = CHOP_V0 - CHOP_SINK;
        let (b1, b2) = (self.c51, self.c52 + g_load * dt * sink / C52);
        let det = a22 * a11 - a12 * a21;
        self.c51 = (a22 * b1 - b2 * a12) / det;
        self.c52 = (b2 * a11 - b1 * a21) / det;

        self.chop += (ctl.white * st.chop_norm - self.chop) * st.chop_coeff;
        let span = (self.c52 - CHOP_V0).max(c(0.0));
        let collector = self.c52.min(c(CHOP_V0)) + span * (self.chop * CHOP_RMS + CHOP_MEAN);
        let thinned = st.snap_hp.process(&mut self.snap_hp, collector);
        let snap_current =
            high_pass(&mut self.snap_couple, thinned, c(st.snap_couple_coeff)) / R205;

        let mixed = low_pass(
            &mut self.out_lp,
            tone_current + snap_current,
            c(st.out_lp_coeff),
        );
        mixed * -R_LEVEL
    }

    /// The tone network's current into IC13 for this sample, given both
    /// resonators' outputs.
    fn tone(&mut self, tone: L::S, low: L::S, high: L::S, dt: f32) -> L::S {
        // The wiper node W is resistive: with e = (low − v63, high − v65),
        // currents i = G·e flow through the pot's two halves.
        let r_low = (tone * VR8).max(c(1.0));
        let r_high = (c::<L::S>(1.0) - tone) * VR8 + R200;
        let (g_low, g_high) = (c::<L::S>(1.0) / r_low, c::<L::S>(1.0) / r_high);
        let rp = c::<L::S>(1.0) / (g_low + 1.0 / R204 + g_high);
        let g11 = g_low - rp / (r_low * r_low);
        let g12 = -rp / (r_low * r_high);
        let g22 = g_high - rp / (r_high * r_high);
        // Backward Euler on (v63, v65): v' = D·G·(o − v).
        let (d1, d2) = (dt / C63, dt / C65);
        let (a11, a12, a21, a22) = (g11 * d1 + 1.0, g12 * d1, g12 * d2, g22 * d2 + 1.0);
        let b1 = self.c63 + (g11 * low + g12 * high) * d1;
        let b2 = self.c65 + (g12 * low + g22 * high) * d2;
        let det = a11 * a22 - a12 * a21;
        self.c63 = (a22 * b1 - a12 * b2) / det;
        self.c65 = (a11 * b2 - a21 * b1) / det;
        let wiper = rp * ((low - self.c63) / r_low + (high - self.c65) / r_high);
        wiper / R204
    }
}
