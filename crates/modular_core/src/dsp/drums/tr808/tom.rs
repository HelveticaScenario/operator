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
    Arms, Biquad, BiquadState, BridgedT, PULSE_SECS, SILENCE, WhiteNoise, pulse_volts,
};
use crate::dsp::drums::analog::{VT, high_pass, low_pass, one_pole_coeff, rc_hz};
use crate::dsp::drums::voice::{CHOKE_SECS, Pulse, Strike, samples};
use crate::dsp::utils::rng::seed_base;
use crate::dsp::utils::{sanitize, voct_to_hz};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

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

/// Component values that differ between the six voices.
pub struct Parts {
    /// Input coupling capacitor (C73) and the divider it drives: the series
    /// resistance to IC15's input and the shunt to ground, with the TOM/CONGA
    /// switch's 1.5–2.2 kΩ resistor already placed.
    c_in: f32,
    r_series: f32,
    r_shunt: f32,
    /// Bridged-T arms (C76 and C78 with any switched capacitor).
    c_a: f32,
    c_b: f32,
    /// Output coupling capacitor (C81).
    c_out: f32,
    noise: Option<NoiseParts>,
    /// Scales the output so an unaccented hit at stock pitch peaks near 5 V.
    gain: f32,
}

/// The tom's noise gate capacitors: C72, C71, C74 and C75.
pub struct NoiseParts {
    c_env: f32,
    c_couple: f32,
    c_hp: f32,
    c_lp: f32,
}

const fn parallel(a: f32, b: f32) -> f32 {
    a * b / (a + b)
}

pub const LT: Parts = Parts {
    c_in: 6.8e-9,
    r_series: parallel(22.0e3, 1.5e3),
    r_shunt: 4.7e3,
    c_a: 0.056e-6,
    c_b: 0.012e-6 + 0.047e-6,
    c_out: 0.047e-6,
    noise: Some(NoiseParts {
        c_env: 0.039e-6,
        c_couple: 0.047e-6,
        c_hp: 0.047e-6,
        c_lp: 0.018e-6,
    }),
    gain: 1.859,
};

pub const LC: Parts = Parts {
    c_in: 6.8e-9,
    r_series: 22.0e3,
    r_shunt: parallel(4.7e3, 1.5e3),
    c_a: 0.056e-6,
    c_b: 0.012e-6,
    c_out: 0.047e-6,
    noise: None,
    gain: 3.356,
};

pub const MT: Parts = Parts {
    c_in: 2.7e-9,
    r_series: parallel(22.0e3, 2.2e3),
    r_shunt: 6.8e3,
    c_a: 0.027e-6 + 0.0082e-6,
    c_b: 0.012e-6 + 0.027e-6,
    c_out: 0.039e-6,
    noise: Some(NoiseParts {
        c_env: 0.033e-6,
        c_couple: 0.047e-6,
        c_hp: 0.033e-6,
        c_lp: 0.015e-6,
    }),
    gain: 2.203,
};

pub const MC: Parts = Parts {
    c_in: 2.7e-9,
    r_series: 22.0e3,
    r_shunt: parallel(6.8e3, 2.2e3),
    c_a: 0.027e-6,
    c_b: 0.012e-6,
    c_out: 0.039e-6,
    noise: None,
    gain: 5.102,
};

pub const HT: Parts = Parts {
    c_in: 2.7e-9,
    r_series: parallel(22.0e3, 1.5e3),
    r_shunt: 5.6e3,
    c_a: 0.027e-6,
    c_b: 0.0056e-6 + 0.022e-6,
    c_out: 0.033e-6,
    noise: Some(NoiseParts {
        c_env: 0.033e-6,
        c_couple: 0.047e-6,
        c_hp: 0.027e-6,
        c_lp: 0.01e-6,
    }),
    gain: 1.938,
};

pub const HC: Parts = Parts {
    c_in: 2.7e-9,
    r_series: 22.0e3,
    r_shunt: parallel(5.6e3, 1.5e3),
    c_a: 0.027e-6,
    c_b: 0.0056e-6,
    c_out: 0.033e-6,
    noise: None,
    gain: 3.125,
};

/// D56 holds IC15's input divider at most this far below ground.
const D56_DROP: f32 = 0.6;

/// Shortest and longest `decay` the feedback can reach, seconds.
const MIN_DECAY: f32 = 0.01;
const MAX_DECAY: f32 = 10.0;

