//! TR-808 toms and congas, simulated from the 808 service notes' voicing
//! board schematic. Part names (R228, C76, …) are the low tom's; the mid and
//! high voices use the same circuit with their own capacitors.
//!
//! The trigger pulse is differentiated into IC15's non-inverting input, which
//! drives a bridged-T network in its feedback path. An inverting buffer feeds
//! the ring back into the T's junction through R230, which sets the decay.
//! A germanium diode pair across part of the tuning pot conducts on large
//! swings and lowers the T's resistance to ground, so each hit starts sharp
//! and falls to its resting pitch. The toms add pink noise through a
//! transistor gate that opens a few milliseconds after the hit; the congas
//! ground it.

use std::sync::OnceLock;

use deserr::Deserr;
use schemars::JsonSchema;

use super::circuit::{
    Arms, Biquad, BiquadState, BridgedT, SILENCE, Timing, WhiteNoise, pulse_volts,
};
use crate::dsp::drums::analog::{VT, high_pass, low_pass, one_pole_coeff, rc_hz};
use crate::dsp::drums::voice::{Circuit, Pulse, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::dsp::utils::{sanitize, voct_to_hz};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

mod modules;
mod parts;
mod tuning;

pub use modules::{Hc808, Ht808, Lc808, Lt808, Mc808, Mt808};
use parts::*;
use tuning::*;

// Bridged-T: C76 from IC15's inverting input to the junction, C78 (with C77
// switched in for the tom) from the junction to the output, R228 bridging,
// and R231 into the 500 Ω tuning pot to ground, set to its centre.
const R228: f32 = 820.0e3;
const R230: f32 = 820.0e3;
const R231: f32 = 1.0e3;
const POT_A: f32 = 250.0;
const POT_B: f32 = 250.0;
// D80/D81 (1S188FM germanium) from the junction to the pot's far end.
const GE_IS: f32 = 200.0e-9;
const GE_NVT: f32 = 1.3 * VT;

// Output mixer IC13: the ring and the noise through 100 kΩ each into C81,
// with R235 ∥ the level pot at maximum (100 kΩ) ∥ C83 as feedback.
const R_MIX: f32 = 100.0e3;
const R_LEVEL: f32 = 470.0e3 * 100.0e3 / (470.0e3 + 100.0e3);
const C83: f32 = 1.0e-9;

// Noise gate: the trigger charges C72 through R219 and D55, and C72 bleeds
// through R218 into Q52's base, which also takes pink noise through C71 from
// the R216/R217 divider. Q52's collector current through R221 is the gated
// noise, coupled out through C74 into R223 and smoothed by R222 and C75.
const R216: f32 = 15.0e3;
const R217: f32 = 3.3e3;
const R218: f32 = 2.2e6;
const R219: f32 = 4.7e3;
const R220: f32 = 33.0e3;
const R221: f32 = 4.7e3;
const R222: f32 = 22.0e3;
const R223: f32 = 22.0e3;
const Q52_BETA: f32 = 300.0;
const Q52_IS: f32 = 10.0e-15;
const VCC: f32 = 15.0;
/// Once the hit's charge has bled away, C71 holds Q52's base near 0.45 V for
/// many seconds, leaking noise some 80 dB down. The gate fades that out
/// as the base-emitter voltage falls from `Q52_OFF + Q52_FADE` to
/// `Q52_OFF`, where it counts as closed.
const Q52_OFF: f32 = 0.44;
const Q52_FADE: f32 = 0.03;
/// Forward drop of D55 while it charges C72.
const D55_DROP: f32 = 0.6;
// The pink noise source: a unity-gain Sallen-Key low-pass (R133, R132, C32,
// C33) on the white noise generator.
const PINK_R: f32 = 22.0e3;
const PINK_C_FEEDBACK: f32 = 0.039e-6;
const PINK_C_GROUND: f32 = 0.018e-6;

/// D56 holds IC15's input divider at most this far below ground.
const D56_DROP: f32 = 0.6;

/// The circuit's state for one hit sequence.
#[derive(Clone, Copy)]
struct Voice {
    pulse: Pulse,
    /// Voltage across the input coupling capacitor.
    c_in: f32,
    arms: Arms,
    /// Junction voltage from the last step.
    junction: f32,
    /// Noise gate: C72, C71, Q52's base-emitter voltage, and the pink
    /// noise, collector coupling and smoothing filters.
    env: f32,
    couple: f32,
    vbe: f32,
    pink: BiquadState,
    gate_hp: f32,
    gate_lp: f32,
    out_hp: f32,
    out_lp: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            pulse: Pulse::default(),
            c_in: 0.0,
            arms: Arms::default(),
            junction: 0.0,
            env: 0.0,
            couple: 0.0,
            vbe: 0.0,
            pink: BiquadState::default(),
            gate_hp: 0.0,
            gate_lp: 0.0,
            out_hp: 0.0,
            out_lp: 0.0,
        }
    }
}

