use schemars::JsonSchema;
use serde::Deserialize;

use crate::poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS};
use crate::types::Clickless;

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct RemapParams {
    /// signal input to remap
    input: PolySignal,
    /// minimum of input range
    #[signal(default = -5.0)]
    in_min: Option<PolySignal>,
    /// maximum of input range
    #[signal(default = 5.0)]
    in_max: Option<PolySignal>,
    /// minimum of output range
    #[signal(default = -5.0)]
    out_min: Option<PolySignal>,
    /// maximum of output range
    #[signal(default = 5.0)]
    out_max: Option<PolySignal>,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    in_min: Clickless,
    in_max: Clickless,
    out_min: Clickless,
    out_max: Clickless,
}

/// State for the Remap module.
#[derive(Default)]
struct RemapState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RemapOutputs {
    #[output("output", "remapped signal output", default)]
    sample: PolyOutput,
}

/// Linearly rescales a signal from one voltage range to another.
///
/// Maps **input** from \[inMin, inMax\] to \[outMin, outMax\]. Useful for
/// converting between different voltage standards or reshaping control
/// signals.
///
/// ```js
/// // convert a 0–5 V envelope to a -5–5 V bipolar signal
/// $remap(env, -5, 5, 0, 5)
///
/// // convert a -5–5 V signal to 0–1 V
/// $remap(signal, 0, 1, -5, 5)
/// ```
#[module(name = "$remap", args(input, outMin, outMax, inMin, inMax))]
pub struct Remap {
    outputs: RemapOutputs,
    state: RemapState,
    params: RemapParams,
}

impl Remap {
    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();

        for i in 0..channels as usize {
            let input_val = self.params.input.get_value(i);
            let state = &mut self.state.channels[i];

            // Smooth range parameters to avoid clicks
            state.in_min.update(self.params.in_min.value_or(i, -5.0));
            state.in_max.update(self.params.in_max.value_or(i, 5.0));
            state.out_min.update(self.params.out_min.value_or(i, -5.0));
            state.out_max.update(self.params.out_max.value_or(i, 5.0));

            // Apply remapping using map_range utility
            let output = crate::dsp::utils::map_range(
                input_val,
                *state.in_min,
                *state.in_max,
                *state.out_min,
                *state.out_max,
            );

            self.outputs.sample.set(i, output);
        }
    }
}

message_handlers!(impl Remap {});
