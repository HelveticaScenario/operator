//! `$ensemble` — the core of the 4ms Ensemble Oscillator: per channel, a bank of
//! sine voices spread across a scale, with cross FM, a phase warp, and the
//! fold/cheby/segment warps.
//!
//! Adapted from the 4ms Ensemble Oscillator. Copyright 4ms Company. Used under
//! GPL v3.

mod scale;
mod voice;

use deserr::Deserr;
use schemars::JsonSchema;
use serde::Serialize;

use crate::dsp::phase_warp::{PhaseWarp, PhaseWarpGlides};
use crate::dsp::utils::dc_blocker::DcBlocker;
use crate::dsp::utils::fade::{DEFAULT_FADE_IN_SECS, FadeIn};
use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::dsp::utils::sanitize;
use crate::poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt};
use crate::types::SwapGlide;

use scale::{EnsembleScale, ScaleNotes};
use voice::{EffectOrder, Voice, VoiceContext, WarpChain};

/// Most voices per channel.
const MAX_VOICES: usize = 16;

/// Gain correction for a sum of `n` uncorrelated voices, indexed by `n`: how far
/// the sum can be pushed before it is likely to exceed full scale. From the
/// reference's `normalization_factors` table.
const NORMALIZATION: [f32; MAX_VOICES + 1] = [
    0.0,
    -0.4,
    0.001_303_781,
    0.000_977_517_1,
    0.003_134_796_2,
    0.007_874_016,
    0.018_181_818,
    0.031_218_53,
    0.047_748_977,
    0.066_666_67,
    0.087_678_64,
    0.109_425_79,
    0.132_743_36,
    0.156_502_1,
    0.180_811_81,
    0.205_414_95,
    0.230_073_5,
];

/// How each voice's cross-FM output reaches the others.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, Serialize, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum CrossFmMode {
    /// The lowest voice modulates every other voice.
    #[default]
    Up,
    /// Each voice modulates the next one up.
    Chain,
    /// The highest voice modulates every other voice, one sample late.
    Down,
}

impl CrossFmMode {
    fn depth_scale(self) -> f32 {
        match self {
            CrossFmMode::Up => 6.0,
            CrossFmMode::Chain => 0.9,
            CrossFmMode::Down => 4.0,
        }
    }
}