/// Per-sample-rate constants for one voice.
#[derive(Default)]
struct TomState {
    timing: Timing,
    dt: f32,
    noise: Option<&'static NoiseParts>,
    tuning: &'static [f32],
    noise_scale: f32,
    pink: Biquad,
    rest_hz: f32,
    /// Input capacitor's per-sample decay and the share of its step that
    /// reaches IC15.
    in_mul: f32,
    in_avg: f32,
    in_share: f32,
    env_charge: f32,
    gate_hp_coeff: f32,
    gate_lp_coeff: f32,
    out_hp_coeff: f32,
    out_lp_coeff: f32,
}

impl TomState {
    fn init(&mut self, parts: &'static Parts, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        self.dt = dt;
        self.tuning = tuning_table();
        self.timing = Timing::new(sample_rate);
        self.noise = parts.noise.as_ref();
        self.noise_scale = WhiteNoise::scale(sample_rate);
        self.pink =
            Biquad::sallen_key_low(PINK_R, PINK_R, PINK_C_FEEDBACK, PINK_C_GROUND, sample_rate);
        self.rest_hz = rest_hz(parts);
        let tau = parts.c_in * (parts.r_series + parts.r_shunt);
        self.in_mul = (-dt / tau).exp();
        self.in_avg = tau / dt * (1.0 - self.in_mul);
        self.in_share = parts.r_shunt / (parts.r_series + parts.r_shunt);
        if let Some(n) = &parts.noise {
            self.env_charge = 1.0 - (-dt / (R219 * n.c_env)).exp();
            self.gate_hp_coeff = one_pole_coeff(rc_hz(R221 + R223, n.c_hp), sample_rate);
            self.gate_lp_coeff =
                one_pole_coeff(rc_hz(R222 + parallel(R221, R223), n.c_lp), sample_rate);
        }
        self.out_hp_coeff = one_pole_coeff(rc_hz(0.5 * R_MIX, parts.c_out), sample_rate);
        self.out_lp_coeff = one_pole_coeff(rc_hz(R_LEVEL, C83), sample_rate);
    }
}

/// One output channel: its voices, noise generator, and the bridged-T tuned
/// for the current `freq` and `decay`.
#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Voice>,
    noise: WhiteNoise,
    network: Network,
}

/// The settings for one sample.
struct Controls {
    t: BridgedT,
    /// The white noise generator's voltage. Gated noise keeps its generator
    /// running between hits, so the gate's first sample after a strike draws
    /// fresh noise.
    white: f32,
}

impl Circuit for Voice {
    type Shared = TomState;
    type Controls = Controls;

    fn fire(&mut self, accent: f32, st: &TomState) {
        self.pulse
            .fire(pulse_volts(accent), st.timing.pulse_samples);
    }

    fn settled(&self) -> bool {
        !self.pulse.active()
            && self.arms.settled()
            && self.vbe < Q52_OFF
            && [
                self.c_in,
                self.gate_hp,
                self.gate_lp,
                self.out_hp,
                self.out_lp,
            ]
            .iter()
            .all(|v| v.abs() < SILENCE)
    }

    /// Advances the circuit one sample, returning the output mixer's voltage.
    fn step(&mut self, st: &TomState, c: &Controls) -> f32 {
        let vtrig = self.pulse.next();
        let out = self.ring(st, &c.t, vtrig);
        let noise = match st.noise {
            Some(n) => self.gate(n, st, vtrig, c.white),
            None => 0.0,
        };

        // Output mixer: the ring and noise meet through equal resistors, so
        // their average drives C81 into IC13's virtual ground.
        let mix = 0.5 * (out + noise);
        let hp = high_pass(&mut self.out_hp, mix, st.out_hp_coeff);
        -2.0 * (R_LEVEL / R_MIX) * low_pass(&mut self.out_lp, hp, st.out_lp_coeff)
    }
}

