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

use super::circuit::{DIODE_DROP, PULSE_SECS, SILENCE, T60_TO_RATE, VSAT, pulse_volts, trapezoid};
use crate::dsp::drums::analog::{high_pass, low_pass, one_pole_coeff};
use crate::dsp::drums::voice::{CHOKE_SECS, Pulse, Strike, samples};
use crate::dsp::utils::{sanitize, voct_to_hz};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

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

const MIN_DECAY: f32 = 0.01;
const MAX_DECAY: f32 = 10.0;

const DEFAULT_TONE: f32 = 2.5;
const DEFAULT_DECAY: f32 = 1.0;
/// Bridged-T centre frequency at rest, ≈ 49.45 Hz, in V/Oct.
const DEFAULT_FREQ: f32 = -2.4035;
/// Scales the circuit's output voltage so an unaccented hit at stock pitch
/// peaks near 5 V.
const OUTPUT_GAIN: f32 = 1.23;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Bd808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the trigger pulse from 4 V to 8 V, for a hit up to twice as loud with
    /// more pitch sigh (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// tone control, 0–5 V: opens the output low-pass from ≈300 Hz to ≈7 kHz,
    /// letting through more of the attack click (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// seconds for the ring to fall 60 dB; the stock decay knob spans
    /// ≈0.09–3.8 s, and the circuit can ring no shorter than ≈0.09 s at stock
    /// pitch (default 1)
    #[signal(default = 1.0, range = (0.1, 4.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// resting pitch of the bridged-T in V/Oct, scaling the whole circuit;
    /// stock is ≈49.5 Hz (default -2.4035)
    #[signal(type = pitch, default = -2.4035, range = (-3.5, -1.0))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// fade out a still-ringing hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the ring, as on the
    /// hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Bd808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// The circuit's state for one hit sequence.
#[derive(Clone, Copy)]
struct Voice {
    pulse: Pulse,
    /// Voltage across C40.
    shaper: f32,
    /// Voltage across C38; charged at rest, so Venv starts low.
    c38: f32,
    /// Voltage across C39, Venv side minus Vrp side.
    c39: f32,
    /// Voltages across C41 (inverting input to Vcomm) and C42 (Vcomm to Vbt).
    arm1: f32,
    arm2: f32,
    vcomm: f32,
    network: Network,
    /// `tone` the low-pass coefficient is computed for.
    tone: f32,
    tone_coeff: f32,
    tone_lp: f32,
    level_hp: f32,
    output_hp: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            pulse: Pulse::default(),
            shaper: 0.0,
            c38: RAIL,
            c39: 0.0,
            arm1: 0.0,
            arm2: 0.0,
            vcomm: 0.0,
            network: Network::default(),
            // NaN never compares equal, so the first sample always computes.
            tone: f32::NAN,
            tone_coeff: 0.0,
            tone_lp: 0.0,
            level_hp: 0.0,
            output_hp: 0.0,
        }
    }
}

impl Voice {
    fn settled(&self) -> bool {
        !self.pulse.active()
            && self.c38 > Q42_ON_START + Q42_ON_WIDTH
            && [
                self.shaper,
                self.c39,
                self.arm1,
                self.arm2,
                self.tone_lp,
                self.level_hp,
                self.output_hp,
            ]
            .iter()
            .all(|v| v.abs() < SILENCE)
    }
}

#[derive(Default)]
struct Bd808State {
    pulse_samples: u32,
    choke_samples: u32,
    /// Per-sample decay multipliers, which depend only on the sample rate.
    shaper_mul: f32,
    c38_coeff: f32,
    /// One sample's charge of C39 per volt across R161.
    c39_step: f32,
    level_hp_coeff: f32,
    output_hp_coeff: f32,
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
        let rate = T60_TO_RATE / decay.clamp(MIN_DECAY, MAX_DECAY) - branch_rate;
        let g_crit = R170 * (1.0 / R167 + c * a);
        let g = (R170 * (1.0 / R167 + c * (a - 2.0 * rate))).clamp(0.0, g_crit * 0.999);
        Self {
            freq,
            decay,
            c,
            a,
            g,
        }
    }
}