fn default_count() -> usize {
    8
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct EnsembleParams {
    /// pitch in V/Oct of the scale's first note; every voice moves with it
    #[signal(type = pitch)]
    pitch: PolySignal,
    /// the scale the voices sit on: an Ensemble Oscillator preset such as
    /// "enosc.harm4-7" or "enosc.bp", notes as one V/Oct pitch per channel, or
    /// a spec string like "C(major)" / "C[maj7]". Only the intervals above the
    /// first note matter. Semitones if omitted.
    #[deserr(default)]
    scale: Option<EnsembleScale>,
    /// position of the lowest voice in V above **pitch**, quantized to
    /// **scale**; each voice above adds **spread**
    #[signal(default = 0.0)]
    #[deserr(default)]
    root: Option<PolySignal>,
    /// root distance between neighbouring voices (0–5, up to 7.5 semitones)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    spread: Option<PolySignal>,
    /// unquantized detuning that grows and alternates in sign up the voices
    /// (0–5, up to 0.625 semitones between the first two)
    #[signal(default = 1.5, range = (0.0, 5.0))]
    #[deserr(default)]
    detune: Option<PolySignal>,
    /// how much of each step between scale notes crossfades rather than jumps
    /// (0 = hard steps, 5 = always crossfading)
    #[signal(default = 4.33, range = (0.0, 5.0))]
    #[deserr(default)]
    crossfade: Option<PolySignal>,
    /// cross-FM depth (0–5)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    cross_fm: Option<PolySignal>,
    /// cross-FM routing: up (default), chain, or down
    #[serde(default)]
    #[deserr(default)]
    cross_fm_mode: CrossFmMode,
    /// level tilt across the voices: negative favours the lowest, positive the
    /// highest, 0 is even
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    #[deserr(default)]
    balance: Option<PolySignal>,
    /// phase warp applied to every oscillator before its sine, built with
    /// `$phase.*`
    #[deserr(default)]
    phase: Option<PhaseWarp>,
    /// wavefold amount (0–5)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    fold: Option<PolySignal>,
    /// Chebyshev waveshaping amount (0–5)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    cheby: Option<PolySignal>,
    /// triangle-segment waveshaping amount (0–5)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    segment: Option<PolySignal>,
    /// order of the fold (f), cheby (c) and segment (s) warps, e.g. "fcs"
    /// (default) or "scf"
    #[serde(default)]
    #[deserr(default)]
    effect_order: EffectOrder,
    /// voices per channel (1–16, default 8)
    #[serde(default = "default_count")]
    #[deserr(default = default_count())]
    count: usize,
}

/// As wide as the widest input. The scale is one shared set of notes, so it
/// does not widen the module.
#[allow(private_interfaces)]
pub fn ensemble_derive_channel_count(params: &EnsembleParams) -> usize {
    params
        .pitch
        .channels()
        .max(params.root.channel_count())
        .max(params.spread.channel_count())
        .max(params.detune.channel_count())
        .max(params.crossfade.channel_count())
        .max(params.cross_fm.channel_count())
        .max(params.balance.channel_count())
        .max(params.phase.as_ref().map_or(0, PhaseWarp::channels))
        .max(params.fold.channel_count())
        .max(params.cheby.channel_count())
        .max(params.segment.channel_count())
        .clamp(1, PORT_MAX_CHANNELS)
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct EnsembleOutputs {
    #[output("output", "mix of every voice", default, range = (-5.0, 5.0))]
    output: PolyOutput,
    #[output("odd", "mix of voices 1, 3, 5, …", range = (-5.0, 5.0))]
    odd: PolyOutput,
    #[output("even", "mix of voices 2, 4, 6, …", range = (-5.0, 5.0))]
    even: PolyOutput,
    #[output("low", "mix of the lower half of the voices", range = (-5.0, 5.0))]
    low: PolyOutput,
    #[output("high", "mix of the upper half of the voices", range = (-5.0, 5.0))]
    high: PolyOutput,
    #[output("root", "the lowest voice alone", range = (-5.0, 5.0))]
    root: PolyOutput,
    #[output("other", "mix of every voice but the lowest", range = (-5.0, 5.0))]
    other: PolyOutput,
}

/// Number of outputs, each with its own DC blocker.
const OUTPUTS: usize = 7;

#[derive(Clone, Copy, Default)]
struct ChannelState {
    voices: [Voice; MAX_VOICES],
    /// Voices that sounded on the previous sample; the rest restart silent.
    active_voices: usize,
    /// The highest voice's cross-FM output from the previous sample, for
    /// [`CrossFmMode::Down`].
    down_bus: f32,
    dc: [DcBlocker; OUTPUTS],
    cross_fm: SwapGlide,
    balance: SwapGlide,
    fold: SwapGlide,
    cheby: SwapGlide,
    segment: SwapGlide,
    phase: PhaseWarpGlides,
}

#[derive(Default)]
struct EnsembleState {
    scale: ScaleNotes,
    inv_sample_rate: f32,
    dc_coeff: f32,
    crossfade_coef: f32,
    fade_inc: f32,
}

/// A bank of sine voices on a scale, after the 4ms Ensemble Oscillator.
///
/// Each channel is a whole ensemble of **count** voices playing **scale** from
/// **pitch**. The lowest voice sits **root** above it, quantized to the scale;
/// each voice above adds **spread** to the root before quantizing, so the
/// voices stack up the scale. **detune** pulls the voices apart by small,
/// growing, alternating amounts.
///
/// **scale** takes the Ensemble Oscillator's Octave and Free banks as presets
/// (`enosc.harm2-3`, `enosc.harm4-7`, `enosc.harm8-15`, `enosc.harm-odd`,
/// `enosc.pythag3`, `enosc.pythag5`, `enosc.just4`, `enosc.just6`,
/// `enosc.5tet`, `enosc.7tet`, `enosc.fifths`, `enosc.mlt1`–`enosc.mlt4`,
/// `enosc.bp`, `enosc.bp-major`, `enosc.bp-minor`, `enosc.alpha`,
/// `enosc.beta`). Free scales repeat at their own interval (a fifth, a
/// tritave, …) rather than the octave.
///
/// A voice whose root falls between two scale notes crossfades between them, so
/// sweeping **root** or **spread** morphs smoothly from chord to chord;
/// **crossfade** narrows the fade until the voices step from note to note.
///
/// **crossFm** routes each voice's output into the others' phase (see
/// **crossFmMode**), **phase** warps every oscillator's phase with a `$phase.*`
/// warp, and **fold**, **cheby** and **segment** shape every voice's waveform in
/// the order **effectOrder** gives. **balance** tilts the mix toward the low or
/// high voices.
///
/// **output** is the mix of every voice; the other outputs split the voices the
/// way the hardware's stereo modes do.
///
/// ```js
/// $ensemble('c3', 'C(minor)').out()
/// $ensemble('c2', 'enosc.bp', { spread: 1, count: 6 }).out()
/// ```
///
/// ```js
/// // Stereo: odd voices left, even voices right, with pulsar and cross FM
/// const e = $ensemble('a2', 'A(dorian)', {
///   spread: $sine('0.05hz').range(1, 3),
///   crossFm: 1.5,
///   phase: $phase.pulsar(2),
///   fold: 1,
/// })
/// e.odd.out({ pan: -5 })
/// e.even.out({ pan: 5 })
/// ```
#[module(
    name = "$ensemble",
    channels_derive = ensemble_derive_channel_count,
    has_init,
    args(pitch, scale)
)]
pub struct Ensemble {
    outputs: EnsembleOutputs,
    state: EnsembleState,
    channel_state: Box<[ChannelState]>,
    params: EnsembleParams,
}

