use std::f32::consts::PI;

use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::utils::sanitize,
    poly::{PolyOutput, PolySignal},
};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct DcBlockParams {
    /// signal input
    input: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DcBlockOutputs {
    #[output("output", "DC-blocked signal", default)]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct DcBlockChannelState {
    prev_in: f32,
    prev_out: f32,
    initialized: bool,
}

/// State for the DcBlock module.
#[derive(Default)]
struct DcBlockState {
    coeff: f32,
}

/// DC-blocker corner frequency, in Hz. Matches the inline blocker in
/// `$overdrive`: low enough to leave audio untouched, high enough to strip a
/// constant offset within a few milliseconds.
const DC_BLOCK_FC_HZ: f32 = 20.0;

/// DC blocker — removes the DC (0 Hz) offset from a signal.
///
/// A first-order high-pass filter (`y = x − x_prev + R·y_prev`) with a fixed
/// ~20 Hz corner. It strips a constant offset while passing audio essentially
/// untouched, running independently on every channel of a polyphonic input.
/// For a tunable corner use `$hpf`.
///
/// Useful after asymmetric waveshaping or a non-square `$pulse`, whose duty
/// cycle leaves a duty-dependent DC offset.
///
/// ```js
/// // recenter an asymmetric pulse on 0 V
/// $dcBlock($pulse('c2', { width: 1 }))
/// ```
#[module(name = "$dcBlock", args(input), has_init)]
pub struct DcBlock {
    outputs: DcBlockOutputs,
    params: DcBlockParams,
    state: DcBlockState,
    channel_state: Box<[DcBlockChannelState]>,
}

impl DcBlock {
    /// Compute the sample-rate-dependent coefficient once on the main thread.
    /// `R → 1` as the sample rate rises; the `clamp` keeps the pole stable for
    /// pathologically low sample rates.
    fn init(&mut self, sample_rate: f32) {
        let rate = sample_rate.max(1.0);
        self.state.coeff = (1.0 - (2.0 * PI * DC_BLOCK_FC_HZ / rate)).clamp(0.0, 1.0);
    }

    fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();
        let coeff = self.state.coeff;

        for ch in 0..num_channels {
            let input = self.params.input.get_value(ch);
            let state = &mut self.channel_state[ch];

            if !state.initialized {
                // Prime the input history so a steady offset present from the
                // first sample produces no start-up transient.
                state.prev_in = input;
                state.initialized = true;
            }

            let out = sanitize(input - state.prev_in + coeff * state.prev_out);
            state.prev_in = input;
            state.prev_out = out;
            self.outputs.sample.set(ch, out);
        }
    }
}

message_handlers!(impl DcBlock {});
