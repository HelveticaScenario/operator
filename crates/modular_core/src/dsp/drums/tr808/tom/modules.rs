use super::*;

/// Seeds each channel's noise from the module's [`seed_base`].
fn seed(base: u64, channels: &mut [Channel]) {
    for (ch, cs) in channels.iter_mut().enumerate() {
        cs.noise.seed(base, ch);
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
            #[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
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
                choke: bool,
            }

            #[derive(Outputs, JsonSchema)]
            #[serde(rename_all = "camelCase")]
            struct [<$Struct Outputs>] {
                #[output("output", "drum output", default, range = (-10.0, 10.0))]
                sample: PolyOutput,
            }

            $(#[$doc])*
            #[module(name = $name, has_init, patch_update, args(trigger))]
            pub struct $Struct {
                outputs: [<$Struct Outputs>],
                params: [<$Struct Params>],
                state: TomState,
                channel_state: Box<[Channel]>,
            }

            impl $Struct {
                fn init(&mut self, sample_rate: f32) {
                    self.state.init(&$parts, sample_rate);
                }

                fn update(&mut self, _sample_rate: f32) {
                    let channels = self.channel_count();
                    let p = &self.params;
                    process(
                        &$parts,
                        &self.state,
                        &mut self.channel_state[..channels],
                        &p.trigger,
                        &p.accent,
                        &p.freq,
                        &p.decay,
                        p.choke,
                        $freq,
                        $decay,
                        &mut self.outputs.sample,
                    );
                }
            }

            impl crate::types::PatchUpdateHandler for $Struct {
                fn on_patch_update(&mut self) {
                    let base = seed_base(self);
                    seed(base, &mut self.channel_state);
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
    /// $tr808.lt($clock.beatTrigger).out()
    /// ```
    name = "$tr808.lt",
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
    /// $tr808.mt($clock.beatTrigger).out()
    /// ```
    name = "$tr808.mt",
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
    /// $tr808.ht($clock.beatTrigger).out()
    /// ```
    name = "$tr808.ht",
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
    /// $tr808.lc($clock.beatTrigger).out()
    /// ```
    name = "$tr808.lc",
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
    /// $tr808.mc($clock.beatTrigger).out()
    /// ```
    name = "$tr808.mc",
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
    /// $tr808.hc($clock.beatTrigger).out()
    /// ```
    name = "$tr808.hc",
    ident = Hc808,
    parts = HC,
    freq = 0.629,
    freq_range = (-0.87, 2.13),
    decay = 0.306,
    freq_doc = "resting pitch in V/Oct, scaling the bridged-T; stock is ≈405 Hz (default 0.629)",
    decay_doc = "seconds for the ring to fall 60 dB; stock is 0.31 s (default 0.306)",
}