impl Ensemble {
    /// Sample-rate-only constants and the random starting phases. Phases live in
    /// `channel_state`, which a patch update carries over channel by channel, so
    /// only fresh channels keep these.
    fn init(&mut self, sample_rate: f32) {
        self.state.inv_sample_rate = 1.0 / sample_rate;
        // The reference's DC blocker pole is 0.995 at 48 kHz.
        self.state.dc_coeff = 0.995f32.powf(48_000.0 / sample_rate);
        // Matches the reference's crossfade glide: coefficient 0.1 applied 6,000
        // times a second (once per 8 samples at 48 kHz), spread over every sample.
        self.state.crossfade_coef = 1.0 - 0.9f32.powf(6_000.0 / sample_rate);
        self.state.fade_inc = FadeIn::increment(DEFAULT_FADE_IN_SECS, sample_rate);

        let mut rng = LcgRng::default();
        rng.seed(seed_base(self), 0);
        for channel in self.channel_state.iter_mut() {
            for voice in channel.voices.iter_mut() {
                for osc in voice.osc.iter_mut() {
                    osc.phase = rng.next_unit();
                }
            }
        }
    }

    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        let params = &self.params;
        let state = &mut self.state;
        state.scale.update(params.scale.as_ref());
        let scale = &state.scale;

        let count = params.count.clamp(1, MAX_VOICES);
        let depth_scale = 6.0 / count as f32 * params.cross_fm_mode.depth_scale();
        let normalization = NORMALIZATION[count];

