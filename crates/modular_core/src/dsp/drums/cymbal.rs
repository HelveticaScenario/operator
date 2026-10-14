use deserr::Deserr;
use schemars::JsonSchema;

use super::common::{Decay, Strike, Svf, sine};
use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{one_pole::OnePole, voct_to_hz, wrap_phase};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// The partials' fundamental, ≈F4 (≈370 Hz), in V/Oct.
const DEFAULT_FREQ: f32 = 0.5;
const DEFAULT_DECAY: f32 = 1.7;
const DEFAULT_TONE: f32 = 1.5;

/// The bell-like partials sit at these harmonics of `freq`, with these levels:
/// a bright cluster around the 10th harmonic over a weak low body that `tone`
/// filters away. A sub-octave partial at half `freq` adds to the low body.
const PARTIALS: [f32; 8] = [1.0, 2.0, 3.0, 7.0, 9.0, 10.0, 11.0, 13.0];
const PARTIAL_LEVELS: [f32; 8] = [0.08, 0.1, 0.1, 0.26, 0.54, 1.0, 0.75, 0.46];
const SUB_LEVEL: f32 = 0.2;
/// A partial fades out over the top `NYQUIST_FADE` of the band below Nyquist,
/// so pitch sweeps never switch one on or off abruptly.
const NYQUIST_FADE: f32 = 0.1;
const PARTIAL_GAIN: f32 = 0.018;
/// The hiss is white noise rising 6 dB/octave up to `HISS_CORNER_HZ` and flat
/// above it.
const HISS_LEVEL: f32 = 0.36;
const HISS_CORNER_HZ: f32 = 8000.0;
/// The strike adds a burst on top of the ring: `STRIKE_BOOST` times the ring's
/// level at the strike, fading with `STRIKE_TAU` seconds.
const STRIKE_BOOST: f32 = 2.0;
const STRIKE_TAU: f32 = 0.008;
/// The high-pass on the whole cymbal sweeps from `TONE_MIN_HZ` up by
/// `TONE_OCTAVES` as `tone` goes 0–5 V.
const TONE_MIN_HZ: f32 = 300.0;
const TONE_OCTAVES: f32 = 3.8;
const TONE_Q: f32 = 0.7;
/// Level of the softest accent.
const ACCENT_FLOOR: f32 = 0.155;
/// Output scale that puts a default hit's peak near full scale.
const OUTPUT_GAIN: f32 = 1.0;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct CymbalParams {
    /// rising edges strike the cymbal
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// fundamental of the cymbal's bell-like partials in V/Oct; they ring at
    /// its 7th–13th harmonics (default ≈F4)
    #[signal(type = pitch, default = 0.5, range = (0.4, 1.6))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// seconds for the hit to fall to −60 dB (default 1.7)
    #[signal(default = 1.7, range = (0.0, 6.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// high-pass cutoff, 0–5 V; higher thins out the body (default 1.5)
    #[signal(default = 1.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// hit strength, 0–5 V, sampled on each trigger and held for the hit; it
    /// scales the output level only (default 5)
    #[signal(default = 5.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CymbalOutputs {
    #[output("output", "cymbal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    strike: Strike,
    env: Decay,
    phase: f32,
    sub_phase: f32,
    /// Strike burst level, decaying from 1 at the strike.
    burst: f32,
    rng: LcgRng,
    /// Whether `rng` has been seeded; channels added by a patch update start
    /// unseeded.
    seeded: bool,
    tilt: OnePole,
    highpass: Svf,
}

#[derive(Default)]
struct CymbalState {
    /// Per-sample strike-burst decay multiplier and hiss tilt coefficient;
    /// both depend only on the sample rate.
    burst_mul: f32,
    tilt_coeff: f32,
}

/// Cymbal: a cluster of bell-like partials over bright hiss.
///
/// Each rising edge on **trigger** strikes the cymbal: partials at the 7th–13th
/// harmonics of **freq** over broadband hiss, high-passed by **tone** and
/// falling to −60 dB over **decay** seconds. **accent**, sampled at each
/// strike, sets the hit's level.
///
/// Output range is **±5 V**.
///
/// ```js
/// $cymbal($clock.barTrigger, { decay: 3 }).out()
/// ```
#[module(name = "$cymbal", has_init, patch_update, args(trigger))]
pub struct Cymbal {
    outputs: CymbalOutputs,
    params: CymbalParams,
    state: CymbalState,
    channel_state: Box<[ChannelState]>,
}

impl Cymbal {
    fn init(&mut self, sample_rate: f32) {
        self.state.burst_mul = (-1.0 / (STRIKE_TAU * sample_rate)).exp();
        self.state.tilt_coeff = 1.0 - (-std::f32::consts::TAU * HISS_CORNER_HZ / sample_rate).exp();
    }

    fn update(&mut self, sample_rate: f32) {
        let state = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];

            if cs
                .strike
                .process(p.trigger.get_value(ch), p.accent.value_or(ch, 5.0))
            {
                cs.env.strike();
                cs.burst = 1.0;
            }
            if !cs.env.active() {
                self.outputs.sample.set(ch, 0.0);
                continue;
            }

            cs.burst *= state.burst_mul;
            let level = cs
                .env
                .next(p.decay.value_or(ch, DEFAULT_DECAY), sample_rate)
                * (1.0 + STRIKE_BOOST * cs.burst);
            let hz = voct_to_hz(p.freq.value_or(ch, DEFAULT_FREQ));
            cs.phase = wrap_phase(cs.phase + hz / sample_rate);
            cs.sub_phase = wrap_phase(cs.sub_phase + 0.5 * hz / sample_rate);
            let nyquist = 0.5 * sample_rate;
            let mut bell = SUB_LEVEL * sine(cs.sub_phase);
            for (harmonic, amp) in PARTIALS.iter().zip(PARTIAL_LEVELS) {
                let fade = ((nyquist - hz * harmonic) / (NYQUIST_FADE * nyquist)).clamp(0.0, 1.0);
                if fade > 0.0 {
                    bell += fade * amp * sine(wrap_phase(cs.phase * harmonic));
                }
            }
            let white = cs.rng.next_bipolar();
            cs.tilt.set_coeff(state.tilt_coeff);
            let hiss = white - cs.tilt.process(white);
            let raw = PARTIAL_GAIN * bell + HISS_LEVEL * hiss;

            let tone = p.tone.value_or(ch, DEFAULT_TONE).clamp(0.0, 5.0) / 5.0;
            let cutoff = TONE_MIN_HZ * (TONE_OCTAVES * tone).exp2();
            let x = cs.highpass.process(raw, cutoff, TONE_Q, sample_rate).hp;

            self.outputs.sample.set(
                ch,
                x * level * cs.strike.gain(ACCENT_FLOOR) * OUTPUT_GAIN * 5.0,
            );
        }
    }
}

impl crate::types::PatchUpdateHandler for Cymbal {
    /// Seeds each channel's noise off this module's stable heap address the
    /// first time it is seen, so streams carry over unchanged across patch
    /// updates and channels added later get their own.
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            if !cs.seeded {
                cs.rng.seed(base, ch);
                cs.seeded = true;
            }
        }
    }
}

message_handlers!(impl Cymbal {});