/// The bridged-T tuned for one `freq` and `decay`.
#[derive(Clone, Copy)]
struct Network {
    freq: f32,
    decay: f32,
    t: BridgedT,
}

impl Default for Network {
    fn default() -> Self {
        // NaN never compares equal, so the first sample always computes.
        Self {
            freq: f32::NAN,
            decay: f32::NAN,
            t: BridgedT {
                c_a: 1.0,
                c_b: 1.0,
                r_bridge: R228,
                g_fb: 0.0,
            },
        }
    }
}

impl Network {
    fn new(parts: &Parts, freq: f32, decay: f32, rest_hz: f32) -> Self {
        let r = voct_to_hz(freq) / rest_hz;
        Self {
            freq,
            decay,
            t: stock_t(parts).tuned(r, decay.clamp(MIN_DECAY, MAX_DECAY)),
        }
    }
}

/// A voice's bridged-T as built.
fn stock_t(parts: &Parts) -> BridgedT {
    BridgedT {
        c_a: parts.c_a,
        c_b: parts.c_b,
        r_bridge: R228,
        g_fb: 1.0 / R230,
    }
}

/// Resting frequency of a voice's bridged-T, Hz.
fn rest_hz(parts: &Parts) -> f32 {
    stock_t(parts).rest_hz(1.0 / (R231 + POT_B))
}

/// Largest junction voltage the tuning table covers, and its resolution.
const TUNE_MAX: f32 = 16.0;
const TUNE_STEPS: usize = 2048;

/// Current through the tuning network for junction voltages 0–[`TUNE_MAX`].
/// The network is odd-symmetric, so negative voltages read it mirrored.
static TUNING: OnceLock<Box<[f32]>> = OnceLock::new();

fn tuning_table() -> &'static [f32] {
    TUNING.get_or_init(|| {
        (0..=TUNE_STEPS)
            .map(|j| tuning_current_exact(TUNE_MAX * j as f32 / TUNE_STEPS as f32))
            .collect()
    })
}

/// Current through R231, the pot and D80/D81 from a junction voltage `x`.
/// Newton in the diode current rather than its voltage converges from any
/// start, since the voltage is only logarithmic in the current.
fn tuning_current_exact(x: f32) -> f32 {
    let (x, k) = (x as f64, 1.0 + (POT_B / R231) as f64);
    let (is, nvt, pa, pb) = (GE_IS as f64, GE_NVT as f64, POT_A as f64, POT_B as f64);
    let (mut i, mut vd) = (0.0f64, 0.0f64);
    for _ in 0..100 {
        vd = nvt * (i / (2.0 * is)).asinh();
        let dvd = nvt / (i * i + 4.0 * is * is).sqrt();
        let f = (vd + i * pa) * k + pb * i - x;
        i -= f / ((dvd + pa) * k + pb);
    }
    ((x - vd - i * pa) / pb) as f32
}

/// The tuning network's current at `x` and its slope, interpolated.
#[inline]
fn tuning_current(table: &[f32], x: f32) -> (f32, f32) {
    let pos = x.abs().min(TUNE_MAX) * (TUNE_STEPS as f32 / TUNE_MAX);
    let j = (pos as usize).min(TUNE_STEPS - 1);
    let (lo, hi) = (table[j], table[j + 1]);
    let current = lo + (pos - j as f32) * (hi - lo);
    (
        current.copysign(x),
        (hi - lo) * (TUNE_STEPS as f32 / TUNE_MAX),
    )
}

