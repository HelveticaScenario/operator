use deserr::Deserr;
use schemars::JsonSchema;

use super::common::{Decay, Strike, Svf, sine};
use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::{voct_to_hz, wrap_phase};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// E2 (≈82.4 Hz) in V/Oct.
const DEFAULT_FREQ: f32 = -20.0 / 12.0;
const DEFAULT_DECAY: f32 = 1.1;
const DEFAULT_BEND: f32 = 0.5;
const DEFAULT_COLOR: f32 = 2.1;
const DEFAULT_TONE: f32 = 5.0;

/// Time constant of the glide from the bent pitch back to `freq`, in seconds.
const BEND_TAU: f32 = 0.152;
/// The body starts from zero, briefly louder by `ONSET_BOOST`, settling
/// with this time constant.
const ONSET_BOOST: f32 = 0.4;
const ONSET_TAU: f32 = 0.0025;
/// A free-running wobble at `WOBBLE_HZ` splits the body into sidebands that
/// many hertz above and below it. The upper one sits `WOBBLE_DB` below the
/// body at `WOBBLE_REF_HZ`, rising `WOBBLE_DB_PER_OCTAVE` per octave of pitch;
/// the lower one is weaker by `LOWER_DB_HZ / (pitch − WOBBLE_HZ)` dB. `tone`
/// crossfades the wobble into the membrane noise over its lower half.
const WOBBLE_HZ: f32 = 190.0;
const WOBBLE_DB: f32 = -16.3;
const WOBBLE_REF_HZ: f32 = 177.0;
const WOBBLE_DB_PER_OCTAVE: f32 = 2.67;
const LOWER_DB_HZ: f32 = 520.0;
/// `color` c (0–1) also lifts the body by `1 + COLOR_BODY · c²`.
const BODY_LEVEL: f32 = 1.1;
const COLOR_BODY: f32 = 1.03;
/// The membrane is noise band-passed around `NOISE_RATIO` times the pitch,
/// then low-passed steeply at `COLOR_MIN_HZ · 2^(COLOR_OCTAVES · color)`,
/// quietening by `(cutoff / COLOR_REF_HZ)^-COLOR_TILT` as it opens. Up to
/// `BYPASS` of the band skirts the low-pass, and softer accents close the
/// low-pass by up to `ACCENT_OCTAVES` and shrink the bypass.
const NOISE_RATIO: f32 = 1.15;
const NOISE_Q: f32 = 1.2;
const NOISE_LEVEL: f32 = 3.7;
const BYPASS: f32 = 0.2;
const COLOR_MIN_HZ: f32 = 1100.0;
const COLOR_OCTAVES: f32 = 4.2;
const COLOR_REF_HZ: f32 = 1000.0;
const COLOR_TILT: f32 = 0.29;
const ACCENT_OCTAVES: f32 = 3.5;
/// Level of the softest accent.
const ACCENT_FLOOR: f32 = 0.22;
/// Output scale that puts a default hit's peak near full scale.
const OUTPUT_GAIN: f32 = 0.55;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct TomParams {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// resting pitch in V/Oct (default E2, ≈82 Hz)
    #[signal(type = pitch, default = -1.667, range = (-2.7, 1.2))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// seconds for the hit to fall to −60 dB (default 1.1)
    #[signal(default = 1.1, range = (0.0, 4.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// octaves above `freq` that the hit starts at, gliding back down (default 0.5)
    #[signal(default = 0.5, range = (0.0, 2.0))]
    #[deserr(default)]
    bend: Option<PolySignal>,
    /// brightness of the membrane noise, 0–5 V (default 2.1)
    #[signal(default = 2.1, range = (0.0, 5.0))]
    #[deserr(default)]
    color: Option<PolySignal>,
    /// membrane noise, 0–5 V: crossfades from a metallic wobble to full noise
    /// at 2.5 V and above (default 5)
    #[signal(default = 5.0, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// hit strength, 0–5 V, sampled on each trigger and held for the hit;
    /// softer hits are quieter and darker (default 5)
    #[signal(default = 5.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TomOutputs {
    #[output("output", "drum output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    strike: Strike,
    env: Decay,
    phase: f32,
    wobble_phase: f32,
    /// Remaining bend and onset boost, each decaying from 1 at the strike.
    glide: f32,
    onset: f32,
    rng: LcgRng,
    /// Whether `rng` has been seeded; channels added by a patch update start
    /// unseeded.
    seeded: bool,
    band: Svf,
    color: [Svf; 2],
}

#[derive(Default)]
struct TomState {
    /// Per-sample decay multipliers; they depend only on the sample rate.
    glide_mul: f32,
    onset_mul: f32,
}

/// Parabolic approximation of a sine of `phase` in cycles: its overtones are
/// the odd harmonics, falling as the cube of their number.
#[inline]
fn parabolic(phase: f32) -> f32 {
    if phase < 0.5 {
        16.0 * phase * (0.5 - phase)
    } else {
        -16.0 * (phase - 0.5) * (1.0 - phase)
    }
}

