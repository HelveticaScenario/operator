//! TR-808 closed hi-hat; the circuit is in [`super::hh`].

use deserr::Deserr;
use schemars::JsonSchema;

use super::hh::{Closed, HatShared};
use super::metal::{MetalBank, MetalFilters};
use crate::dsp::drums::voice::Voices;
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal};

/// Scales the output so an unaccented hit peaks near 5 V.
const OUTPUT_GAIN: f32 = 3.55;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Ch808Params {
    /// rising edges strike the hat
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and held for the hit; raises
    /// the trigger pulse, for a louder hit (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// fade out a still-sounding hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the envelope, as on
    /// the hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Ch808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 closed hi-hat.
///
/// Each rising edge on **trigger** fires the 808's trigger pulse into a
/// circuit model of its closed hat: six square-wave oscillators, filtered to
/// a bright band, gated by a short envelope and high-passed. **accent**,
/// sampled at each strike, drives it harder.
///
/// On the 808 a closed hat also chokes the open hat: patch the same trigger
/// into `$tr808.oh`'s **close** input.
///
/// An unaccented hit peaks near **±5 V**.
///
/// ```js
/// $tr808.ch($clock.beatTrigger).out()
/// ```
#[module(name = "$tr808.ch", has_init, patch_update, args(trigger))]
pub struct Ch808 {
    outputs: Ch808Outputs,
    params: Ch808Params,
    state: ChState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct ChState {
    shared: HatShared,
    filters: MetalFilters,
    seeded: bool,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Closed>,
    bank: MetalBank,
}

impl Ch808 {
    fn init(&mut self, sample_rate: f32) {
        self.state.shared = HatShared::new(sample_rate);
        self.state.filters = MetalFilters::new(sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let st = &self.state;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];
            let (band, _) = cs.bank.step(&st.filters);
            let out = cs.voices.process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                st.shared.timing.choke_samples,
                &st.shared,
                &band,
            );
            self.outputs.sample.set(ch, out * OUTPUT_GAIN);
        }
    }
}

impl crate::types::PatchUpdateHandler for Ch808 {
    fn on_patch_update(&mut self) {
        if !self.state.seeded {
            let base = seed_base(self);
            for (ch, cs) in self.channel_state.iter_mut().enumerate() {
                cs.bank
                    .seed(base ^ (ch as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            }
            self.state.seeded = true;
        }
    }
}

message_handlers!(impl Ch808 {});
