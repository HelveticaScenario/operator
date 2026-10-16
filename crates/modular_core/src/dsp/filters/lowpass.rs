use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::filters::resonant_svf::{self, Response, SvfChannel, SvfFilterState},
    poly::{PolyOutput, PolySignal},
};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct LowpassFilterParams {
    /// signal input
    input: PolySignal,
    /// cutoff frequency in V/Oct (0V = C4)
    #[signal(type = frequency, default = 0.0, range = (-5.0, 5.0))]
    cutoff: PolySignal,
    /// filter resonance (0-5)
    #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
    #[deserr(default)]
    resonance: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LowpassFilterOutputs {
    #[output("output", "filtered signal", default)]
    sample: PolyOutput,
}

/// Lowpass filter that attenuates frequencies above the cutoff point.
///
/// Use it to tame bright timbres, create bass-heavy sounds, or build classic
/// subtractive synth patches. Sweeping the cutoff with an envelope or LFO
/// produces the familiar filter-sweep effect.
///
/// - **cutoff** — set in V/Oct (0 V = C4). Accepts modulation for filter sweeps.
/// - **resonance** — boosts frequencies near the cutoff (0–5). High values
///   produce a ringing peak, but the filter never self-oscillates; use
///   `$jup6f` for a filter that does.
///
/// ```js
/// // subtractive bass: saw through a lowpass with envelope on cutoff
/// let env = $adsr($pPulse($clock[0]), { attack: 0.01, decay: 0.3, sustain: 1, release: 0.4 })
/// $lpf($saw('c2'), env.range('200hz', '2000hz'))
/// ```
#[module(name = "$lpf", patch_update, args(input, cutoff, resonance))]
pub struct LowpassFilter {
    outputs: LowpassFilterOutputs,
    state: SvfFilterState,
    channel_state: Box<[SvfChannel]>,
    params: LowpassFilterParams,
}

impl LowpassFilter {
    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        resonant_svf::run(
            Response::Lowpass,
            channels,
            &self.params.input,
            &self.params.cutoff,
            &self.params.resonance,
            0.0,
            sample_rate,
            &mut self.state,
            &mut self.channel_state,
            &mut self.outputs.sample,
        );
    }
}

impl crate::types::PatchUpdateHandler for LowpassFilter {
    fn on_patch_update(&mut self) {
        resonant_svf::on_patch_update(
            &self.params.cutoff,
            &self.params.resonance,
            &mut self.state,
            &mut self.channel_state,
        );
    }
}

message_handlers!(impl LowpassFilter {});
