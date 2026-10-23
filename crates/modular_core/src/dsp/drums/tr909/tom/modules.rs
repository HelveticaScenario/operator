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
        parts = $parts:ident $(,)?
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
                /// raises the accent voltage, for a hit up to twice as loud
                /// (default 0)
                #[signal(default = 0.0, range = (0.0, 5.0))]
                #[deserr(default)]
                accent: Option<PolySignal>,
                /// tune control, 0–5 V: the drum's pitch (default 2.5)
                #[signal(default = 2.5, range = (0.0, 5.0))]
                #[deserr(default)]
                tune: Option<PolySignal>,
                /// decay control, 0–5 V: how long the drum rings (default 2.5)
                #[signal(default = 2.5, range = (0.0, 5.0))]
                #[deserr(default)]
                decay: Option<PolySignal>,
                /// fade out a still-sounding hit over 2 ms as the next one
                /// strikes, so every hit starts from rest; otherwise hits add
                /// to the envelopes, as on the hardware (default false)
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
                    init(&mut self.state, $parts, sample_rate);
                }

                fn update(&mut self, _sample_rate: f32) {
                    let channels = self.channel_count();
                    let p = &self.params;
                    let inputs = TomInputs {
                        trigger: &p.trigger,
                        accent: &p.accent,
                        tune: &p.tune,
                        decay: &p.decay,
                        choke: p.choke,
                    };
                    update(
                        &self.state,
                        inputs,
                        &mut self.channel_state[..channels],
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
    /// TR-909 low tom.
    ///
    /// Each rising edge on **trigger** fires the 909's trigger into a circuit
    /// model of its low tom: three triangle VCOs, their pitch bent down from
    /// the strike, one clipped ever harder as it fades, the others rounded
    /// toward sines, over a burst of noise. **tune** sets the pitch, **decay**
    /// how long the drum rings, and **accent**, sampled at each strike,
    /// drives it harder.
    ///
    /// An unaccented hit peaks near **±5 V**.
    ///
    /// ```js
    /// $tr909.lt($clock.beatTrigger).out()
    /// ```
    name = "$tr909.lt",
    ident = Lt909,
    parts = LOW,
}

tom_module! {
    /// TR-909 mid tom.
    ///
    /// The low tom's circuit with smaller capacitors: it rings higher and
    /// shorter. **tune** sets the pitch, **decay** how long the drum rings,
    /// and **accent**, sampled at each strike, drives it harder.
    ///
    /// An unaccented hit peaks near **±5 V**.
    ///
    /// ```js
    /// $tr909.mt($clock.beatTrigger).out()
    /// ```
    name = "$tr909.mt",
    ident = Mt909,
    parts = MID,
}

tom_module! {
    /// TR-909 high tom.
    ///
    /// The low tom's circuit with smaller capacitors still: the highest and
    /// shortest of the three. **tune** sets the pitch, **decay** how long
    /// the drum rings, and **accent**, sampled at each strike, drives it
    /// harder.
    ///
    /// An unaccented hit peaks near **±5 V**.
    ///
    /// ```js
    /// $tr909.ht($clock.beatTrigger).out()
    /// ```
    name = "$tr909.ht",
    ident = Ht909,
    parts = HIGH,
}
