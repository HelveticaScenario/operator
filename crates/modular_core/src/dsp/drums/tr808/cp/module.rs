use std::ops::Range;

use super::*;
use crate::dsp::drums::input::{Input, Live, each_slot};
use crate::dsp::drums::voice::rails;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Cp808Params {
    /// rising edges clap
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled at each strike and held for the clap: up to twice
    /// as loud, with a longer tail (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// fade out a clap still sounding over 2 ms as the next one strikes, so
    /// every clap starts from rest; otherwise each clap runs into the last, as
    /// on the 808 (default false)
    #[serde(default)]
    #[deserr(default)]
    restart: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Cp808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 hand clap.
///
/// A circuit model of the 808's hand clap: bursts of filtered noise in quick
/// succession, like several hands clapping at once, over a softer noise tail.
/// **accent** makes it louder and its tail longer.
///
/// An unaccented clap peaks near **±5 V** and a fully accented one near **±10
/// V**.
///
/// ```js
/// $tr808.clap($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.clap", has_init, patch_update, args(trigger), block)]
pub struct Cp808 {
    outputs: Cp808Outputs,
    params: Cp808Params,
    state: CpState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct CpState {
    shared: ClapShared,
    groups: Groups<ClapDrum>,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Clap>,
    noise: WhiteNoise,
}

/// A time step long enough that every capacitor in the circuit carries no
/// current: the networks discretised at it solve for the circuit at DC.
const DC_STEP: f32 = 1.0e3;
/// Iterations that settle the switches and Q70 at DC.
const DC_ITERATIONS: usize = 20;

impl Cp808 {
    fn init(&mut self, sample_rate: f32) {
        self.state.groups = Groups::new();
        let sh = &mut self.state.shared;
        sh.noise = noise_network(DC_STEP);
        sh.envelope = EnvelopeNetwork::new(DC_STEP);
        sh.output = output_network(DC_STEP);
        let mut rest = Clap {
            primed: true,
            ..Clap::default()
        };
        for _ in 0..DC_ITERATIONS {
            rest.advance(sh, 0.0, 0.0);
        }
        rest.quiet = Quiet::default();
        sh.rest = rest;
        let dt = 1.0 / sample_rate;
        sh.timing = Timing::new(sample_rate);
        sh.noise_scale = WhiteNoise::scale(sample_rate);
        sh.noise = noise_network(dt);
        sh.envelope = EnvelopeNetwork::new(dt);
        sh.output = output_network(dt);
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let p = &self.params;
        let inputs = [
            Live::new(&p.trigger),
            Live::or(&p.accent, Cp808Params::ACCENT_DEFAULT),
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
        outputs: &mut Cp808BlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let Cp808ParamsBlockInputs { trigger, accent } =
            p.block_inputs(range.clone(), channels, &mut scratch)?;
        let channel_state = &mut self.channel_state[..channels];
        each_slot(
            &mut outputs.sample,
            range,
            [trigger, accent],
            |inputs, mut slot| {
                self.state
                    .tick(channel_state, p.restart, inputs, |ch, v| slot.set(ch, v))
            },
        );
        Some(())
    }
}

impl CpState {
    /// Runs one sample of every channel on its `[trigger, accent]`, handing
    /// `out` each one's output.
    #[inline(always)]
    fn tick<I: Input>(
        &mut self,
        channels: &mut [Channel],
        restart: bool,
        [trigger, accent]: [I; 2],
        mut out: impl FnMut(usize, f32),
    ) {
        let run = Run {
            accent,
            restart,
            fade_samples: self.shared.timing.fade_samples,
            shared: &self.shared,
        };
        let inputs = CpInputs {
            trigger,
            noise_scale: self.shared.noise_scale,
        };
        self.groups.run(&inputs, channels, &run, |ch, v| {
            out(ch, rails(v * OUTPUT_GAIN))
        });
    }
}

impl crate::types::PatchUpdateHandler for Cp808 {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.noise.seed(base, ch);
        }
        let count = self.channel_count();
        self.state
            .groups
            .layout::<CpInputs<&[f32]>>(&mut self.channel_state, count);
    }
}

message_handlers!(impl Cp808 {});

/// The inputs every channel of the clap reads.
struct CpInputs<I> {
    trigger: I,
    noise_scale: f32,
}

impl<I: Input> Channels<ClapDrum> for CpInputs<I> {
    type Channel = Channel;

    fn voices(channel: &mut Channel) -> &mut Voices<Clap> {
        &mut channel.voices
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, channel: &mut Channel) -> (f32, f32) {
        (self.trigger.at(ch), channel.noise.next(self.noise_scale))
    }
}