/// TR-808 bass drum.
///
/// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into a
/// circuit model of its bass drum: a resonant bridged-T filter rings at
/// ≈49.5 Hz with a short frequency jump on the attack and a falling pitch
/// "sigh". **decay** sets how long the ring lasts, **tone** how much of the
/// attack click comes through, and **accent**, sampled at each strike, drives
/// the circuit harder. Retriggering a ringing drum adds to its current state
/// rather than restarting it, as on the hardware, so long decays at fast
/// tempos swing in level from hit to hit; **choke** starts every hit from rest
/// instead.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near
/// **±10 V**; higher **freq** and **tone** run hotter, up to where the
/// circuit's op-amp clips.
///
/// ```js
/// $tr808.bd($clock.beatTrigger, { decay: 2 }).out()
/// ```
#[module(name = "$tr808.bd", has_init, args(trigger))]
pub struct Bd808 {
    outputs: Bd808Outputs,
    params: Bd808Params,
    state: Bd808State,
    channel_state: Box<[ChannelState]>,
}

#[derive(Clone, Copy, Default)]
struct ChannelState {
    strike: Strike,
    voice: Voice,
    /// A choked hit fading out under `voice` for `tail_left` more samples.
    tail: Voice,
    tail_left: u32,
}

impl Bd808 {
    fn init(&mut self, sample_rate: f32) {
        let st = &mut self.state;
        let dt = 1.0 / sample_rate;
        st.pulse_samples = samples(PULSE_SECS, sample_rate);
        st.choke_samples = samples(CHOKE_SECS, sample_rate);
        let shaper_tau = C40 * R162 * R163 / (R162 + R163);
        st.shaper_mul = (-dt / shaper_tau).exp();
        st.c38_coeff = 1.0 - (-dt / R156_C38).exp();
        st.c39_step = dt / (R161 * C39);
        st.level_hp_coeff = one_pole_coeff(LEVEL_HP_HZ, sample_rate);
        st.output_hp_coeff = one_pole_coeff(OUTPUT_HP_HZ, sample_rate);
        let rest_r = 1.0 / (1.0 / R161 + 1.0 / R170 + 1.0 / (R165 + R166));
        st.rest_hz = 1.0 / (std::f32::consts::TAU * (rest_r * R167).sqrt() * C41_42);
    }

    fn update(&mut self, sample_rate: f32) {
        let st = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];

            if let Some(volts) = cs
                .strike
                .process(p.trigger.get_value(ch), &p.accent, ch)
                .map(pulse_volts)
            {
                if p.choke && !cs.voice.settled() {
                    cs.tail = cs.voice;
                    cs.tail_left = st.choke_samples;
                    cs.voice = Voice {
                        network: cs.voice.network,
                        tone: cs.voice.tone,
                        tone_coeff: cs.voice.tone_coeff,
                        ..Voice::default()
                    };
                }
                cs.voice.pulse.fire(volts, st.pulse_samples);
            }
            let freq = p.freq.value_or(ch, DEFAULT_FREQ);
            let decay = p.decay.value_or(ch, DEFAULT_DECAY);
            let tone = p.tone.value_or(ch, DEFAULT_TONE);
            let mut out = if cs.voice.settled() {
                0.0
            } else {
                cs.voice.step(st, freq, decay, tone, sample_rate)
            };
            if cs.tail_left > 0 {
                let fade = cs.tail_left as f32 / st.choke_samples as f32;
                cs.tail_left -= 1;
                out += fade * cs.tail.step(st, freq, decay, tone, sample_rate);
            }
            self.outputs.sample.set(ch, out * OUTPUT_GAIN);
        }
    }
}

