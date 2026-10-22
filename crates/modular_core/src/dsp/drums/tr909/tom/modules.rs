use std::ops::Range;

use super::*;
use crate::dsp::drums::input::{Cached, Input, Live, each_slot};
use crate::dsp::drums::voice::rails;

fn noise_network(dt: f32) -> Mna<NOISE_UNKNOWNS, NOISE_CAPS, 0, 1> {
    let mut b = MnaBuilder::<NOISE_UNKNOWNS>::default();
    b.resistor(NOISE_IN, GROUND, 1.0 / STIFF)
        .resistor(NOISE_IN, N1, 22.0e3)
        .capacitor(N1, GROUND, 0.0047e-6)
        .capacitor(N1, N2, 0.0047e-6)
        .capacitor(N2, N3, 0.0047e-6)
        .resistor(N2, HIGH_OUT, 10.0e3)
        .resistor(N3, GROUND, 220.0e3)
        .opamp(N3, HIGH_OUT, HIGH_OUT, OP27)
        .capacitor(HIGH_OUT, N53, 0.047e-6);
    b.build(dt, [])
}

/// The tom at every width.
struct TomDrum;

impl Drum for TomDrum {
    type Shared = TomShared;
    type Circuit<L: Lanes> = Tom<L>;
    type Controls<L: Lanes> = TomControls<L>;
    type Sources<L: Lanes> = ();
    const QUAD: bool = true;
    const PAIR: bool = true;
}

#[derive(Default)]
struct TomState {
    shared: TomShared,
    groups: Groups<TomDrum>,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Tom>,
    noise: Noise,
    rate: Cached<f32, f32>,
}

/// Sets up a tom of `parts` at `sample_rate`.
fn init(state: &mut TomState, parts: TomParts, sample_rate: f32) {
    let dt = 1.0 / sample_rate;
    state.groups = Groups::new();
    let sh = &mut state.shared;
    sh.parts = Some(parts);
    sh.timing = Timing::new(sample_rate);
    sh.dt = dt;
    sh.noise_rate = Noise::rate(sample_rate);
    // The noise network at DC: discretised at a step long enough that its
    // capacitors carry no current.
    let dc = noise_network(DC_STEP);
    let mut injected = [0.0f32; NOISE_UNKNOWNS];
    injected[NOISE_IN] = NOISE_MEAN * STIFF;
    let mut rest = MnaState::default();
    for _ in 0..DC_ITERATIONS {
        dc.step(&mut rest, 0, &injected);
    }
    sh.noise_rest = rest;
    sh.noise = noise_network(dt);
    sh.reset = parts.vco.map(|c| (-dt / (R_RESET * c)).exp());
    sh.fall16 = (-dt / (R60 * C16)).exp();
    sh.fall17 = (-dt / (R61 * C17)).exp();
}

/// The inputs every tom reads.
struct TomInputs<I> {
    trigger: I,
    freq: I,
    decay: I,
    /// The stock pitch, V/Oct.
    stock: f32,
    noise_rate: f32,
}

impl<I: Input> Channels<TomDrum> for TomInputs<I> {
    type Channel = Channel;

    fn voices(channel: &mut Channel) -> &mut Voices<Tom> {
        &mut channel.voices
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, channel: &mut Channel) -> (f32, TomControls) {
        let knob = |s: I| s.at(ch).clamp(0.0, 5.0) / 5.0;
        let controls = TomControls {
            rate: channel
                .rate
                .get(self.freq.at(ch), |f| (f - self.stock).exp2()),
            decay: 1.0 / (R111 + VR11 * knob(self.decay)),
            noise: channel.noise.next(self.noise_rate),
        };
        (self.trigger.at(ch), controls)
    }
}

