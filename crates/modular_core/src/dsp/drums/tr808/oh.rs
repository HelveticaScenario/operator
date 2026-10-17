//! TR-808 open hi-hat; the circuit is in [`super::hh`].

use deserr::Deserr;
use schemars::JsonSchema;

use super::hh::{Closed, HatShared, Open, OpenControls};
use super::metal::{MetalBank, MetalFilters};
use crate::dsp::drums::voice::{Circuit, Strike, Voices};
use crate::dsp::utils::rng::seed_base;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

/// Scales the output so an unaccented hit peaks near 5 V.
const OUTPUT_GAIN: f32 = 2.32;

/// The decay pot, VR3: 2 MΩ linear, after R132.
const R132: f32 = 100.0e3;
const VR3: f32 = 2.0e6;
const DEFAULT_DECAY: f32 = 2.5;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct Oh808Params {
    /// rising edges strike the hat
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
    /// accent, 0–5 V, sampled on each trigger and on each close, and held
    /// for the hit; raises the trigger pulse, for a louder hit (default 0)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    accent: Option<PolySignal>,
    /// decay control, 0–5 V: how long the hat rings before it falls away
    /// (default 2.5)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    decay: Option<PolySignal>,
    /// rising edges close the hat, as the closed hi-hat's trigger does on
    /// the 808, choking a ringing hit
    #[signal(type = trig, range = (0.0, 5.0))]
    #[deserr(default)]
    close: Option<PolySignal>,
    /// fade out a still-sounding hit over 2 ms as the next one strikes, so
    /// every hit starts from rest; otherwise hits add to the envelope, as on
    /// the hardware (default false)
    #[serde(default)]
    #[deserr(default)]
    choke: bool,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Oh808Outputs {
    #[output("output", "drum output", default, range = (-10.0, 10.0))]
    sample: PolyOutput,
}

/// TR-808 open hi-hat.
///
/// Each rising edge on **trigger** fires the 808's trigger pulse into a
/// circuit model of its open hat: six square-wave oscillators, filtered to a
/// bright band, gated by an envelope that holds and then falls away, and
/// high-passed. **decay** sets how long it holds, and **accent**, sampled at
/// each strike, drives it harder.
///
/// A rising edge on **close** chokes the hat as the 808's closed hat does:
/// patch the closed hat's trigger into it.
///
/// An unaccented hit peaks near **±5 V**.
///
/// ```js
/// $tr808.oh($clock.barTrigger, { close: $clock.beatTrigger }).out()
/// ```
#[module(name = "$tr808.oh", has_init, patch_update, args(trigger))]
pub struct Oh808 {
    outputs: Oh808Outputs,
    params: Oh808Params,
    state: OhState,
    channel_state: Box<[Channel]>,
}

#[derive(Default)]
struct OhState {
    shared: HatShared,
    filters: MetalFilters,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    voices: Voices<Open>,
    bank: MetalBank,
    /// The closed hat's trigger and envelope, which drive Q23.
    close: Strike,
    closed: Closed,
}

impl Oh808 {
    fn init(&mut self, sample_rate: f32) {
        self.state.shared = HatShared::new(sample_rate);
        self.state.filters = MetalFilters::new(sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let st = &self.state;
        let sh = &st.shared;
        for ch in 0..self.channel_count() {
            let p = &self.params;
            let cs = &mut self.channel_state[ch];
            let (band, _) = cs.bank.step(&st.filters);
            if let Some(accent) = cs.close.process(p.close.value_or(ch, 0.0), &p.accent, ch) {
                cs.closed.fire(accent, sh);
            }
            // C63 can hold Q23 on after the closed hat falls silent, so its
            // circuit runs until Q23 lets go of the open hat.
            if !cs.closed.settled() || cs.closed.q23() > 0.0 {
                cs.closed.step(sh, &band);
            }
            let decay = p.decay.value_or(ch, DEFAULT_DECAY).clamp(0.0, 5.0) / 5.0;
            let controls = OpenControls {
                band,
                decay: 1.0 / (R132 + VR3 * decay),
                q23: cs.closed.q23(),
            };
            let out = cs.voices.process(
                p.trigger.get_value(ch),
                &p.accent,
                ch,
                p.choke,
                sh.timing.choke_samples,
                sh,
                &controls,
            );
            self.outputs.sample.set(ch, out * OUTPUT_GAIN);
        }
    }
}

impl crate::types::PatchUpdateHandler for Oh808 {
    fn on_patch_update(&mut self) {
        let base = seed_base(self);
        for (ch, cs) in self.channel_state.iter_mut().enumerate() {
            cs.bank.seed(base, ch);
        }
    }
}

message_handlers!(impl Oh808 {});
