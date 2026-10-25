use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::filters::resonant_svf::{self, Response, SvfChannel, SvfFilterState},
    poly::{PolyOutput, PolySignal},
};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct BandpassFilterParams {
    /// signal input
    input: PolySignal,
    /// center frequency in V/Oct (0V = C4)
    #[signal(type = frequency)]
    center: PolySignal,
    /// filter resonance — controls bandwidth (0–5)
    #[signal(default = 1.0, range = (0.0, 5.0))]
    #[deserr(default)]
    resonance: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BandpassFilterOutputs {
    #[output("output", "filtered signal", default)]
    sample: PolyOutput,
}

/// Bandpass filter that passes frequencies near the center frequency and
/// attenuates everything else.
///
/// Use it to isolate a frequency region, create vowel-like tones, or
/// build resonant "wah" effects by sweeping the center frequency.
///
/// - **center** — center frequency in V/Oct (0 V = C4).
/// - **resonance** — controls bandwidth (0–5). Higher values narrow the
///   passband for a more pronounced, ringing sound.
///
/// ```js
/// // resonant bandpass sweep on noise
/// $bpf($noise("white"), $sine('0.5hz').range('440hz', '1200hz'), 3)
/// ```
#[module(name = "$bpf", patch_update, args(input, center, resonance))]
pub struct BandpassFilter {
    outputs: BandpassFilterOutputs,
    state: SvfFilterState,
    channel_state: Box<[SvfChannel]>,
    params: BandpassFilterParams,
}

impl BandpassFilter {
    fn update(&mut self, sample_rate: f32) {
        let channels = self.channel_count();
        resonant_svf::run(
            Response::Bandpass,
            channels,
            &self.params.input,
            &self.params.center,
            &self.params.resonance,
            1.0,
            sample_rate,
            &mut self.state,
            &mut self.channel_state,
            &mut self.outputs.sample,
        );
    }
}

impl crate::types::PatchUpdateHandler for BandpassFilter {
    fn on_patch_update(&mut self) {
        resonant_svf::on_patch_update(
            &self.params.center,
            &self.params.resonance,
            &mut self.state,
            &mut self.channel_state,
        );
    }
}

message_handlers!(impl BandpassFilter {});
