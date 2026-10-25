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
    Arms, Biquad, BiquadState, BridgedT, PULSE_SECS, SILENCE, T60_TO_RATE, WhiteNoise,
    decay_around, pulse_volts,
};
use crate::dsp::drums::analog::{Exp, VT, converged, high_pass, low_pass, one_pole_coeff, rc_hz};
use crate::dsp::drums::group::{ChannelInputs, Drum, Groups, Run};
use crate::dsp::drums::input::{Cached, Hold};
use crate::dsp::drums::lanes::{Lanes, Lanewise, Mask, Mono, Real, c, lanewise};
use crate::dsp::drums::voice::{Circuit, Fire, Pulse, Timing, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PolyOutput, PolySignal};

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
#[derive(Clone, Copy, Default)]
struct Tom<L: Lanes = Mono> {
    pulse: Pulse<L>,
    /// Voltage across the input coupling capacitor.
    c_in: L::S,
    arms: Arms<L>,
    /// Junction voltage from the last step.
    junction: L::S,
    /// Noise gate: C72, C71, Q52's base-emitter voltage and its
    /// exponential, and the pink noise, collector coupling and smoothing
    /// filters.
    env: L::S,
    couple: L::S,
    vbe: L::S,
    q52: Exp<L>,
    pink: BiquadState<L>,
    gate_hp: L::S,
    gate_lp: L::S,
    out_hp: L::S,
    out_lp: L::S,
}

lanewise!(Tom {
    pulse,
    c_in,
    arms,
    junction,
    env,
    couple,
    vbe,
    q52,
    pink,
    gate_hp,
    gate_lp,
    out_hp,
    out_lp,
});

/// Per-sample-rate constants for one voice.
#[derive(Default)]
struct TomShared {
    timing: Timing,
    dt: f32,
    noise: Option<&'static NoiseParts>,
    accent_trim: f32,
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

impl TomShared {
    fn init(&mut self, parts: &'static Parts, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        self.dt = dt;
        self.accent_trim = parts.accent_trim;
        self.tuning = tuning_table();
        self.timing = Timing::new(PULSE_SECS, sample_rate);
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
    voices: Voices<Tom>,
    noise: WhiteNoise,
    network: Cached<(f32, f32), Network>,
}

/// The settings for one sample.
#[derive(Clone, Copy, Default)]
struct TomControls<L: Lanes = Mono> {
    t: BridgedT<L>,
    /// The white noise generator's voltage. Gated noise keeps its generator
    /// running between hits, so the gate's first sample after a strike draws
    /// fresh noise.
    white: L::S,
    /// Output gain levelling `freq`.
    level: L::S,
}

lanewise!(TomControls { t, white, level });

/// The toms and congas at every width.
struct TomDrum;

impl Drum for TomDrum {
    type Shared = TomShared;
    type Circuit<L: Lanes> = Tom<L>;
    type Controls<L: Lanes> = TomControls<L>;
    type Sources<L: Lanes> = ();
    const RUNS_IN_QUADS: bool = true;
    const RUNS_IN_PAIRS: bool = false;
}

impl Fire for Tom {
    fn accent_trim(sh: &TomShared) -> f32 {
        sh.accent_trim
    }

    fn fire(&mut self, accent: f32, sh: &TomShared) {
        self.pulse
            .fire(pulse_volts(accent), sh.timing.trigger_samples);
    }
}

impl<L: Lanes> Circuit<L> for Tom<L> {
    type Shared = TomShared;
    type Controls = TomControls<L>;

    fn settled(&self) -> L::SM {
        let silent = |v: L::S| v.abs().lt(c(SILENCE));
        (!self.pulse.active())
            .and_then(|| self.arms.settled())
            .and_then(|| self.vbe.lt(c(Q52_OFF)))
            .and_then(|| silent(self.c_in) & silent(self.gate_hp) & silent(self.gate_lp))
            .and_then(|| silent(self.out_hp) & silent(self.out_lp))
    }

