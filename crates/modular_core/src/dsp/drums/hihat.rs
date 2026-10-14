use deserr::Deserr;
use schemars::JsonSchema;

use super::common::{AttackDecay, Strike, Svf};
use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{one_pole::OnePole, voct_to_hz};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// A8 (7040 Hz) in V/Oct.
const DEFAULT_FREQ: f32 = 4.75;
const DEFAULT_DECAY: f32 = 0.3;
const DEFAULT_TONE: f32 = 4.53;

/// `tone` moves the cutoff by this many octaves per unit (0–1) around the
/// default, from duller to brighter.
const TONE_OCTAVES: f32 = 0.9;
const TONE_DEFAULT: f32 = 0.906;
/// The filter's resonance rises from `Q_MIN` once the cutoff passes
/// `Q_START_HZ`, reaching `Q_MIN + Q_RISE` `Q_SPAN_OCTAVES` higher.
const Q_MIN: f32 = 0.7;
const Q_RISE: f32 = 2.0;
const Q_START_HZ: f32 = 6000.0;
const Q_SPAN_OCTAVES: f32 = 0.6;
/// A fixed high-pass ahead of the resonant one keeps the lows out whatever
/// the cutoff.
const PRE_HP_HZ: f32 = 4500.0;
const NOISE_LEVEL: f32 = 1.8;
/// Pink noise is white noise through a one-pole low-pass at `PINK_HZ`,
/// lifted by `PINK_LEVEL` to sit at a similar loudness.
const PINK_HZ: f32 = 5000.0;
const PINK_LEVEL: f32 = 2.2;
/// A slow attack also slows the fall: it adds `ATTACK_STRETCH` of itself to
/// `decay`.
const ATTACK_STRETCH: f32 = 0.5;
/// Output scale that puts a default hit's peak near full scale.
const OUTPUT_GAIN: f32 = 0.55;

/// Noise source for the hat.
#[derive(Clone, Copy, Default, Deserr, JsonSchema, Debug, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
enum HatNoise {
    /// bright and hissy
    #[default]
    White,
    /// darker and softer
    Pink,
}

/// Steepness of the hat's high-pass filter.
#[derive(Clone, Copy, Default, Deserr, JsonSchema, Debug, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
enum HatSlope {
    /// 12 dB/octave: more body below the cutoff
    #[default]
    Db12,
    /// 24 dB/octave: thinner and tighter
    Db24,
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct HiHatParams {
    /// rising edges strike the hat
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// resonant high-pass cutoff in V/Oct; higher is brighter and more
    /// resonant (default A8, ≈7 kHz)
    #[signal(type = pitch, default = 4.75, range = (4.4, 5.3))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// seconds for the hit to fall to −60 dB; short for closed, long for open (default 0.3)
    #[signal(default = 0.3, range = (0.0, 3.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// seconds to swell to full level; a slow swell also lengthens the fall
    /// (default 0)
    #[signal(default = 0.0, range = (0.0, 0.22))]
    #[deserr(default)]
    attack: Option<PolySignal>,
    /// brightness, 0–5 V: opens the cutoff around `freq` (default 4.53)
    #[signal(default = 4.53, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// steepness of the high-pass (default db12)
    #[serde(default)]
    #[deserr(default)]
    slope: HatSlope,
    /// noise source (default white)
    #[serde(default)]
    #[deserr(default)]
    noise: HatNoise,
    /// hit strength, 0–5 V, sampled on each trigger and held for the hit; it
    /// scales the output level only (default 5)
    #[signal(default = 5.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct HiHatOutputs {
    #[output("output", "hat output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    strike: Strike,
    env: AttackDecay,
    rng: LcgRng,
    pink: OnePole,
    pre: Svf,
    first: Svf,
    second: Svf,
}

#[derive(Default)]
struct HiHatState {
    /// One-pole coefficient of the pink-noise low-pass; depends only on the sample rate.
    pink_coeff: f32,
    seeded: bool,
}

/// Hi-hat: noise through a resonant high-pass.
///
/// Each rising edge on **trigger** opens the hat, swelling over **attack**
/// and falling to −60 dB over **decay** seconds: short for a closed hat, long
/// for an open one. **freq** sets the high-pass cutoff, which grows more
/// resonant as it rises, and **tone** brightens or dulls it from there.
/// **slope** picks the filter's steepness and **noise** the noise color.
/// **accent**, sampled at each strike, sets the hit's level.
///
/// Output range is **±5 V**.
///
/// ```js
/// $hihat($clock.beatTrigger, { decay: 0.08 }).out()
/// ```
#[module(name = "$hihat", has_init, patch_update, args(trigger))]
pub struct HiHat {
    outputs: HiHatOutputs,
    params: HiHatParams,
    state: HiHatState,
    channel_state: Box<[ChannelState]>,
}

impl HiHat {
    fn init(&mut self, sample_rate: f32) {
        self.state.pink_coeff = 1.0 - (-std::f32::consts::TAU * PINK_HZ / sample_rate).exp();
    }

    fn update(&mut self, sample_rate: f32) {
        let pink_coeff = self.state.pink_coeff;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];

            if cs
                .strike
                .process(p.trigger.get_value(ch), p.accent.value_or(ch, 5.0))
            {
                cs.env.strike();
            }
            if !cs.env.active() {
                self.outputs.sample.set(ch, 0.0);
                continue;
            }

            let attack = p.attack.value_or(ch, 0.0).max(0.0);
            let decay = p.decay.value_or(ch, DEFAULT_DECAY) + ATTACK_STRETCH * attack;
            let level = cs.env.next_linear(attack, decay, sample_rate);
            let tone = p.tone.value_or(ch, DEFAULT_TONE).clamp(0.0, 5.0) / 5.0;
            let cutoff = voct_to_hz(
                p.freq.value_or(ch, DEFAULT_FREQ) + TONE_OCTAVES * (tone - TONE_DEFAULT),
            );
            let rise = ((cutoff / Q_START_HZ).log2() / Q_SPAN_OCTAVES).clamp(0.0, 1.0);
            let q = Q_MIN + Q_RISE * rise * rise;

            let white = cs.rng.next_bipolar();
            let noise = match p.noise {
                HatNoise::White => NOISE_LEVEL * white,
                HatNoise::Pink => {
                    cs.pink.set_coeff(pink_coeff);
                    PINK_LEVEL * cs.pink.process(white)
                }
            };
            let pre = cs.pre.process(noise, PRE_HP_HZ, Q_MIN, sample_rate).hp;
            let mut x = cs.first.process(pre, cutoff, q, sample_rate).hp;
            if p.slope == HatSlope::Db24 {
                x = cs.second.process(x, cutoff, Q_MIN, sample_rate).hp;
            }

            self.outputs
                .sample
                .set(ch, x * level * cs.strike.velocity * OUTPUT_GAIN * 5.0);
        }
    }
}

impl crate::types::PatchUpdateHandler for HiHat {
    /// Seeds each channel's noise off this module's stable heap address once,
    /// so the stream carries over unchanged across patch updates.
    fn on_patch_update(&mut self) {
        if !self.state.seeded {
            let base = seed_base(self);
            for (ch, cs) in self.channel_state.iter_mut().enumerate() {
                cs.rng.seed(base, ch);
            }
            self.state.seeded = true;
        }
    }
}

message_handlers!(impl HiHat {});
