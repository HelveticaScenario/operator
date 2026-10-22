use std::ops::Range;

use super::*;
use crate::dsp::drums::input::{Input, Live, each_slot};
use crate::dsp::drums::voice::rails;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Bd808Params {
    /// rising edges strike the drum
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the trigger pulse from 4 V to 8 V, for a hit up to twice as loud with
    /// more pitch sigh (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// tone control, 0–5 V: opens the output low-pass from ≈300 Hz to ≈7 kHz,
    /// letting through more of the attack click (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// seconds for the ring to fall 60 dB; the stock decay knob spans
    /// ≈0.09–3.8 s, and the circuit can ring no shorter than ≈0.09 s at stock
    /// pitch (default 1)
    #[signal(default = 1.0, range = (0.1, 4.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// resting pitch of the bridged-T in V/Oct, scaling the whole circuit;
    /// stock is ≈49.5 Hz (default -2.4035)
    #[signal(type = pitch, default = -2.4035, range = (-3.5, -1.0))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// fade out a still-ringing hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the ring, as on the
    /// hardware (default false)
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
/// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into a
/// circuit model of its bass drum: a resonant bridged-T filter rings at
/// ≈49.5 Hz with a short frequency jump on the attack and a falling pitch
/// "sigh". **decay** sets how long the ring lasts, **tone** how much of the
/// attack click comes through, and **accent**, sampled at each strike, drives
/// the circuit harder. Retriggering a ringing drum adds to its current state
/// rather than restarting it, as on the hardware, so long decays at fast
/// tempos swing in level from hit to hit; **restart** starts every hit from
/// rest instead.
///
/// An unaccented hit peaks near **±5 V** at any **freq**, and a fully
/// accented one near **±10 V**; toward the top of the **freq** range the
/// circuit's op-amp clips, so accent adds less there.
///
/// ```js
/// $tr808.kick($clock.beatTrigger, { decay: 2 }).out()
/// ```
#[module(name = "$tr808.kick", has_init, patch_update, args(trigger), block)]
pub struct Bd808 {
    outputs: Bd808Outputs,
    params: Bd808Params,
    state: BdModule,
    channel_state: Box<[ChannelState]>,
}

/// The bass drum's per-sample-rate constants and its channel groups.
#[derive(Default)]
struct BdModule {
    st: Bd808State,
    groups: Groups<BassDrum>,
}

#[derive(Clone, Copy)]
struct ChannelState {
    voices: Voices<Voice>,
    network: Network,
    /// `tone` the low-pass coefficient is computed for.
    tone: f32,
    tone_coeff: f32,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            voices: Voices::default(),
            network: Network::default(),
            // NaN never compares equal, so the first sample always computes.
            tone: f32::NAN,
            tone_coeff: 0.0,
        }
    }
}

impl Bd808 {
    fn init(&mut self, sample_rate: f32) {
        self.state.groups = Groups::new();
        let st = &mut self.state.st;
        let dt = 1.0 / sample_rate;
        st.timing = Timing::new(sample_rate);
        st.dt = dt;
        st.sample_rate = sample_rate;
        let shaper_tau = C40 * R162 * R163 / (R162 + R163);
        st.shaper_mul = (-dt / shaper_tau).exp();
        st.c38_coeff = 1.0 - (-dt / R156_C38).exp();
        st.c39_step = dt / (R161 * C39);
        st.level_hp_coeff = one_pole_coeff(LEVEL_HP_HZ, sample_rate);
        st.output_hp_coeff = one_pole_coeff(OUTPUT_HP_HZ, sample_rate);
        let rest_r = 1.0 / (1.0 / R161 + 1.0 / R170 + 1.0 / (R165 + R166));
        st.rest_hz = 1.0 / (std::f32::consts::TAU * (rest_r * R167).sqrt() * C41_42);
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

impl BdModule {
    /// Runs one sample of every channel on its `[trigger, accent, freq, decay,
    /// tone]`, handing `out` each one's output.
    #[inline(always)]
    fn tick<I: Input>(
        &mut self,
        channels: &mut [ChannelState],
        restart: bool,
        [trigger, accent, freq, decay, tone]: [I; 5],
        mut out: impl FnMut(usize, f32),
    ) {
        let run = Run {
            accent,
            restart,
            fade_samples: self.st.timing.fade_samples,
            shared: &self.st,
        };
        let inputs = BdInputs {
            st: &self.st,
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
    st: &'a Bd808State,
    trigger: I,
    freq: I,
    decay: I,
    tone: I,
}

impl<I: Input> Channels<BassDrum> for BdInputs<'_, I> {
    type Channel = ChannelState;

    fn voices(channel: &mut ChannelState) -> &mut Voices<Voice> {
        &mut channel.voices
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, cs: &mut ChannelState) -> (f32, Controls) {
        let freq = self.freq.at(ch);
        let decay = self.decay.at(ch);
        if freq != cs.network.freq || decay != cs.network.decay {
            cs.network = Network::new(freq, decay, self.st.rest_hz);
        }
        let tone = self.tone.at(ch);
        if tone != cs.tone {
            cs.tone = tone;
            cs.tone_coeff = one_pole_coeff(tone_hz(tone), self.st.sample_rate);
        }
        let controls = Controls {
            c: cs.network.c,
            a: cs.network.a,
            g: cs.network.g,
            tone_coeff: cs.tone_coeff,
            level: cs.network.level,
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