    /// Advances the circuit one sample, returning the output mixer's voltage.
    fn step(&mut self, sh: &TomShared, ctl: &TomControls<L>) -> L::S {
        let vtrig = self.pulse.next();
        let out = self.ring(sh, &ctl.t, vtrig);
        let noise = match sh.noise {
            Some(n) => self.gate(n, sh, vtrig, ctl.white),
            None => c(0.0),
        };

        // Output mixer: the ring and noise meet through equal resistors, so
        // their average drives C81 into IC13's virtual ground.
        let mix = (out + noise) * 0.5;
        let hp = high_pass(&mut self.out_hp, mix, c(sh.out_hp_coeff));
        low_pass(&mut self.out_lp, hp, c(sh.out_lp_coeff)) * (-2.0 * (R_LEVEL / R_MIX)) * ctl.level
    }
}

impl<L: Lanes> Tom<L> {
    /// Advances the input network and bridged-T one sample, returning
    /// IC15's output.
    #[inline]
    fn ring(&mut self, sh: &TomShared, t: &BridgedT<L>, vtrig: L::S) -> L::S {
        let dt = sh.dt;
        // C73 charges through the divider, so IC15 sees a spike only tens of
        // microseconds long; its average over the sample keeps the energy it
        // delivers exact. D56 stops the falling edge a diode drop below
        // ground, discharging C73 as it does.
        let across = vtrig - self.c_in;
        let clipped = across.lt(c(-D56_DROP));
        let vp = L::S::select(
            clipped,
            c(-D56_DROP * sh.in_share),
            across * sh.in_share * sh.in_avg,
        );
        self.c_in = L::S::select(clipped, vtrig + D56_DROP, vtrig - across * sh.in_mul);

        // The tuning network's current is linearised about the junction's
        // new voltage, refined once, so the diodes join the implicit solve as
        // a conductance.
        let mut junction = self.junction;
        let mut solved = (c(0.0), self.arms);
        for _ in 0..2 {
            let (i_tune, g_tune) = tuning_current::<L>(sh.tuning, junction);
            solved = t.step(self.arms, vp, g_tune, i_tune - g_tune * junction, dt);
            junction = solved.0 + solved.1.b;
        }
        let (out, arms) = solved;
        self.arms = arms;
        self.junction = junction.sanitize();
        out
    }

    /// The noise gate's output at C75 for this sample.
    fn gate(&mut self, n: &NoiseParts, sh: &TomShared, vtrig: L::S, white: L::S) -> L::S {
        let dt = sh.dt;
        let charging = (vtrig - D55_DROP).gt(self.env);
        self.env = L::S::select(
            charging,
            self.env + (vtrig - D55_DROP - self.env) * sh.env_charge,
            self.env,
        );
        let gated = self.vbe.lt(c(Q52_OFF));
        let pink = if gated.all() {
            c(0.0)
        } else {
            let mut pink = self.pink;
            let y = sh.pink.process(&mut pink, white);
            self.pink = BiquadState::select(gated, self.pink, pink);
            L::S::select(gated, c(0.0), y)
        };
        let thevenin = pink * R217 / (R216 + R217);
        let r_th = parallel(R216, R217);

        // Q52's base node: C72's bleed through R218, C71 from the noise
        // divider, and the base current through R220. Newton on the
        // base-emitter voltage from the last sample's value, with steps
        // limited so the junction's exponential cannot overshoot.
        const DONE: f64 = converged(1.0e-6, VT as f64);
        let (vt, inv_vt) = (VT as f64, 1.0 / VT as f64);
        let is_b = (Q52_IS / Q52_BETA) as f64;
        let (r220, g218, g_th) = (R220 as f64, 1.0 / R218 as f64, 1.0 / r_th as f64);
        let source = L::wide(self.env) * g218 + L::wide(thevenin + self.couple) * g_th;
        let mut vbe = L::wide(self.vbe);
        let mut e = self.q52.at(vbe, inv_vt);
        let mut done = L::DM::default();
        for _ in 0..4 {
            let ib = (e.value - 1.0) * is_b;
            let dib = e.value * is_b * inv_vt;
            let base = vbe + ib * r220;
            let f = source - base * (g218 + g_th) - ib;
            let df = -(dib * r220 + 1.0) * (g218 + g_th) - dib;
            let step = (f / df).clamp(c(-4.0 * vt), c(4.0 * vt));
            vbe = L::D::select(done, vbe, vbe - step);
            e = e.at_where(!done, vbe, inv_vt);
            done = done | (step * step).lt(c(DONE));
            if done.all() {
                break;
            }
        }
        let vbe = vbe.min(c(0.9));
        let e = e.at(vbe, inv_vt);
        self.q52 = e;
        let vbe = L::narrow(vbe);
        self.vbe = vbe;
        let ib = L::narrow((e.value - 1.0) * is_b);
        let base = vbe + ib * R220;
        // Backward Euler for C71 (its time constant is a few samples) and
        // C72's slow bleed.
        let h = dt / (r_th * n.c_couple);
        self.couple = (self.couple - (thevenin - base) * h) / (1.0 + h);
        self.env -= (self.env - base) * dt / (R218 * n.c_env);

        let open = ((vbe - Q52_OFF) / Q52_FADE).clamp(c(0.0), c(1.0));
        let drop = open * (ib * Q52_BETA * R221).min(c(VCC - 0.2));
        let coupled = high_pass(&mut self.gate_hp, -drop, c(sh.gate_hp_coeff));
        low_pass(
            &mut self.gate_lp,
            coupled * R223 / (R221 + R223),
            c(sh.gate_lp_coeff),
        )
    }
}

/// A tom's per-sample-rate constants and its channel groups.
#[derive(Default)]
struct TomState {
    hold: Hold<1, 2>,
    shared: TomShared,
    groups: Groups<TomDrum>,
}
