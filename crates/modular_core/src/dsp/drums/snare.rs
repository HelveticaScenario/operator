use deserr::Deserr;
use schemars::JsonSchema;

use super::common::{Decay, Strike, Svf, sine};
use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{one_pole::CachedCoeff, voct_to_hz, wrap_phase};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// C3 (≈131 Hz) in V/Oct.
const DEFAULT_FREQ: f32 = -1.0;
const DEFAULT_DECAY: f32 = 0.475;
const DEFAULT_COLOR: f32 = 4.05;
const DEFAULT_TONE: f32 = 2.55;

/// The body and the color ring fade over these fractions of `decay`; the
/// noise uses all of it.
const BODY_DECAY: f32 = 0.47;
const RING_DECAY: f32 = 0.95;
/// The body starts this much above its pitch and drops back with a time
/// constant of `BODY_DROP_TAU` times `decay`.
const BODY_DROP: f32 = 1.3;
const BODY_DROP_TAU: f32 = 0.0077;
const BODY_LEVEL: f32 = 0.8;
/// Strike click: a positive half-sine of this length and height.
const CLICK_SECS: f32 = 0.0001;
const CLICK_LEVEL: f32 = 0.2;
/// The color ring's pitch in Hz: `RING_BASE_HZ + RING_LIN·c + RING_SQ·c²` for
/// `color` c in 0–1.
const RING_BASE_HZ: f32 = 401.0;
const RING_LIN: f32 = -103.0;
const RING_SQ: f32 = 1202.0;
/// The ring's starting level in dB below the body: `RING_DB_MIN` at `color`
/// 0, rising by `RING_DB_SPAN · c^1.5`.
const RING_DB_MIN: f32 = -37.5;
const RING_DB_SPAN: f32 = 18.5;
/// The wires drown the ring: it fades out as `tone` rises to half way.
/// The noise filter's cutoff sits `NOISE_TONE_OCTAVES` below
/// `NOISE_CUTOFF_HZ` at `tone` 0 and rises to it at full `tone`.
const NOISE_CUTOFF_HZ: f32 = 4500.0;
const NOISE_TONE_OCTAVES: f32 = 2.5;
const NOISE_Q: f32 = 0.8;
/// A gentle low-pass above the audio band dulls the wires of softer accents.
const NOISE_TOP_HZ: f32 = 24000.0;
/// Noise level at full `tone`; it grows as `tone^NOISE_CURVE` up to
/// `NOISE_KNEE` of the range and holds above it.
const NOISE_LEVEL: f32 = 1.2;
const NOISE_CURVE: f32 = 1.9;
const NOISE_KNEE: f32 = 0.8;
/// Level of the softest accent's body and wires; the color ring keeps its
/// level. Softer accents also lower the wire filters by up to
/// `ACCENT_OCTAVES`.
const ACCENT_FLOOR: f32 = 0.087;
const ACCENT_OCTAVES: f32 = 1.6;
/// Output scale that puts a default hit's peak near full scale.
const OUTPUT_GAIN: f32 = 0.6;

