//! `$pWarp` — applies a `$phase.*` warp to a phase signal.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::phase_warp::{PhaseWarp, PhaseWarpGlides, PhaseWarpState};
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct PWarpParams {
    /// input phase (0 to 1)
    #[signal(range = (0.0, 1.0))]
    input: PolySignal,
    /// the warp to apply, built with `$phase.*`
    phase: PhaseWarp,
    /// pitch of the phase in V/Oct (optional, reduces aliasing of the pulsar
    /// and feedback warps at high frequencies)
    #[signal(type = pitch)]
    #[deserr(default)]
    freq: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PWarpOutputs {
    #[output("output", "warped phase output", default, range = (0.0, 1.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    warp: PhaseWarpState,
    glides: PhaseWarpGlides,
}

/// As wide as the input, the warp's signals, and `freq`.
#[allow(private_interfaces)]
pub fn p_warp_derive_channel_count(params: &PWarpParams) -> usize {
    params
        .input
        .channels()
        .max(params.phase.channels())
        .max(params.freq.channel_count())
        .clamp(1, PORT_MAX_CHANNELS)
}

/// Phase effect: reshapes a 0–1 phase signal with a `$phase.*` warp.
///
/// Feed the output into a phase oscillator (`$pSine`, `$pSaw`, `$pPulse`) to
/// hear the result. Warps compose with `.pipe()` or their optional second
/// argument; `$phase.feedback` stages keep their own state per channel.
///
/// # Example
///
/// ```js
/// // Ensemble Oscillator-style pulsar and crush on a ramp
/// $pSine($pWarp($ramp('c3'), $phase.pulsar(3))).out()
/// $pSine($pWarp($ramp('c3'), $phase.crush(2, $phase.feedback(1)))).out()
/// ```
#[module(name = "$pWarp", channels_derive = p_warp_derive_channel_count, args(input, phase))]
pub struct PWarp {
    outputs: PWarpOutputs,
    channel_state: Box<[ChannelState]>,
    params: PWarpParams,
}

impl PWarp {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let freq_connected = !self.params.freq.is_disconnected();

        for ch in 0..num_channels {
            let freq_norm = if freq_connected {
                voct_to_hz(self.params.freq.value_or_zero(ch)) / sample_rate
            } else {
                0.0
            };
            let input = self.params.input.get_value(ch).clamp(0.0, 1.0);
            let state = &mut self.channel_state[ch];
            let resolved = self
                .params
                .phase
                .resolve(ch, sample_rate, &mut state.glides);
            let y = self
                .params
                .phase
                .evaluate(input, &resolved, freq_norm, &mut state.warp);
            self.outputs.sample.set(ch, y);
        }
    }
}

message_handlers!(impl PWarp {});