/// Tom drum: a pitched body that glides down from **bend**, over a band of
/// noisy membrane.
///
/// Each rising edge on **trigger** starts the body **bend** octaves above
/// **freq** and glides it down. **tone** sets the level of the membrane noise
/// around the body and **color** its brightness. Everything falls to −60 dB
/// over **decay** seconds. **accent**, sampled at each strike, sets the hit's
/// level; softer hits are also darker.
///
/// Output range is **±5 V**.
///
/// ```js
/// $tom($clock.beatTrigger, { freq: 'a2', bend: 1 }).out()
/// ```
#[module(name = "$tom", has_init, patch_update, args(trigger))]
pub struct Tom {
    outputs: TomOutputs,
    params: TomParams,
    state: TomState,
    channel_state: Box<[ChannelState]>,
}

impl Tom {
    fn init(&mut self, sample_rate: f32) {
        self.state.glide_mul = (-1.0 / (BEND_TAU * sample_rate)).exp();
        self.state.onset_mul = (-1.0 / (ONSET_TAU * sample_rate)).exp();
    }

    fn update(&mut self, sample_rate: f32) {
        let state = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];

            cs.wobble_phase = wrap_phase(cs.wobble_phase + WOBBLE_HZ / sample_rate);
            if cs
                .strike
                .process(p.trigger.get_value(ch), p.accent.value_or(ch, 5.0))
            {
                cs.env.strike();
                cs.phase = 0.0;
                cs.glide = 1.0;
                cs.onset = 1.0;
            }
            if !cs.env.active() {
                self.outputs.sample.set(ch, 0.0);
                continue;
            }

            let level = cs
                .env
                .next(p.decay.value_or(ch, DEFAULT_DECAY), sample_rate);
            cs.glide *= state.glide_mul;
            cs.onset *= state.onset_mul;
            let bend = p.bend.value_or(ch, DEFAULT_BEND).clamp(0.0, 4.0);
            let hz = voct_to_hz(p.freq.value_or(ch, DEFAULT_FREQ) + bend * cs.glide);
            cs.phase = wrap_phase(cs.phase + hz / sample_rate);

            let tone = p.tone.value_or(ch, DEFAULT_TONE).clamp(0.0, 5.0) / 5.0;
            let noisy = (2.0 * tone).min(1.0);
            let color = p.color.value_or(ch, DEFAULT_COLOR).clamp(0.0, 5.0) / 5.0;

            let upper_db = WOBBLE_DB + WOBBLE_DB_PER_OCTAVE * (hz / WOBBLE_REF_HZ).log2();
            let upper = (1.0 - noisy) * 10f32.powf(upper_db / 20.0);
            let gap = hz - WOBBLE_HZ;
            let lower = if gap > 0.0 {
                upper * 10f32.powf(-LOWER_DB_HZ / (20.0 * gap))
            } else {
                0.0
            };
            let body = parabolic(cs.phase)
                + upper * sine(wrap_phase(cs.phase + cs.wobble_phase))
                + lower * sine(wrap_phase(cs.phase - cs.wobble_phase));
            let lift = 1.0 + COLOR_BODY * color * color;
            let mut x = BODY_LEVEL * lift * body * (1.0 + ONSET_BOOST * cs.onset);

            if noisy > 0.0 {
                let open = COLOR_MIN_HZ * (COLOR_OCTAVES * color).exp2();
                let tilt = (open / COLOR_REF_HZ).powf(-COLOR_TILT);
                let cutoff = open * (-ACCENT_OCTAVES * (1.0 - cs.strike.velocity)).exp2();
                let band = cs
                    .band
                    .process(
                        cs.rng.next_bipolar(),
                        NOISE_RATIO * hz,
                        NOISE_Q,
                        sample_rate,
                    )
                    .bp;
                let first = cs.color[0].process(band, cutoff, 0.707, sample_rate).lp;
                let membrane = cs.color[1].process(first, cutoff, 0.707, sample_rate).lp;
                let bypass = BYPASS * (0.3 + 0.7 * cs.strike.velocity);
                let mixed = (1.0 - bypass) * membrane + bypass * band;
                x += NOISE_LEVEL * tilt * noisy * mixed;
            }

            self.outputs.sample.set(
                ch,
                x * level * cs.strike.gain(ACCENT_FLOOR) * OUTPUT_GAIN * 5.0,
            );
        }
    }
}

impl crate::types::PatchUpdateHandler for Tom {
    /// Seeds each channel's noise off this module's stable heap address the
    /// first time it is seen, so streams carry over unchanged across patch
    /// updates and channels added later get their own.
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            if !cs.seeded {
                cs.rng.seed(base, ch);
                cs.wobble_phase = cs.rng.next_unit();
                cs.seeded = true;
            }
        }
    }
}

message_handlers!(impl Tom {});