impl TomState {
    /// Runs one sample of every channel on its `[trigger, accent, freq,
    /// decay]`, with `stock` the tom's stock pitch, handing `out` each one's
    /// output.
    #[inline(always)]
    fn tick<I: Input>(
        &mut self,
        channels: &mut [Channel],
        restart: bool,
        stock: f32,
        [trigger, accent, freq, decay]: [I; 4],
        mut out: impl FnMut(usize, f32),
    ) {
        let run = Run {
            accent,
            restart,
            fade_samples: self.shared.timing.fade_samples,
            shared: &self.shared,
        };
        let inputs = TomInputs {
            trigger,
            freq,
            decay,
            stock,
            noise_rate: self.shared.noise_rate,
        };
        self.groups.run(&inputs, channels, &run, |ch, v| {
            out(ch, rails(v * OUTPUT_GAIN))
        });
    }
}

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
        freq_doc = $freq_doc:literal $(,)?
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
                /// raises the accent voltage, for a hit up to twice as loud
                /// (default 0)
                #[signal(default = 0.0, range = (0.0, 5.0))]
                #[deserr(default)]
                accent: Option<PolySignal>,
                #[doc = $freq_doc]
                #[signal(type = pitch, default = $freq, range = ($freq_min, $freq_max))]
                #[deserr(default)]
                freq: Option<PolySignal>,
                /// decay control, 0–5 V: how long the drum rings (default 2.5)
                #[signal(default = 2.5, range = (0.0, 5.0))]
                #[deserr(default)]
                decay: Option<PolySignal>,
                /// fade out a still-sounding hit over 2 ms as the next one
                /// strikes, so every hit starts from rest; otherwise hits add
                /// to the envelopes, as on the hardware (default false)
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
                    let inputs = [
                        Live::new(&p.trigger),
                        Live::or(&p.accent, [<$Struct Params>]::ACCENT_DEFAULT),
                        Live::or(&p.freq, [<$Struct Params>]::FREQ_DEFAULT),
                        Live::or(&p.decay, [<$Struct Params>]::DECAY_DEFAULT),
                    ];
                    let out = &mut self.outputs.sample;
                    self.state
                        .tick(
                            &mut self.channel_state[..channels],
                            p.restart,
                            [<$Struct Params>]::FREQ_DEFAULT,
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
                            self.state
                                .tick(
                                    channel_state,
                                    p.restart,
                                    [<$Struct Params>]::FREQ_DEFAULT,
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
    /// TR-909 low tom.
    ///
    /// Each rising edge on **trigger** fires the 909's trigger into a circuit
    /// model of its low tom: three triangle VCOs, their pitch bent down from
    /// the strike, one clipped ever harder as it fades, the others rounded
    /// toward sines, over a burst of noise. **freq** sets the pitch, **decay**
    /// how long the drum rings, and **accent**, sampled at each strike,
    /// drives it harder.
    ///
    /// An unaccented hit peaks near **±5 V**.
    ///
    /// ```js
    /// $tr909.lowTom($clock.beatTrigger).out()
    /// ```
    name = "$tr909.lowTom",
    ident = Lt909,
    parts = LOW,
    freq = -2.0993,
    freq_range = (-3.6, -0.6),
    freq_doc = "resting pitch of VCO-2 in V/Oct, scaling all three VCOs and their bend; stock is ≈61 Hz (default -2.0993)",
}

tom_module! {
    /// TR-909 mid tom.
    ///
    /// The low tom's circuit with smaller capacitors: it rings higher and
    /// shorter. **freq** sets the pitch, **decay** how long the drum rings,
    /// and **accent**, sampled at each strike, drives it harder.
    ///
    /// An unaccented hit peaks near **±5 V**.
    ///
    /// ```js
    /// $tr909.midTom($clock.beatTrigger).out()
    /// ```
    name = "$tr909.midTom",
    ident = Mt909,
    parts = MID,
    freq = -1.8098,
    freq_range = (-3.3, -0.3),
    freq_doc = "resting pitch of VCO-2 in V/Oct, scaling all three VCOs and their bend; stock is ≈75 Hz (default -1.8098)",
}

tom_module! {
    /// TR-909 high tom.
    ///
    /// The low tom's circuit with smaller capacitors still: the highest and
    /// shortest of the three. **freq** sets the pitch, **decay** how long
    /// the drum rings, and **accent**, sampled at each strike, drives it
    /// harder.
    ///
    /// An unaccented hit peaks near **±5 V**.
    ///
    /// ```js
    /// $tr909.highTom($clock.beatTrigger).out()
    /// ```
    name = "$tr909.highTom",
    ident = Ht909,
    parts = HIGH,
    freq = -1.5143,
    freq_range = (-3.0, 0.0),
    freq_doc = "resting pitch of VCO-2 in V/Oct, scaling all three VCOs and their bend; stock is ≈92 Hz (default -1.5143)",
}