impl Voice {
    /// Advances the input network and bridged-T one sample, returning
    /// IC15's output.
    #[inline]
    fn ring(&mut self, st: &TomState, t: &BridgedT, vtrig: f32) -> f32 {
        let dt = st.dt;
        // C73 charges through the divider, so IC15 sees a spike only tens of
        // microseconds long; its average over the sample keeps the energy it
        // delivers exact. D56 stops the falling edge a diode drop below
        // ground, discharging C73 as it does.
        let across = vtrig - self.c_in;
        let vp = if across < -D56_DROP {
            self.c_in = vtrig + D56_DROP;
            -D56_DROP * st.in_share
        } else {
            self.c_in = vtrig - across * st.in_mul;
            st.in_share * across * st.in_avg
        };

        // The tuning network's current is linearised about the junction's
        // new voltage, refined once, so the diodes join the implicit solve as
        // a conductance.
        let mut junction = self.junction;
        let mut solved = (0.0, self.arms);
        for _ in 0..2 {
            let (i_tune, g_tune) = tuning_current(st.tuning, junction);
            solved = t.step(self.arms, vp, g_tune, i_tune - g_tune * junction, dt);
            junction = solved.0 + solved.1.b;
        }
        let (out, arms) = solved;
        self.arms = arms;
        self.junction = sanitize(junction);
        out
    }

    /// The noise gate's output at C75 for this sample.
    fn gate(&mut self, n: &NoiseParts, st: &TomState, vtrig: f32, white: f32) -> f32 {
        let dt = st.dt;
        if vtrig - D55_DROP > self.env {
            self.env += (vtrig - D55_DROP - self.env) * st.env_charge;
        }
        let pink = if self.vbe < Q52_OFF {
            0.0
        } else {
            st.pink.process(&mut self.pink, white)
        };
        let thevenin = pink * R217 / (R216 + R217);
        let r_th = parallel(R216, R217);

        // Q52's base node: C72's bleed through R218, C71 from the noise
        // divider, and the base current through R220. Newton on the
        // base-emitter voltage from the last sample's value, with steps
        // limited so the junction's exponential cannot overshoot.
        let is_b = Q52_IS / Q52_BETA;
        let mut vbe = self.vbe;
        for _ in 0..4 {
            let e = (vbe / VT).exp();
            let ib = is_b * (e - 1.0);
            let dib = is_b * e / VT;
            let base = vbe + R220 * ib;
            let f = (self.env - base) / R218 + (thevenin + self.couple - base) / r_th - ib;
            let df = -(1.0 + R220 * dib) * (1.0 / R218 + 1.0 / r_th) - dib;
            vbe -= (f / df).clamp(-4.0 * VT, 4.0 * VT);
        }
        vbe = vbe.min(0.9);
        self.vbe = vbe;
        let ib = is_b * ((vbe / VT).exp() - 1.0);
        let base = vbe + R220 * ib;
        // Backward Euler for C71 (its time constant is a few samples) and
        // C72's slow bleed.
        let h = dt / (r_th * n.c_couple);
        self.couple = (self.couple - h * (thevenin - base)) / (1.0 + h);
        self.env -= dt * (self.env - base) / (R218 * n.c_env);

        let open = ((vbe - Q52_OFF) / Q52_FADE).clamp(0.0, 1.0);
        let drop = open * (Q52_BETA * ib * R221).min(VCC - 0.2);
        let coupled = high_pass(&mut self.gate_hp, -drop, st.gate_hp_coeff);
        low_pass(
            &mut self.gate_lp,
            coupled * R223 / (R221 + R223),
            st.gate_lp_coeff,
        )
    }
}

/// Runs one block of samples for a voice's channels.
#[allow(clippy::too_many_arguments)]
fn process(
    parts: &Parts,
    st: &TomState,
    channels: &mut [Channel],
    trigger: &PolySignal,
    accent: &Option<PolySignal>,
    freq: &Option<PolySignal>,
    decay: &Option<PolySignal>,
    choke: bool,
    default_freq: f32,
    default_decay: f32,
    output: &mut PolyOutput,
) {
    for (ch, cs) in channels.iter_mut().enumerate() {
        let freq = freq.value_or(ch, default_freq);
        let decay = decay.value_or(ch, default_decay);
        if freq != cs.network.freq || decay != cs.network.decay {
            cs.network = Network::new(parts, freq, decay, st.rest_hz);
        }
        let controls = Controls {
            t: cs.network.t,
            white: cs.noise.next(st.noise_scale),
        };
        let out = cs.voices.process(
            trigger.get_value(ch),
            accent,
            ch,
            choke,
            st.timing.choke_samples,
            st,
            &controls,
        );
        output.set(ch, out * parts.gain);
    }
}
