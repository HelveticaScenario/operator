use std::ops::Range;

use super::*;
use crate::dsp::drums::input::{Input, Live, each_slot};
use crate::dsp::drums::voice::rails;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Ma808Params {
    /// rising edges shake the maracas
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled at each strike and held for the shake: up to
    /// twice as loud, and drawn out (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// fade out a shake still sounding over 2 ms as the next one strikes, so
    /// every shake starts from rest; otherwise each shake runs into the last,
    /// as on the 808 (default false)
    #[serde(default)]
    #[deserr(default)]
    restart: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Ma808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 maracas.
///
/// A circuit model of the 808's maracas: a short shake of high, hissing noise.
/// **accent** makes it louder and draws it out.
///
/// An unaccented shake peaks near **±5 V** and a fully accented one near **±10
/// V**.
///
/// ```js
/// $tr808.maracas($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.maracas", has_init, patch_update, args(trigger), block)]
pub struct Ma808 {
    outputs: Ma808Outputs,
    params: Ma808Params,
    state: MaState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct MaState {
    shared: ShakeShared,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Shake>,
    noise: WhiteNoise,
}

/// A time step long enough that every capacitor in the circuit carries no
/// current: the networks discretised at it solve for the circuit at DC.
const DC_STEP: f32 = 1.0e3;
/// Iterations that settle the switches at DC.
const DC_ITERATIONS: usize = 20;

impl Ma808 {
    fn init(&mut self, sample_rate: f32) {
        let sh = &mut self.state.shared;
        sh.envelope = envelope_network(DC_STEP);
        sh.noise = noise_network(DC_STEP);
        let mut rest = Shake {
            primed: true,
            ..Shake::default()
        };
        for _ in 0..DC_ITERATIONS {
            rest.advance(sh, 0.0, 0.0);
        }
        rest.quiet = Quiet::default();
        sh.rest = rest;
        let dt = 1.0 / sample_rate;
        sh.timing = Timing::new(sample_rate);
        sh.noise_scale = WhiteNoise::scale(sample_rate);
        sh.envelope = envelope_network(dt);
        sh.noise = noise_network(dt);
        sh.c130_coeff = one_pole_coeff(rc_hz(R335, C130), sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let p = &self.params;
        let inputs = [
            Live::new(&p.trigger),
            Live::or(&p.accent, Ma808Params::ACCENT_DEFAULT),
        ];
        let out = &mut self.outputs.sample;
        tick(
            &self.state,
            &mut self.channel_state[..channels],
            p.restart,
            inputs,
            |ch, v| out.set(ch, v),
        );
    }

    fn process_block(
        &mut self,
        outputs: &mut Ma808BlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let Ma808ParamsBlockInputs { trigger, accent } =
            p.block_inputs(range.clone(), channels, &mut scratch)?;
        let channel_state = &mut self.channel_state[..channels];
        each_slot(
            &mut outputs.sample,
            range,
            [trigger, accent],
            |inputs, mut slot| {
                tick(&self.state, channel_state, p.restart, inputs, |ch, v| {
                    slot.set(ch, v)
                })
            },
        );
        Some(())
    }
}

/// Runs one sample of every channel on its `[trigger, accent]`, handing
/// `out` each one's output.
#[inline(always)]
fn tick<I: Input>(
    state: &MaState,
    channels: &mut [Channel],
    restart: bool,
    [trigger, accent]: [I; 2],
    mut out: impl FnMut(usize, f32),
) {
    let sh = &state.shared;
    for (ch, cs) in channels.iter_mut().enumerate() {
        let white = cs.noise.next(sh.noise_scale);
        let v = cs.voices.process(
            trigger.at(ch),
            accent,
            ch,
            restart,
            sh.timing.fade_samples,
            sh,
            &white,
        );
        out(ch, rails(v * OUTPUT_GAIN));
    }
}

impl crate::types::PatchUpdateHandler for Ma808 {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.noise.seed(base, ch);
        }
    }
}

message_handlers!(impl Ma808 {});