/// Response of the snare-wire noise filter.
#[derive(Clone, Copy, Default, Deserr, JsonSchema, Debug, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
enum SnareFilter {
    /// dark, papery wires
    Lowpass,
    /// bright, sizzling wires
    #[default]
    Highpass,
    /// a narrower, mid-focused rattle
    Bandpass,
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct SnareParams {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// pitch of the drum body in V/Oct (default C3, ≈131 Hz)
    #[signal(type = pitch, default = -1.0, range = (-1.7, 0.6))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// seconds for the snare wires to fall to −60 dB; the body and ring fade
    /// sooner in proportion (default 0.475)
    #[signal(default = 0.475, range = (0.0, 2.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// pitch and level of a ringing overtone, 0–5 V; higher is brighter and
    /// louder (default 4.05)
    #[signal(default = 4.05, range = (0.0, 5.0))]
    #[deserr(default)]
    color: Option<PolySignal>,
    /// level and brightness of the snare wires, 0–5 V (default 2.55)
    #[signal(default = 2.55, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// response of the snare-wire filter (default highpass)
    #[serde(default)]
    #[deserr(default)]
    filter: SnareFilter,
    /// hit strength, 0–5 V, sampled on each trigger and held for the hit;
    /// softer hits are quieter with darker wires (default 5)
    #[signal(default = 5.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SnareOutputs {
    #[output("output", "drum output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    strike: Strike,
    body: Decay,
    ring: Decay,
    wires: Decay,
    body_phase: f32,
    ring_phase: f32,
    /// Remaining body pitch drop, decaying from 1 at the strike.
    drop: f32,
    drop_coeff: CachedCoeff,
    /// Seconds since the strike.
    elapsed: f32,
    rng: LcgRng,
    /// Whether `rng` has been seeded; channels added by a patch update start
    /// unseeded.
    seeded: bool,
    filter: Svf,
    top: Svf,
}

#[derive(Default)]
struct SnareState {
    sample_period: f32,
}

/// Snare drum: a short pitched body, a ringing overtone, and filtered noise
/// for the wires.
///
/// Each rising edge on **trigger** strikes the body at **freq**, a ringing
/// overtone whose pitch and level rise with **color**, and the wires: noise
/// whose level and brightness follow **tone**, shaped by **filter**. The wires
/// fall to −60 dB over **decay** seconds, with the body and overtone fading
/// sooner. **accent**, sampled at each strike, sets the hit's level.
///
/// Output range is **±5 V**.
///
/// ```js
/// $snare($clock.beatTrigger, { tone: 4, filter: 'bandpass' }).out()
/// ```
#[module(name = "$snare", has_init, patch_update, args(trigger))]
pub struct Snare {
    outputs: SnareOutputs,
    params: SnareParams,
    state: SnareState,
    channel_state: Box<[ChannelState]>,
}

impl Snare {
    fn init(&mut self, sample_rate: f32) {
        self.state.sample_period = 1.0 / sample_rate;
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
                cs.body.strike();
                cs.ring.strike();
                cs.wires.strike();
                cs.body_phase = 0.0;
                cs.ring_phase = 0.0;
                cs.drop = 1.0;
                cs.elapsed = 0.0;
            }
            if !cs.wires.active() && !cs.ring.active() {
                self.outputs.sample.set(ch, 0.0);
                continue;
            }

            let decay = p.decay.value_or(ch, DEFAULT_DECAY);
            let body_level = cs.body.next(decay * BODY_DECAY, sample_rate);
            let ring_level = cs.ring.next(decay * RING_DECAY, sample_rate);
            let wire_level = cs.wires.next(decay, sample_rate);

            cs.drop *= 1.0
                - cs.drop_coeff
                    .get(BODY_DROP_TAU * decay.max(0.01), sample_rate);
            let hz = voct_to_hz(p.freq.value_or(ch, DEFAULT_FREQ)) * (1.0 + BODY_DROP * cs.drop);
            cs.body_phase = wrap_phase(cs.body_phase + hz / sample_rate);
            let mut x = BODY_LEVEL * body_level * sine(cs.body_phase);
            if cs.elapsed < CLICK_SECS {
                x += CLICK_LEVEL * sine(0.5 * cs.elapsed / CLICK_SECS);
            }

            let color = p.color.value_or(ch, DEFAULT_COLOR).clamp(0.0, 5.0) / 5.0;
            let ring_hz = RING_BASE_HZ + RING_LIN * color + RING_SQ * color * color;
            cs.ring_phase = wrap_phase(cs.ring_phase + ring_hz / sample_rate);
            let tone = p.tone.value_or(ch, DEFAULT_TONE).clamp(0.0, 5.0) / 5.0;
            let ring_db = RING_DB_MIN + RING_DB_SPAN * color * color.sqrt();
            let ring = BODY_LEVEL
                * (1.0 - 2.0 * tone).max(0.0)
                * 10f32.powf(ring_db / 20.0)
                * ring_level
                * sine(cs.ring_phase);

            if tone > 0.0 {
                let darken = ACCENT_OCTAVES * (1.0 - cs.strike.velocity);
                let cutoff = NOISE_CUTOFF_HZ * (-NOISE_TONE_OCTAVES * (1.0 - tone) - darken).exp2();
                let level = NOISE_LEVEL * (tone.min(NOISE_KNEE) / NOISE_KNEE).powf(NOISE_CURVE);
                let out = cs
                    .filter
                    .process(cs.rng.next_bipolar(), cutoff, NOISE_Q, sample_rate);
                let shaped = match p.filter {
                    SnareFilter::Lowpass => out.lp,
                    SnareFilter::Highpass => out.hp,
                    SnareFilter::Bandpass => out.bp,
                };
                let top = NOISE_TOP_HZ * (-darken).exp2();
                let smoothed = cs.top.process(shaped, top, 0.5, sample_rate).lp;
                x += level * wire_level * smoothed;
            }
            cs.elapsed += state.sample_period;

            self.outputs.sample.set(
                ch,
                (x * cs.strike.gain(ACCENT_FLOOR) + ring) * OUTPUT_GAIN * 5.0,
            );
        }
    }
}

impl crate::types::PatchUpdateHandler for Snare {
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

message_handlers!(impl Snare {});