        for ch in 0..channels {
            let cs = &mut self.channel_state[ch];
            if count > cs.active_voices {
                for voice in &mut cs.voices[cs.active_voices..count] {
                    voice.restart();
                }
            }
            cs.active_voices = count;

            let knob = |v: f32| (v / 5.0).clamp(0.0, 1.0);
            let pitch = sanitize(params.pitch.get_value(ch));
            let root = sanitize(params.root.value_or_zero(ch));
            let spread = knob(params.spread.value_or(ch, 2.5)) * 7.5 / 12.0;
            let detune = knob(params.detune.value_or(ch, 1.5)).powi(4) * (10.0 / 16.0) / 12.0;
            let crossfade = knob(params.crossfade.value_or(ch, 4.33));
            let crossfade_factor = 0.5 * (1.0 - crossfade * crossfade);

            let cross_fm = cs.cross_fm.follow(
                params.cross_fm.value_or_zero(ch),
                params.cross_fm.glide_mark(ch),
                sample_rate,
            );
            // Ignore the bottom 1% so CV noise cannot add harmonics.
            let cross_fm = ((knob(cross_fm) - 0.01) / 0.99).max(0.0) * depth_scale;

            let balance = cs.balance.follow(
                params.balance.value_or_zero(ch),
                params.balance.glide_mark(ch),
                sample_rate,
            );
            let tilt = (balance / 5.0).clamp(-1.0, 1.0);
            let balance = f32::exp2(4.0 * tilt * tilt * tilt);

            let glided = |glide: &mut SwapGlide, signal: &Option<PolySignal>| {
                knob(glide.follow(signal.value_or_zero(ch), signal.glide_mark(ch), sample_rate))
            };
            let warps = WarpChain::new(
                params.effect_order,
                glided(&mut cs.fold, &params.fold),
                glided(&mut cs.cheby, &params.cheby),
                glided(&mut cs.segment, &params.segment),
            );

            let phase = params
                .phase
                .as_ref()
                .map(|warp| (warp, warp.resolve(ch, sample_rate, &mut cs.phase)));

            let ctx = VoiceContext {
                inv_sample_rate: state.inv_sample_rate,
                phase,
                warps,
                cross_fm,
                crossfade_factor,
                crossfade_coef: state.crossfade_coef,
                fade_inc: state.fade_inc,
            };

            let mut sums = [0.0f32; OUTPUTS];
            let [all, odd, even, low, high, lowest, other] = &mut sums;
            let mut amplitude = 1.0f32;
            let mut amplitude_sum = 0.0f32;
            let mut voice_root = root;
            let mut detune_step = detune;
            let mut detune_offset = 0.0f32;
            let mut bus = 0.0f32;
            let mut next_down_bus = 0.0f32;

            for (i, voice) in cs.voices[..count].iter_mut().enumerate() {
                let pair = voice.pitch_pair(scale, voice_root);
                let offset = pitch + detune_offset;
                voice_root += spread;
                detune_step *= -1.2;
                detune_offset += detune_step;

                let top = i == count - 1;
                let mod_in = match params.cross_fm_mode {
                    CrossFmMode::Up | CrossFmMode::Chain if i == 0 => 0.0,
                    CrossFmMode::Up | CrossFmMode::Chain => bus,
                    CrossFmMode::Down if top => 0.0,
                    CrossFmMode::Down => cs.down_bus,
                };
                let (sample, mod_out) = voice.process(
                    [pair.p1 + offset, pair.p2 + offset],
                    pair.crossfade,
                    mod_in,
                    &ctx,
                );
                match params.cross_fm_mode {
                    CrossFmMode::Up if i == 0 => bus = mod_out,
                    CrossFmMode::Chain => bus = mod_out,
                    CrossFmMode::Down if top => next_down_bus = mod_out,
                    _ => {}
                }

                let v = sample * amplitude;
                amplitude_sum += amplitude;
                amplitude *= balance;
                *all += v;
                if i % 2 == 0 {
                    *odd += v;
                } else {
                    *even += v;
                }
                if i < count / 2 {
                    *low += v;
                } else {
                    *high += v;
                }
                if i == 0 {
                    *lowest += v;
                } else {
                    *other += v;
                }
            }
            cs.down_bus = next_down_bus;

            // Normalize the mix, giving back level as the balance evens out
            // (more voices contribute and their sum is less likely to peak).
            let evenness = if balance <= 1.0 {
                balance
            } else {
                1.0 / balance
            };
            let atten = (0.5 + (0.5 + normalization) * evenness) / amplitude_sum;
            let gains = [atten, atten, atten, atten, atten, 0.7, 0.6 * atten];
            let outputs = [
                &mut self.outputs.output,
                &mut self.outputs.odd,
                &mut self.outputs.even,
                &mut self.outputs.low,
                &mut self.outputs.high,
                &mut self.outputs.root,
                &mut self.outputs.other,
            ];
            for (((out, sum), gain), dc) in outputs.into_iter().zip(sums).zip(gains).zip(&mut cs.dc)
            {
                out.set(ch, dc.process(sum * gain * 5.0, state.dc_coeff));
            }
        }
    }
}

message_handlers!(impl Ensemble {});
