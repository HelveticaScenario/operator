use std::ops::Range;

use super::*;

/// The inputs every channel of the rim shot reads.
struct RsInputs<I> {
    trigger: I,
}

impl<I: Input> ChannelInputs<RimDrum> for RsInputs<I> {
    type Channel = Voices<Rim>;

    fn voices(channel: &mut Voices<Rim>) -> &mut Voices<Rim> {
        channel
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, _: &mut Voices<Rim>) -> (f32, ()) {
        (self.trigger.at(ch), ())
    }
}

#[derive(Default)]
struct RsState {
    shared: RimShared,
    groups: Groups<RimDrum>,
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Rs909Params {
    /// rising edges strike the rim
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled at each strike and held for the hit: up to twice
    /// as loud (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// fade out a hit still sounding over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise each hit builds on the last, as on
    /// the 909 (default false)
    #[serde(default)]
    #[deserr(default)]
    restart: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Rs909Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-909 rim shot.
///
/// A circuit model of the 909's rim shot: a short, sharp, metallic click from
/// three tones rung together and clipped. **accent** makes it hit harder.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near **±10
/// V**.
///
/// ```js
/// $tr909.rimShot($clock.beatTrigger).out()
/// ```
#[module(name = "$tr909.rimShot", has_init, patch_update, args(trigger), block)]
pub struct Rs909 {
    outputs: Rs909Outputs,
    params: Rs909Params,
    state: RsState,
    channel_state: Box<[Voices<Rim>]>,
}

impl Rs909 {
    fn init(&mut self, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        self.state.groups = Groups::new();
        let sh = &mut self.state.shared;
        sh.timing = Timing::new(TRIG_SECS, sample_rate);
        sh.dt = dt;
        sh.res = resonator_network(dt);
        sh.out = output_network(dt);
        sh.env_charge = (-dt / (R401 * C119)).exp();
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let p = &self.params;
        let inputs = [
            Live::new(&p.trigger),
            Live::or(&p.accent, Rs909Params::ACCENT_DEFAULT),
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
        outputs: &mut Rs909BlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let Rs909ParamsBlockInputs { trigger, accent } =
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

impl RsState {
    /// Runs one sample of every channel on its `[trigger, accent]`, handing
    /// `out` each one's output.
    #[inline(always)]
    fn tick<I: Input>(
        &mut self,
        channels: &mut [Voices<Rim>],
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
        let inputs = RsInputs { trigger };
        self.groups
            .run(&inputs, channels, &run, |ch, v| out(ch, rails(v)));
    }
}

impl crate::types::PatchUpdateHandler for Rs909 {
    fn on_patch_update(&mut self) {
        let count = self.channel_count();
        self.state
            .groups
            .layout::<RsInputs<&[f32]>>(&mut self.channel_state, count);
    }
}

message_handlers!(impl Rs909 {});
