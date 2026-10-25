use std::ops::Range;

use super::*;
use crate::dsp::drums::input::{Cached, Input, Live, each_slot};
use crate::dsp::drums::tr808::circuit::PULSE_SECS;
use crate::dsp::drums::voice::{Timing, rails};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Bd808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled at each strike and held for the hit: up to twice
    /// as loud, with more pitch sigh (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// tone control, 0–5 V: from a dull thud to a bright click at the attack
    /// (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// decay control, 0–5 V: how long the drum rings, from ≈0.09 s to
    /// ≈3.8 s (default 2.5, ≈0.6 s)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// pitch in V/Oct; stock is ≈49.5 Hz (default -2.4035)
    #[signal(type = pitch, default = -2.4035, range = (-3.5, -1.0))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// fade out a hit still sounding over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise each hit builds on the last, as on
    /// the 808 (default false)
    #[serde(default)]
    #[deserr(default)]
    restart: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Bd808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 bass drum.
///
/// A circuit model of the 808's deep, booming kick: a low, round ring at ≈49.5
/// Hz with a click at the attack and a pitch that sighs down as it rings.
/// **decay** sets how long it booms, **tone** how much click comes through,
/// **freq** its pitch, and **accent** how hard it hits.
///
/// As on the 808, a hit that lands on a still-ringing one builds on it, so long
/// decays at fast tempos swing in level from hit to hit; **restart** starts
/// every hit from rest instead.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near **±10
/// V**. Near the top of the **freq** range the drum overdrives, so accent adds
/// less there.
///
/// ```js
/// $tr808.kick($clock.beatTrigger, { decay: 4 }).out()
/// ```
#[module(name = "$tr808.kick", has_init, patch_update, args(trigger), block)]
pub struct Bd808 {
    outputs: Bd808Outputs,
    params: Bd808Params,
    state: BdState,
    channel_state: Box<[Channel]>,
}

/// The bass drum's per-sample-rate constants and its channel groups.
#[derive(Default)]
struct BdState {
    shared: KickShared,
    groups: Groups<KickDrum>,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Kick>,
    /// The bridged-T for the channel's `freq` and `decay`.
    network: Cached<(f32, f32), Network>,
    /// The tone low-pass coefficient for its `tone`.
    tone: Cached<f32, f32>,
}

impl Bd808 {
    fn init(&mut self, sample_rate: f32) {
        self.state.groups = Groups::new();
        let sh = &mut self.state.shared;
        let dt = 1.0 / sample_rate;
        sh.timing = Timing::new(PULSE_SECS, sample_rate);
        sh.dt = dt;
        sh.sample_rate = sample_rate;
        let shaper_tau = C40 * R162 * R163 / (R162 + R163);
        sh.shaper_mul = (-dt / shaper_tau).exp();
        sh.c38_coeff = 1.0 - (-dt / R156_C38).exp();
        sh.c39_step = dt / (R161 * C39);
        sh.level_hp_coeff = one_pole_coeff(LEVEL_HP_HZ, sample_rate);
        sh.output_hp_coeff = one_pole_coeff(OUTPUT_HP_HZ, sample_rate);
        let rest_r = 1.0 / (1.0 / R161 + 1.0 / R170 + 1.0 / (R165 + R166));
        sh.rest_hz = 1.0 / (std::f32::consts::TAU * (rest_r * R167).sqrt() * C41_42);
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let p = &self.params;
        let inputs = [
            Live::new(&p.trigger),
            Live::or(&p.accent, Bd808Params::ACCENT_DEFAULT),
            Live::or(&p.freq, Bd808Params::FREQ_DEFAULT),
            Live::or(&p.decay, Bd808Params::DECAY_DEFAULT),
            Live::or(&p.tone, Bd808Params::TONE_DEFAULT),
        ];
        let out = &mut self.outputs.sample;
        self.state.tick(
            &mut self.channel_state[..channels],
            p.restart,
            inputs,
            |ch, v| out.set(ch, v),
        );
    }

    fn process_block(
        &mut self,
        outputs: &mut Bd808BlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let Bd808ParamsBlockInputs {
            trigger,
            accent,
            freq,
            decay,
            tone,
        } = p.block_inputs(range.clone(), channels, &mut scratch)?;
        let channel_state = &mut self.channel_state[..channels];
        each_slot(
            &mut outputs.sample,
            range,
            [trigger, accent, freq, decay, tone],
            |inputs, mut slot| {
                self.state
                    .tick(channel_state, p.restart, inputs, |ch, v| slot.set(ch, v))
            },
        );
        Some(())
    }
}

impl BdState {
    /// Runs one sample of every channel on its `[trigger, accent, freq, decay,
    /// tone]`, handing `out` each one's output.
    #[inline(always)]
    fn tick<I: Input>(
        &mut self,
        channels: &mut [Channel],
        restart: bool,
        [trigger, accent, freq, decay, tone]: [I; 5],
        mut out: impl FnMut(usize, f32),
    ) {
        let run = Run {
            accent,
            restart,
            fade_samples: self.shared.timing.fade_samples,
            shared: &self.shared,
        };
        let inputs = BdInputs {
            sh: &self.shared,
            trigger,
            freq,
            decay,
            tone,
        };
        self.groups.run(&inputs, channels, &run, |ch, v| {
            out(ch, rails(v * OUTPUT_GAIN))
        });
    }
}

/// The inputs every channel of the bass drum reads.
struct BdInputs<'a, I> {
    sh: &'a KickShared,
    trigger: I,
    freq: I,
    decay: I,
    tone: I,
}

impl<I: Input> ChannelInputs<KickDrum> for BdInputs<'_, I> {
    type Channel = Channel;

    fn voices(channel: &mut Channel) -> &mut Voices<Kick> {
        &mut channel.voices
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, cs: &mut Channel) -> (f32, KickControls) {
        let network = cs
            .network
            .get((self.freq.at(ch), self.decay.at(ch)), |(freq, decay)| {
                Network::new(freq, decay, self.sh.rest_hz)
            });
        let tone_coeff = cs.tone.get(self.tone.at(ch), |tone| {
            one_pole_coeff(tone_hz(tone), self.sh.sample_rate)
        });
        let controls = KickControls {
            c: network.c,
            a: network.a,
            g: network.g,
            tone_coeff,
            level: network.level,
        };
        (self.trigger.at(ch), controls)
    }
}

impl crate::types::PatchUpdateHandler for Bd808 {
    fn on_patch_update(&mut self) {
        let count = self.channel_count();
        self.state
            .groups
            .layout::<BdInputs<&[f32]>>(&mut self.channel_state, count);
    }
}

message_handlers!(impl Bd808 {});