/// The circuit's state for one hit sequence.
#[derive(Clone, Copy)]
struct Voice {
    pulse: Pulse,
    /// Voltage across the input coupling capacitor.
    c_in: f32,
    arms: Arms,
    /// Junction voltage from the last step.
    junction: f32,
    network: Network,
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
            network: Network::default(),
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
pub struct TomState {
    pulse_samples: u32,
    choke_samples: u32,
    dt: f32,
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
    seeded: bool,
}

impl TomState {
    pub fn init(&mut self, parts: &Parts, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        self.dt = dt;
        self.tuning = tuning_table();
        self.pulse_samples = samples(PULSE_SECS, sample_rate);
        self.choke_samples = samples(CHOKE_SECS, sample_rate);
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

/// One output channel: the strike detector, the sounding voice, and a choked
/// voice fading out under it.
#[derive(Clone, Copy, Default)]
pub struct Channel {
    strike: Strike,
    noise: WhiteNoise,
    voice: Voice,
    tail: Voice,
    tail_left: u32,
}

impl Voice {
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

    /// Advances the circuit one sample with white noise `white`, returning
    /// the output mixer's voltage.
    fn step(&mut self, parts: &Parts, st: &TomState, white: f32) -> f32 {
        let vtrig = self.pulse.next();
        let out = self.ring(st, vtrig);
        let noise = match &parts.noise {
            Some(n) => self.gate(n, st, vtrig, white),
            None => 0.0,
        };

        // Output mixer: the ring and noise meet through equal resistors, so
        // their average drives C81 into IC13's virtual ground.
        let mix = 0.5 * (out + noise);
        let hp = high_pass(&mut self.out_hp, mix, st.out_hp_coeff);
        -2.0 * (R_LEVEL / R_MIX) * low_pass(&mut self.out_lp, hp, st.out_lp_coeff)
    }

    /// Advances the input network and bridged-T one sample, returning
    /// IC15's output.
    #[inline]
    fn ring(&mut self, st: &TomState, vtrig: f32) -> f32 {
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
            solved = self
                .network
                .t
                .step(self.arms, vp, g_tune, i_tune - g_tune * junction, dt);
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
pub fn process(
    parts: &Parts,
    st: &mut TomState,
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
        if let Some(volts) = cs
            .strike
            .process(trigger.get_value(ch), accent, ch)
            .map(pulse_volts)
        {
            if choke && !cs.voice.settled() {
                cs.tail = cs.voice;
                cs.tail_left = st.choke_samples;
                cs.voice = Voice {
                    network: cs.voice.network,
                    ..Voice::default()
                };
            }
            cs.voice.pulse.fire(volts, st.pulse_samples);
        }
        let freq = freq.value_or(ch, default_freq);
        let decay = decay.value_or(ch, default_decay);
        for voice in [&mut cs.voice, &mut cs.tail] {
            if freq != voice.network.freq || decay != voice.network.decay {
                voice.network = Network::new(parts, freq, decay, st.rest_hz);
            }
        }
        // Gated noise keeps its generator running between hits, so the
        // gate's first sample after a strike draws fresh noise.
        let white = cs.noise.next(st.noise_scale);
        let mut out = if cs.voice.settled() {
            0.0
        } else {
            cs.voice.step(parts, st, white)
        };
        if cs.tail_left > 0 {
            let fade = cs.tail_left as f32 / st.choke_samples as f32;
            cs.tail_left -= 1;
            out += fade * cs.tail.step(parts, st, white);
        }
        output.set(ch, out * parts.gain);
    }
}

/// Seeds each channel's noise from the module's [`seed_base`], once per
/// instance so a patch update keeps the running streams.
pub fn seed(base: u64, st: &mut TomState, channels: &mut [Channel]) {
    if !st.seeded {
        for (ch, cs) in channels.iter_mut().enumerate() {
            cs.noise.seed(base, ch);
        }
        st.seeded = true;
    }
}

macro_rules! tom_module {
    (
        $(#[$doc:meta])*
        name = $name:literal,
        ident = $Struct:ident,
        parts = $parts:ident,
        freq = $freq:literal,
        freq_range = ($freq_min:literal, $freq_max:literal),
        decay = $decay:literal,
        freq_doc = $freq_doc:literal,
        decay_doc = $decay_doc:literal $(,)?
    ) => {
        paste::paste! {
            #[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
            #[serde(rename_all = "camelCase")]
            #[deserr(rename_all = camelCase, deny_unknown_fields)]
            struct [<$Struct Params>] {
                /// rising edges strike the drum
                #[signal(type = trig, range = (0.0, 5.0))]
                trigger: PolySignal,
                /// accent, 0–5 V, sampled on each trigger and held for the hit;
                /// raises the trigger pulse from 4 V to 8 V, for a hit up to
                /// twice as loud with a sharper pitch drop (default 0)
                #[signal(default = 0.0, range = (0.0, 5.0))]
                #[deserr(default)]
                accent: Option<PolySignal>,
                #[doc = $freq_doc]
                #[signal(type = pitch, default = $freq, range = ($freq_min, $freq_max))]
                #[deserr(default)]
                freq: Option<PolySignal>,
                #[doc = $decay_doc]
                #[signal(default = $decay, range = (0.05, 3.0))]
                #[deserr(default)]
                decay: Option<PolySignal>,
                /// fade out a still-ringing hit over 2 ms as the next one
                /// strikes, so every hit starts from rest; otherwise hits add
                /// to the ring, as on the hardware (default false)
                #[serde(default)]
                #[deserr(default)]
                choke: bool,
            }

            #[derive(Outputs, JsonSchema)]
            #[serde(rename_all = "camelCase")]
            struct [<$Struct Outputs>] {
                #[output("output", "drum output", default, range = (-10.0, 10.0))]
                sample: PolyOutput,
            }

            $(#[$doc])*
            #[module(name = $name, has_init, patch_update, args(trigger))]
            pub struct $Struct {
                outputs: [<$Struct Outputs>],
                params: [<$Struct Params>],
                state: TomState,
                channel_state: Box<[Channel]>,
            }

            impl $Struct {
                fn init(&mut self, sample_rate: f32) {
                    self.state.init(&$parts, sample_rate);
                }

                fn update(&mut self, _sample_rate: f32) {
                    let channels = self.channel_count();
                    let p = &self.params;
                    process(
                        &$parts,
                        &mut self.state,
                        &mut self.channel_state[..channels],
                        &p.trigger,
                        &p.accent,
                        &p.freq,
                        &p.decay,
                        p.choke,
                        $freq,
                        $decay,
                        &mut self.outputs.sample,
                    );
                }
            }

            impl crate::types::PatchUpdateHandler for $Struct {
                fn on_patch_update(&mut self) {
                    let base = seed_base(self);
                    seed(base, &mut self.state, &mut self.channel_state);
                }
            }

            message_handlers!(impl $Struct {});
        }
    };
}

tom_module! {
    /// TR-808 low tom.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its low tom: a bridged-T filter rings at ≈87 Hz,
    /// starting sharp and settling as its diodes stop conducting, over a burst
    /// of low pink noise that swells in a few milliseconds after the hit.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.lt($clock.beatTrigger).out()
    /// ```
    name = "$tr808.lt",
    ident = Lt808,
    parts = LT,
    freq = -1.5959,
    freq_range = (-3.1, -0.1),
    decay = 0.634,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈87 Hz (default -1.5959)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.63 s (default 0.634)",
}

tom_module! {
    /// TR-808 mid tom.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its mid tom: a bridged-T filter rings at ≈134 Hz,
    /// starting sharp and settling as its diodes stop conducting, over a burst
    /// of low pink noise that swells in a few milliseconds after the hit.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.mt($clock.beatTrigger).out()
    /// ```
    name = "$tr808.mt",
    ident = Mt808,
    parts = MT,
    freq = -0.9623,
    freq_range = (-2.46, 0.54),
    decay = 0.399,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈134 Hz (default -0.9623)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.40 s (default 0.399)",
}

tom_module! {
    /// TR-808 high tom.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its high tom: a bridged-T filter rings at ≈182 Hz,
    /// starting sharp and settling as its diodes stop conducting, over a burst
    /// of low pink noise that swells in a few milliseconds after the hit.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.ht($clock.beatTrigger).out()
    /// ```
    name = "$tr808.ht",
    ident = Ht808,
    parts = HT,
    freq = -0.5216,
    freq_range = (-2.02, 0.98),
    decay = 0.306,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈182 Hz (default -0.5216)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.31 s (default 0.306)",
}

tom_module! {
    /// TR-808 low conga.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its low conga: a bridged-T filter rings at ≈192 Hz,
    /// starting sharp and settling as its diodes stop conducting.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.lc($clock.beatTrigger).out()
    /// ```
    name = "$tr808.lc",
    ident = Lc808,
    parts = LC,
    freq = -0.4471,
    freq_range = (-1.95, 1.05),
    decay = 0.634,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈192 Hz (default -0.4471)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.63 s (default 0.634)",
}

tom_module! {
    /// TR-808 mid conga.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its mid conga: a bridged-T filter rings at ≈276 Hz,
    /// starting sharp and settling as its diodes stop conducting.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.mc($clock.beatTrigger).out()
    /// ```
    name = "$tr808.mc",
    ident = Mc808,
    parts = MC,
    freq = 0.0792,
    freq_range = (-1.42, 1.58),
    decay = 0.306,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈276 Hz (default 0.0792)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.31 s (default 0.306)",
}

tom_module! {
    /// TR-808 high conga.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its high conga: a bridged-T filter rings at ≈405 Hz,
    /// starting sharp and settling as its diodes stop conducting.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.hc($clock.beatTrigger).out()
    /// ```
    name = "$tr808.hc",
    ident = Hc808,
    parts = HC,
    freq = 0.629,
    freq_range = (-0.87, 2.13),
    decay = 0.306,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈405 Hz (default 0.629)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.31 s (default 0.306)",
}
