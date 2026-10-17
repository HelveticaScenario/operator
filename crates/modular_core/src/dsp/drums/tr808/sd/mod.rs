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
    Arms, Biquad, BiquadState, BridgedT, SILENCE, T60_TO_RATE, Timing, WhiteNoise, pulse_volts,
};
use crate::dsp::drums::analog::{high_pass, low_pass, one_pole_coeff, rc_hz};
use crate::dsp::drums::voice::{Circuit, Pulse, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

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
const OUTPUT_GAIN: f32 = 2.89;

const DEFAULT_TONE: f32 = 2.5;
const DEFAULT_SNAPPY: f32 = 2.5;
/// The low resonator's frequency, ≈ 250 Hz, in V/Oct.
const DEFAULT_FREQ: f32 = -0.0678;

/// The circuit's state for one hit sequence.
#[derive(Clone, Copy, Default)]
struct Voice {
    pulse: Pulse,
    /// Voltage across C57.
    c57: f32,
    low: Arms,
    high: Arms,
    /// Voltages across C63 and C65.
    c63: f32,
    c65: f32,
    /// Voltages across C51 and C52.
    c51: f32,
    c52: f32,
    /// White noise shaped to the chopper's spectrum; it only reaches the
    /// output while C52 is above `CHOP_V0`.
    chop: f32,
    snap_hp: BiquadState,
    snap_couple: f32,
    out_lp: f32,
}

impl Circuit for Voice {
    type Shared = Sd808State;
    type Controls = Controls;

    fn fire(&mut self, accent: f32, st: &Sd808State) {
        self.pulse
            .fire(pulse_volts(accent), st.timing.pulse_samples);
    }

    fn settled(&self) -> bool {
        !self.pulse.active()
            && self.low.settled()
            && self.high.settled()
            && self.snap_hp.settled()
            && [
                self.c57,
                self.c63,
                self.c65,
                self.c51,
                self.c52,
                self.snap_couple,
                self.out_lp,
            ]
            .iter()
            .all(|v| v.abs() < SILENCE)
    }

    fn step(&mut self, st: &Sd808State, c: &Controls) -> f32 {
        self.advance(st, c)
    }
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

/// Both resonators tuned for one `freq`. `freq` scales both Ts and keeps
/// their stock ring times.
#[derive(Clone, Copy)]
struct Tuning {
    freq: f32,
    low: BridgedT,
    high: BridgedT,
}

impl Tuning {
    fn new(st: &Sd808State, freq: f32) -> Self {
        let r = voct_to_hz(freq) / st.rest_hz;
        Self {
            freq,
            low: LOW.tuned(r, st.low_decay),
            high: HIGH.tuned(r, st.high_decay),
        }
    }
}

/// The front-panel settings for one sample.
struct Controls {
    low: BridgedT,
    high: BridgedT,
    /// Tone and snappy pot positions, 0–1.
    tone: f32,
    snappy: f32,
    /// The white noise generator's voltage.
    white: f32,
}

impl Voice {
    /// Advances the circuit one sample, returning IC13's output.
    #[inline]
    fn advance(&mut self, st: &Sd808State, c: &Controls) -> f32 {
        let dt = st.dt;
        let vtrig = self.pulse.next();

        // Low resonator. C57 settles within microseconds to its share of the
        // pulse across R189, so IC14a's input is the remainder: a spike at
        // each edge, averaged over the sample to keep its energy exact.
        let c57_rest = vtrig * R189 / (R189 + R190);
        let vp_low = vtrig - (c57_rest + (self.c57 - c57_rest) * st.c57_avg);
        self.c57 = c57_rest + (self.c57 - c57_rest) * st.c57_mul;
        let (low, arms) = c.low.step(self.low, vp_low, 1.0 / R196, 0.0, dt);
        self.low = arms;

        let vp_high = vtrig * R192 / (R191 + R192);
        let (high, arms) = c.high.step(self.high, vp_high, 1.0 / R195, 0.0, dt);
        self.high = arms;

        let tone_current = self.tone(c.tone, low, high, dt);

        // Snappy envelope: Q47 follows the pot's share of the pulse onto
        // C51, then C51 and C52 bleed into Q48's collector node.
        let wiper = vtrig * (R199 + VR9 * c.snappy) / (R199 + VR9);
        self.c51 = self.c51.max(wiper - Q47_VBE);
        let span = (self.c52 - CHOP_V0).max(0.0);
        let (g_load, g_leak) = if span > 0.0 {
            ((1.0 - CHOP_MEAN) / R185, 0.0)
        } else {
            (0.0, 1.0 / LEAK)
        };
        // Backward Euler on (C51, C52), with R185's share of the load as a
        // conductance just below `CHOP_V0`, or past it Q48's leak to ground.
        let (h1, h2) = (dt / (R184 * C51), dt / (R184 * C52));
        let g2 = dt * (g_load + g_leak) / C52;
        let (a11, a12, a21, a22) = (1.0 + h1, -h1, -h2, 1.0 + h2 + g2);
        let sink = CHOP_V0 - CHOP_SINK;
        let (b1, b2) = (self.c51, self.c52 + dt * g_load * sink / C52);
        let det = a11 * a22 - a12 * a21;
        self.c51 = (a22 * b1 - a12 * b2) / det;
        self.c52 = (a11 * b2 - a21 * b1) / det;

        self.chop += st.chop_coeff * (c.white * st.chop_norm - self.chop);
        let span = (self.c52 - CHOP_V0).max(0.0);
        let collector = self.c52.min(CHOP_V0) + span * (CHOP_MEAN + CHOP_RMS * self.chop);
        let thinned = st.snap_hp.process(&mut self.snap_hp, collector);
        let snap_current = high_pass(&mut self.snap_couple, thinned, st.snap_couple_coeff) / R205;

        let mixed = low_pass(
            &mut self.out_lp,
            tone_current + snap_current,
            st.out_lp_coeff,
        );
        -R_LEVEL * mixed
    }

    /// The tone network's current into IC13 for this sample, given both
    /// resonators' outputs.
    fn tone(&mut self, tone: f32, low: f32, high: f32, dt: f32) -> f32 {
        // The wiper node W is resistive: with e = (low − v63, high − v65),
        // currents i = G·e flow through the pot's two halves.
        let r_low = (VR8 * tone).max(1.0);
        let r_high = VR8 * (1.0 - tone) + R200;
        let rp = 1.0 / (1.0 / R204 + 1.0 / r_low + 1.0 / r_high);
        let g11 = 1.0 / r_low - rp / (r_low * r_low);
        let g12 = -rp / (r_low * r_high);
        let g22 = 1.0 / r_high - rp / (r_high * r_high);
        // Backward Euler on (v63, v65): v' = D·G·(o − v).
        let (d1, d2) = (dt / C63, dt / C65);
        let (a11, a12, a21, a22) = (1.0 + d1 * g11, d1 * g12, d2 * g12, 1.0 + d2 * g22);
        let b1 = self.c63 + d1 * (g11 * low + g12 * high);
        let b2 = self.c65 + d2 * (g12 * low + g22 * high);
        let det = a11 * a22 - a12 * a21;
        self.c63 = (a22 * b1 - a12 * b2) / det;
        self.c65 = (a11 * b2 - a21 * b1) / det;
        let wiper = rp * ((low - self.c63) / r_low + (high - self.c65) / r_high);
        wiper / R204
    }
}
