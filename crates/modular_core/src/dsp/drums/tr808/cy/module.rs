use std::ops::Range;

use super::*;
use crate::dsp::drums::input::{Input, Live, each_slot};
use crate::dsp::drums::voice::rails;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Cy808Params {
    /// rising edges strike the cymbal
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled at each strike and held for the hit: up to twice
    /// as loud (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// decay control, 0–5 V: how long the cymbal's lower band rings on (default
    /// 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// tone control, 0–5 V: from dark, with the high band cut, to bright
    /// (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    tone: Option<PolySignal>,
    /// fade out a hit still sounding over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise each hit builds on the last, as on
    /// the 808 (default false)
    #[serde(default)]
    #[deserr(default)]
    restart: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Cy808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 cymbal.
///
/// A circuit model of the 808's cymbal: six detuned square waves filtered into
/// a shimmering high band and a ringing lower one. **decay** sets how long the
/// lower band rings, **tone** the balance from dark to bright, and **accent**
/// how hard it hits.
///
/// An unaccented hit peaks near **±5 V** and a fully accented one near **±10
/// V**.
///
/// ```js
/// $tr808.cymbal($clock.barTrigger).out()
/// ```
#[module(name = "$tr808.cymbal", has_init, patch_update, args(trigger), block)]
pub struct Cy808 {
    outputs: Cy808Outputs,
    params: Cy808Params,
    state: CyState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct CyState {
    shared: CymbalShared,
    filters: MetalFilters,
    groups: Groups<CymbalDrum>,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Cymbal>,
    bank: MetalBank,
}

/// Conductance of a linear pot half at `fraction` of its track `ohms`.
fn pot(ohms: f32, fraction: f32) -> f32 {
    1.0 / (ohms * fraction.clamp(0.0, 1.0) + POT_END)
}

impl Cy808 {
    fn init(&mut self, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        let sh = &mut self.state.shared;
        sh.timing = Timing::new(sample_rate);
        sh.dt = dt;
        sh.smooth_coeff = 1.0 - (-dt / SMOOTH_TAU).exp();
        sh.env = envelope_network(dt);
        sh.high = high_network(dt);
        sh.mid = mid_network(dt);
        sh.low = low_network(dt);
        sh.tone = tone_network(dt);
        sh.rest = SwingState::rest();
        self.state.filters = MetalFilters::new(sample_rate);
        self.state.groups = Groups::new();
    }

    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let p = &self.params;
        let inputs = [
            Live::new(&p.trigger),
            Live::or(&p.accent, Cy808Params::ACCENT_DEFAULT),
            Live::or(&p.decay, Cy808Params::DECAY_DEFAULT),
            Live::or(&p.tone, Cy808Params::TONE_DEFAULT),
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
        outputs: &mut Cy808BlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let mut scratch = Default::default();
        let Cy808ParamsBlockInputs {
            trigger,
            accent,
            decay,
            tone,
        } = p.block_inputs(range.clone(), channels, &mut scratch)?;
        let channel_state = &mut self.channel_state[..channels];
        each_slot(
            &mut outputs.sample,
            range,
            [trigger, accent, decay, tone],
            |inputs, mut slot| {
                self.state
                    .tick(channel_state, p.restart, inputs, |ch, v| slot.set(ch, v))
            },
        );
        Some(())
    }
}

impl CyState {
    /// Runs one sample of every channel on its `[trigger, accent, decay,
    /// tone]`, handing `out` each one's output.
    #[inline(always)]
    fn tick<I: Input>(
        &mut self,
        channels: &mut [Channel],
        restart: bool,
        [trigger, accent, decay, tone]: [I; 4],
        mut out: impl FnMut(usize, f32),
    ) {
        let run = Run {
            accent,
            restart,
            fade_samples: self.shared.timing.fade_samples,
            shared: &self.shared,
        };
        let inputs = CyInputs {
            trigger,
            decay,
            tone,
            filters: &self.filters,
        };
        self.groups.run(&inputs, channels, &run, |ch, v| {
            out(ch, rails(v * OUTPUT_GAIN))
        });
    }
}

/// The inputs every channel of the cymbal reads.
struct CyInputs<'a, I> {
    trigger: I,
    decay: I,
    tone: I,
    filters: &'a MetalFilters,
}

impl<I: Input> CyInputs<'_, I> {
    /// Channel `ch`'s decay and tone conductances.
    #[inline]
    fn pots(&self, ch: usize) -> (f32, [f32; 2]) {
        let decay = self.decay.at(ch).clamp(0.0, 5.0) / 5.0;
        let tone = self.tone.at(ch).clamp(0.0, 5.0) / 5.0;
        (
            1.0 / (R93 + VR2 * decay),
            [pot(VR4, tone), pot(VR4, 1.0 - tone)],
        )
    }
}

impl<I: Input> Channels<CymbalDrum> for CyInputs<'_, I> {
    type Channel = Channel;

    fn voices(channel: &mut Channel) -> &mut Voices<Cymbal> {
        &mut channel.voices
    }

    #[inline(always)]
    fn inputs(&self, ch: usize, cs: &mut Channel) -> (f32, CymbalControls) {
        let (high, low) = cs.bank.step(self.filters);
        let (decay, tone) = self.pots(ch);
        let controls = CymbalControls {
            high,
            low,
            decay,
            tone,
        };
        (self.trigger.at(ch), controls)
    }

    #[inline]
    fn group_inputs<L: Lanes>(
        &self,
        first: usize,
        members: &mut [Channel],
        bank: &mut MetalBank<L>,
        triggers: &mut [f32; QUAD],
    ) -> CymbalControls<L> {
        let (high, low) = bank.step(self.filters);
        let mut controls = CymbalControls::<L> {
            high,
            low,
            ..Default::default()
        };
        for (lane, trigger) in triggers.iter_mut().take(members.len()).enumerate() {
            let ch = first + lane;
            *trigger = self.trigger.at(ch);
            let (decay, tone) = self.pots(ch);
            controls.decay.set_lane(lane, &decay);
            controls.tone.set_lane(lane, &tone);
        }
        controls
    }

    fn load_sources<L: Lanes>(cs: &mut Channel, lane: usize, bank: &mut MetalBank<L>) {
        bank.set_lane(lane, &cs.bank);
    }

    fn unload_sources<L: Lanes>(cs: &mut Channel, lane: usize, bank: &MetalBank<L>) {
        cs.bank = bank.lane(lane);
    }
}

impl crate::types::PatchUpdateHandler for Cy808 {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.bank.seed(base, ch);
        }
        let count = self.channel_count();
        self.state
            .groups
            .layout::<CyInputs<&[f32]>>(&mut self.channel_state, count);
    }
}

message_handlers!(impl Cy808 {});
