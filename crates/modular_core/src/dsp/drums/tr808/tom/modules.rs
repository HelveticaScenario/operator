use std::ops::Range;

use super::*;
use crate::dsp::drums::input::{Input, Live, each_slot};
use crate::dsp::drums::voice::rails;

/// Seeds each channel's noise from the module's [`seed_base`].
fn seed(base: u64, channels: &mut [Channel]) {
    for (ch, cs) in channels.iter_mut().enumerate() {
        cs.noise.seed(base, ch);
    }
}

/// The inputs every channel of a tom or conga reads.
struct TomInputs<'a, I> {
    parts: &'static Parts,
    st: &'a TomState,
    trigger: I,
    freq: I,
    decay: I,
}

impl<I: Input> Channels<TomDrum> for TomInputs<'_, I> {
    type Channel = Channel;

    fn voices(channel: &mut Channel) -> &mut Voices<Voice> {
        &mut channel.voices
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, cs: &mut Channel) -> (f32, Controls) {
        let freq = self.freq.at(ch);
        let decay = self.decay.at(ch);
        if freq != cs.network.freq || decay != cs.network.decay {
            cs.network = Network::new(self.parts, freq, decay, self.st.rest_hz);
        }
        let controls = Controls {
            t: cs.network.t,
            white: cs.noise.next(self.st.noise_scale),
            level: cs.network.level,
        };
        (self.trigger.at(ch), controls)
    }
}

impl TomModule {
    /// Runs one sample of every channel of the drum built from `parts` on
    /// its `[trigger, accent, freq, decay]`, handing `out` each one's
    /// output.
    #[inline(always)]
    fn tick<I: Input>(
        &mut self,
        parts: &'static Parts,
        channels: &mut [Channel],
        restart: bool,
        [trigger, accent, freq, decay]: [I; 4],
        mut out: impl FnMut(usize, f32),
    ) {
        let run = Run {
            accent,
            restart,
            fade_samples: self.st.timing.fade_samples,
            shared: &self.st,
        };
        let inputs = TomInputs {
            parts,
            st: &self.st,
            trigger,
            freq,
            decay,
        };
        self.groups.run(&inputs, channels, &run, |ch, v| {
            out(ch, rails(v * parts.gain))
        });
    }
}

