use std::ops::Range;

use super::*;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Cb808Params {
    /// rising edges strike the bell
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled at each strike and held for the hit: up to twice
    /// as loud (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// pitch of the lower tone in V/Oct, the higher one following; stock is 540
    /// Hz against 800 Hz (default 1.0454)
    #[signal(type = pitch, default = 1.0454, range = (-0.5, 2.5))]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// fade out a hit still sounding over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise each hit builds on the last, as on
    /// the 808 (default false)
    #[serde(default)]
    #[deserr(default)]
    restart: bool,
    /// sample the controls on each strike and hold them until the next, as
    /// accent always is, so changes reach only the hits after them (default
    /// false)
    #[serde(default)]
    #[deserr(default)]
    hold: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Cb808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 cowbell.
///
/// A circuit model of the 808's cowbell: two square waves near 540 and 800 Hz
/// through a band-pass filter, for its clanking, slightly sour clang. **freq**
/// sets the pitch and **accent** how hard it hits.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near **±10
/// V**.
///
/// ```js
/// $tr808.cowbell($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.cowbell", has_init, patch_update, args(trigger), block)]
pub struct Cb808 {
    outputs: Cb808Outputs,
    params: Cb808Params,
    state: CbState,
    channel_state: Box<[Channel]>,
}

/// The cowbell's per-sample-rate constants and its held `freq`.
#[derive(Default)]
struct CbState {
    shared: BellShared,
    hold: Hold<1, 1>,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Bell>,
    /// The oscillators' phases, in cycles.
    phases: [f32; 2],
    seeded: bool,
    /// Their steps per sample at the channel's `freq`, and the output gain
    /// levelling it.
    tuning: Cached<f32, ([f32; 2], f32)>,
}

impl Cb808 {
    fn init(&mut self, sample_rate: f32) {
        let sh = &mut self.state.shared;
        sh.timing = Timing::new(PULSE_SECS, sample_rate);
        sh.substeps = (CB_RATE / sample_rate).round().max(1.0) as u32;
        let rate = sample_rate * sh.substeps as f32;
        sh.dt = 1.0 / rate;
        let on = 1.0 / R_ON;
        sh.net = network().build(
            sh.dt,
            [
                Switch {
                    node: ENV,
                    other: GROUND,
                    g: 1.0 / R122,
                    volts: 0.0,
                },
                Switch {
                    node: VCA_A,
                    other: GROUND,
                    g: on,
                    volts: VCA_LOW,
                },
                Switch {
                    node: VCA_B,
                    other: GROUND,
                    g: on,
                    volts: VCA_LOW,
                },
            ],
        );
        sh.couple_coeff = one_pole_coeff(rc_hz(R117, C76), rate);
    }

    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        let p = &self.params;
        let inputs = [
            Live::new(&p.trigger),
            Live::or(&p.accent, Cb808Params::ACCENT_DEFAULT),
            Live::or(&p.freq, Cb808Params::FREQ_DEFAULT),
        ];
        let out = &mut self.outputs.sample;
        tick(
            &mut self.state,
            &mut self.channel_state[..channels],
            p.restart,
            p.hold,
            sample_rate,
            inputs,
            |ch, v| out.set(ch, v),
        );
    }

    fn process_block(
        &mut self,
        outputs: &mut Cb808BlockOutputs,
        range: Range<usize>,
        sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let Cb808ParamsBlockInputs {
            trigger,
            accent,
            freq,
        } = p.block_inputs(range.clone(), channels, &mut scratch)?;
        let channel_state = &mut self.channel_state[..channels];
        each_slot(
            &mut outputs.sample,
            range,
            [trigger, accent, freq],
            |inputs, mut slot| {
                tick(
                    &mut self.state,
                    channel_state,
                    p.restart,
                    p.hold,
                    sample_rate,
                    inputs,
                    |ch, v| slot.set(ch, v),
                )
            },
        );
        Some(())
    }
}

/// Runs one sample of every channel on its `[trigger, accent, freq]`,
/// handing `out` each one's output.
#[inline(always)]
fn tick<I: Input>(
    state: &mut CbState,
    channels: &mut [Channel],
    restart: bool,
    hold: bool,
    sample_rate: f32,
    [trigger, accent, freq]: [I; 3],
    mut out: impl FnMut(usize, f32),
) {
    let [freq] = state.hold.apply(hold, channels.len(), [trigger], [freq]);
    let sh = &state.shared;
    for (ch, cs) in channels.iter_mut().enumerate() {
        let (inc, level) = cs.tuning.get(freq.at(ch), |freq| {
            let r = voct_to_hz(freq) / COWBELL_HZ[0];
            let inc = COWBELL_HZ.map(|hz| hz * r / sample_rate);
            (inc, levels::COWBELL.gain(freq))
        });
        let phases = &mut cs.phases;
        let controls = BellControls {
            low: (phases[0], inc[0]),
            high: (phases[1], inc[1]),
        };
        let v = cs.voices.process(
            trigger.at(ch),
            accent,
            ch,
            restart,
            sh.timing.fade_samples,
            sh,
            &controls,
        );
        phases[0] = (phases[0] + inc[0]).fract();
        phases[1] = (phases[1] + inc[1]).fract();
        out(ch, rails(v * OUTPUT_GAIN * level));
    }
}

impl Channel {
    /// Starts the oscillators at random phases, as the 808's run freely from
    /// power-on, unless they already are.
    fn seed(&mut self, base: u64, stream: usize) {
        if self.seeded {
            return;
        }
        self.seeded = true;
        let mut rng = LcgRng::default();
        rng.seed(base, stream);
        self.phases = [rng.next_unit(), rng.next_unit()];
    }
}

impl crate::types::PatchUpdateHandler for Cb808 {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.seed(base, ch);
        }
    }
}

message_handlers!(impl Cb808 {});
