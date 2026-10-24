use std::ops::Range;

use super::*;
use crate::dsp::drums::input::{Cached, Input, Live, each_slot};
use crate::dsp::drums::voice::rails;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Sd909Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled at each strike and held for the hit: up to twice
    /// as loud (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// pitch in V/Oct; stock is ≈148 Hz (default -0.8243)
    #[signal(type = pitch, default = -0.8243, range = (-2.3, 0.7))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// tone control, 0–5 V: how long the noise rings on, from a short snap to a
    /// long hiss (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// snappy control, 0–5 V: how loud the noise is (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    snappy: Option<PolySignal>,
    /// fade out a hit still sounding over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise each hit builds on the last, as on
    /// the 909 (default false)
    #[serde(default)]
    #[deserr(default)]
    restart: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Sd909Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-909 snare drum.
///
/// A circuit model of the 909's snare: a drum tone that bends up at the strike,
/// under a burst of noise. **freq** sets the pitch, **snappy** how loud the
/// noise is, **tone** how long it rings on, and **accent** how hard it hits.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near **±10
/// V**.
///
/// ```js
/// $tr909.snare($clock.beatTrigger).out()
/// ```
#[module(name = "$tr909.snare", has_init, patch_update, args(trigger), block)]
pub struct Sd909 {
    outputs: Sd909Outputs,
    params: Sd909Params,
    state: SdState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct SdState {
    shared: SnareShared,
    groups: Groups<SnareDrum>,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Snare>,
    noise: Noise,
    rate: Cached<f32, f32>,
}

impl Sd909 {
    fn init(&mut self, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        self.state.groups = Groups::new();
        let sh = &mut self.state.shared;
        sh.timing = Timing::new(sample_rate);
        sh.dt = dt;
        sh.noise_rate = Noise::rate(sample_rate);
        // The noise network at DC: discretised at a step long enough that
        // its capacitors carry no current.
        let dc = noise_network(DC_STEP);
        let mut injected = [0.0f32; NOISE_UNKNOWNS];
        injected[NOISE_IN] = NOISE_MEAN * STIFF;
        let mut rest = MnaState::default();
        for _ in 0..DC_ITERATIONS {
            dc.step(&mut rest, 0, &injected);
        }
        sh.noise_rest = rest;
        sh.noise = noise_network(dt);
        sh.reset1 = (-dt / VCO1_RESET).exp();
        sh.reset2 = (-dt / VCO2_RESET).exp();
        let g_tune = 1.0 / (R248 + R249);
        sh.c66_charge = (-(1.0 / R247 + g_tune) * dt / C66).exp();
        sh.c66_bleed = (-g_tune * dt / C66).exp();
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let p = &self.params;
        let inputs = [
            Live::new(&p.trigger),
            Live::or(&p.accent, Sd909Params::ACCENT_DEFAULT),
            Live::or(&p.freq, Sd909Params::FREQ_DEFAULT),
            Live::or(&p.tone, Sd909Params::TONE_DEFAULT),
            Live::or(&p.snappy, Sd909Params::SNAPPY_DEFAULT),
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
        outputs: &mut Sd909BlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let Sd909ParamsBlockInputs {
            trigger,
            accent,
            freq,
            tone,
            snappy,
        } = p.block_inputs(range.clone(), channels, &mut scratch)?;
        let channel_state = &mut self.channel_state[..channels];
        each_slot(
            &mut outputs.sample,
            range,
            [trigger, accent, freq, tone, snappy],
            |inputs, mut slot| {
                self.state
                    .tick(channel_state, p.restart, inputs, |ch, v| slot.set(ch, v))
            },
        );
        Some(())
    }
}

impl SdState {
    /// Runs one sample of every channel on its `[trigger, accent, freq, tone,
    /// snappy]`, handing `out` each one's output.
    #[inline(always)]
    fn tick<I: Input>(
        &mut self,
        channels: &mut [Channel],
        restart: bool,
        [trigger, accent, freq, tone, snappy]: [I; 5],
        mut out: impl FnMut(usize, f32),
    ) {
        let run = Run {
            accent,
            restart,
            fade_samples: self.shared.timing.fade_samples,
            shared: &self.shared,
        };
        let inputs = SdInputs {
            trigger,
            freq,
            tone,
            snappy,
            noise_rate: self.shared.noise_rate,
            dt: self.shared.dt,
        };
        self.groups.run(&inputs, channels, &run, |ch, v| {
            out(ch, rails(v * OUTPUT_GAIN))
        });
    }
}

/// The inputs every channel of the snare reads.
struct SdInputs<I> {
    trigger: I,
    freq: I,
    tone: I,
    snappy: I,
    noise_rate: f32,
    dt: f32,
}

impl<I: Input> Channels<SnareDrum> for SdInputs<I> {
    type Channel = Channel;

    fn voices(channel: &mut Channel) -> &mut Voices<Snare> {
        &mut channel.voices
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, channel: &mut Channel) -> (f32, SnareControls) {
        let knob = |s: I| s.at(ch).clamp(0.0, 5.0) / 5.0;
        let tone = 1.0 / (R254 + VR7 * knob(self.tone));
        let controls = SnareControls {
            rate: channel
                .rate
                .get(self.freq.at(ch), |f| (f - Sd909Params::FREQ_DEFAULT).exp2()),
            tone_fall: (-tone * self.dt / C67).exp(),
            snappy: VR9 * knob(self.snappy) as f64,
            noise: channel.noise.next(self.noise_rate),
        };
        (self.trigger.at(ch), controls)
    }
}

impl crate::types::PatchUpdateHandler for Sd909 {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.noise.seed(base, ch);
        }
        let count = self.channel_count();
        self.state
            .groups
            .layout::<SdInputs<&[f32]>>(&mut self.channel_state, count);
    }
}

message_handlers!(impl Sd909 {});