macro_rules! tom_module {
    (
        $(#[$doc:meta])*
        name = $name:literal,
        ident = $Struct:ident,
        parts = $parts:ident,
        freq = $freq:literal,
        freq_range = ($freq_min:literal, $freq_max:literal),
        decay = $decay:literal,
        freq_doc = $freq_doc:literal,
        decay_doc = $decay_doc:literal $(,)?
    ) => {
        paste::paste! {
            #[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
            #[serde(rename_all = "camelCase")]
            #[deserr(rename_all = camelCase, deny_unknown_fields)]
            struct [<$Struct Params>] {
                /// rising edges strike the drum
                #[signal(type = trig, range = (0.0, 5.0))]
                trigger: PolySignal,
                /// accent, 0–5 V, sampled on each trigger and held for the hit;
                /// raises the trigger pulse from 4 V to 8 V, for a hit up to
                /// twice as loud with a sharper pitch drop (default 0)
                #[signal(default = 0.0, range = (0.0, 5.0))]
                #[deserr(default)]
                accent: Option<PolySignal>,
                #[doc = $freq_doc]
                #[signal(type = pitch, default = $freq, range = ($freq_min, $freq_max))]
                #[deserr(default)]
                freq: Option<PolySignal>,
                #[doc = $decay_doc]
                #[signal(default = $decay, range = (0.05, 3.0))]
                #[deserr(default)]
                decay: Option<PolySignal>,
                /// fade out a still-ringing hit over 2 ms as the next one
                /// strikes, so every hit starts from rest; otherwise hits add
                /// to the ring, as on the hardware (default false)
                #[serde(default)]
                #[deserr(default)]
                restart: bool,
            }

            #[derive(Outputs, JsonSchema)]
            #[serde(rename_all = "camelCase")]
            struct [<$Struct Outputs>] {
                #[output("output", "drum output", default, range = (-10.0, 10.0))]
                sample: PolyOutput,
            }

            $(#[$doc])*
            #[module(name = $name, has_init, patch_update, args(trigger), block)]
            pub struct $Struct {
                outputs: [<$Struct Outputs>],
                params: [<$Struct Params>],
                state: TomModule,
                channel_state: Box<[Channel]>,
            }

            impl $Struct {
                fn init(&mut self, sample_rate: f32) {
                    self.state.st.init(&$parts, sample_rate);
                    self.state.groups = Groups::new();
                }

                fn update(&mut self, _sample_rate: f32) {
                    let channels = self.channel_count();
                    let p = &self.params;
                    let inputs = [
                        Live::new(&p.trigger),
                        Live::or(&p.accent, [<$Struct Params>]::ACCENT_DEFAULT),
                        Live::or(&p.freq, [<$Struct Params>]::FREQ_DEFAULT),
                        Live::or(&p.decay, [<$Struct Params>]::DECAY_DEFAULT),
                    ];
                    let out = &mut self.outputs.sample;
                    self.state.tick(
                        &$parts,
                        &mut self.channel_state[..channels],
                        p.restart,
                        inputs,
                        |ch, v| out.set(ch, v),
                    );
                }

                fn process_block(
                    &mut self,
                    outputs: &mut [<$Struct BlockOutputs>],
                    range: Range<usize>,
                    _sample_rate: f32,
                ) -> Option<()> {
                    let channels = self.channel_count();
                    let p = &self.params;
                    let mut scratch = Default::default();
                    let [<$Struct ParamsBlockInputs>] {
                        trigger,
                        accent,
                        freq,
                        decay,
                    } = p.block_inputs(range.clone(), channels, &mut scratch)?;
                    let channel_state = &mut self.channel_state[..channels];
                    each_slot(
                        &mut outputs.sample,
                        range,
                        [trigger, accent, freq, decay],
                        |inputs, mut slot| {
                            self.state.tick(
                                &$parts,
                                channel_state,
                                p.restart,
                                inputs,
                                |ch, v| slot.set(ch, v),
                            )
                        },
                    );
                    Some(())
                }
            }

            impl crate::types::PatchUpdateHandler for $Struct {
                fn on_patch_update(&mut self) {
                    let base = seed_base(self);
                    seed(base, &mut self.channel_state);
                    let count = self.channel_count();
                    self.state
                        .groups
                        .layout::<TomInputs<&[f32]>>(&mut self.channel_state, count);
                }
            }

            message_handlers!(impl $Struct {});
        }
    };
}

tom_module! {
    /// TR-808 low tom.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its low tom: a bridged-T filter rings at ≈87 Hz,
    /// starting sharp and settling as its diodes stop conducting, over a burst
    /// of low pink noise that swells in a few milliseconds after the hit.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.lowTom($clock.beatTrigger).out()
    /// ```
    name = "$tr808.lowTom",
    ident = Lt808,
    parts = LT,
    freq = -1.5959,
    freq_range = (-3.1, -0.1),
    decay = 0.634,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈87 Hz (default -1.5959)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.63 s (default 0.634)",
}

tom_module! {
    /// TR-808 mid tom.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its mid tom: a bridged-T filter rings at ≈134 Hz,
    /// starting sharp and settling as its diodes stop conducting, over a burst
    /// of low pink noise that swells in a few milliseconds after the hit.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.midTom($clock.beatTrigger).out()
    /// ```
    name = "$tr808.midTom",
    ident = Mt808,
    parts = MT,
    freq = -0.9623,
    freq_range = (-2.46, 0.54),
    decay = 0.399,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈134 Hz (default -0.9623)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.40 s (default 0.399)",
}

tom_module! {
    /// TR-808 high tom.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its high tom: a bridged-T filter rings at ≈182 Hz,
    /// starting sharp and settling as its diodes stop conducting, over a burst
    /// of low pink noise that swells in a few milliseconds after the hit.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.highTom($clock.beatTrigger).out()
    /// ```
    name = "$tr808.highTom",
    ident = Ht808,
    parts = HT,
    freq = -0.5216,
    freq_range = (-2.02, 0.98),
    decay = 0.306,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈182 Hz (default -0.5216)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.31 s (default 0.306)",
}

tom_module! {
    /// TR-808 low conga.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its low conga: a bridged-T filter rings at ≈192 Hz,
    /// starting sharp and settling as its diodes stop conducting.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.lowConga($clock.beatTrigger).out()
    /// ```
    name = "$tr808.lowConga",
    ident = Lc808,
    parts = LC,
    freq = -0.4471,
    freq_range = (-1.95, 1.05),
    decay = 0.634,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈192 Hz (default -0.4471)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.63 s (default 0.634)",
}

tom_module! {
    /// TR-808 mid conga.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its mid conga: a bridged-T filter rings at ≈276 Hz,
    /// starting sharp and settling as its diodes stop conducting.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.midConga($clock.beatTrigger).out()
    /// ```
    name = "$tr808.midConga",
    ident = Mc808,
    parts = MC,
    freq = 0.0792,
    freq_range = (-1.42, 1.58),
    decay = 0.306,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈276 Hz (default 0.0792)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.31 s (default 0.306)",
}

tom_module! {
    /// TR-808 high conga.
    ///
    /// Each rising edge on **trigger** fires the 808's 1 ms trigger pulse into
    /// a circuit model of its high conga: a bridged-T filter rings at ≈405 Hz,
    /// starting sharp and settling as its diodes stop conducting.
    /// **accent**, sampled at each strike, drives the circuit harder.
    ///
    /// An unaccented hit peaks near **±5 V** and a fully accented one near
    /// **±10 V**.
    ///
    /// ```js
    /// $tr808.highConga($clock.beatTrigger).out()
    /// ```
    name = "$tr808.highConga",
    ident = Hc808,
    parts = HC,
    freq = 0.629,
    freq_range = (-0.87, 2.13),
    decay = 0.306,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈405 Hz (default 0.629)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.31 s (default 0.306)",
}