impl Voice {
    /// Advances the circuit one sample and returns the output stage's voltage.
    fn step(&mut self, st: &Bd808State, freq: f32, decay: f32, tone: f32, sample_rate: f32) -> f32 {
        let dt = 1.0 / sample_rate;
        let vtrig = self.pulse.next();

        // Pulse shaper: the shelf passes the edges, and D53 stops the
        // falling edge a diode drop below ground by discharging C40.
        let mut vplus = vtrig - self.shaper;
        if vplus < 0.0 {
            vplus = -DIODE_DROP * (1.0 - (vplus / DIODE_DROP).exp());
            self.shaper = vtrig - vplus;
        }
        let shaper_rest = vtrig * R163 / (R162 + R163);
        self.shaper = shaper_rest + (self.shaper - shaper_rest) * st.shaper_mul;

        // Envelope generator: the trigger pulse holds C38 empty, and Q43
        // grounds the R165/R166 junction for as long as Venv stays high.
        if vtrig > 0.0 {
            self.c38 = 0.0;
        } else {
            self.c38 += (RAIL - self.c38) * st.c38_coeff;
        }
        let q42 = ((self.c38 - Q42_ON_START) / Q42_ON_WIDTH).clamp(0.0, 1.0);
        let attack = 1.0 - q42 * q42 * (3.0 - 2.0 * q42);
        let venv = VENV_HIGH * attack;

        // Retriggering pulse: D52 shaves Venv's rising edge, so its
        // falling edge reaches the bridged-T as a large negative pulse.
        let mut vrp = venv - self.c39;
        if vrp > DIODE_DROP {
            vrp = DIODE_DROP;
            self.c39 = venv - DIODE_DROP;
        }
        self.c39 += (vrp - self.vcomm) * st.c39_step;

        // Q43 leakage on negative swings feeds Vcomm a current I(Vcomm)
        // that pulls it back towards ground. Linearised about the last
        // Vcomm as I0 − G·(Vcomm − V0), its slope G joins the implicit
        // solve as a conductance, which keeps it stable at any pitch.
        let (leak_g, leak_i) = if self.vcomm < LEAK_FLOOR {
            let x = LEAK_ALPHA * (-self.vcomm - LEAK_V0);
            let softplus = if x > 20.0 { x } else { x.exp().ln_1p() };
            let share = (1.0 - attack) * R165 / (R165 + R166);
            let current = share * (LEAK_M / LEAK_ALPHA) * softplus;
            let slope = share * LEAK_M / (1.0 + (-x).exp());
            (slope, current + slope * self.vcomm)
        } else {
            (0.0, 0.0)
        };
        let g_ground = (1.0 - attack) / (R165 + R166) + attack / R166 + leak_g;

        if freq != self.network.freq || decay != self.network.decay {
            self.network = Network::new(freq, decay, st.rest_hz);
        }
        let Network { c, a, g, .. } = self.network;
        let g_total = 1.0 / R161 + 1.0 / R170 + g_ground;

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
                (-1.0 / R167 + g_total + fb) / c,
                (-1.0 / R167 + fb) / c,
            ],
            (drive - vplus * (g_total + fb)) / c,
            dt,
        );
        let mut vcomm = vplus - linear.0;
        let mut vbt = vcomm - linear.1;
        let (arm1, arm2) = if vbt.abs() > VSAT {
            vbt = VSAT.copysign(vbt);
            let pinned = trapezoid(
                (self.arm1, self.arm2),
                [-a, -a, -1.0 / (R167 * c), (-1.0 / R167 - g_total) / c],
                (drive - vbt * (g_total + fb)) / c,
                dt,
            );
            vcomm = vbt + pinned.1;
            pinned
        } else {
            linear
        };
        self.arm1 = sanitize(arm1);
        self.arm2 = sanitize(arm2);
        self.vcomm = vcomm;

        // Tone low-pass, then the level and output coupling high-passes.
        if tone != self.tone {
            self.tone = tone;
            self.tone_coeff = one_pole_coeff(tone_hz(tone), sample_rate);
        }
        let lp = low_pass(&mut self.tone_lp, vbt, self.tone_coeff);
        let level = high_pass(&mut self.level_hp, lp, st.level_hp_coeff);
        let out = high_pass(&mut self.output_hp, level, st.output_hp_coeff);

        out
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

message_handlers!(impl Bd808 {});
