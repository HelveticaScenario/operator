use schemars::JsonSchema;
use serde::Deserialize;

use crate::poly::{MonoSignal, MonoSignalExt, PolyOutput};

fn default_count() -> usize {
    2
}

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct SpreadParams {
    /// lower bound of the spread range
    min: Option<MonoSignal>,
    /// upper bound of the spread range
    max: Option<MonoSignal>,
    /// number of output channels (1–16)
    #[serde(default = "default_count")]
    count: usize,
    /// distribution bias (-5 to 5): positive biases toward max, negative toward min
    #[signal(default = 0.0, range = (-5.0, 5.0))]
    bias: Option<MonoSignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SpreadOutputs {
    #[output("output", "spread signal output", default)]
    sample: PolyOutput,
}

/// Produces a multi-channel output by linearly interpolating between a
/// minimum and maximum value, with an optional bias to skew the
/// distribution.
///
/// Each output channel gets an evenly spaced value between **min** and
/// **max**. The **bias** control reshapes the distribution: positive
/// values push channels toward **max**, negative values toward **min**.
///
/// | count | behaviour |
/// |-------|-----------|
/// | 1     | single value between min and max, positioned by bias |
/// | 2     | channel 0 = min, channel 1 = max (with bias = 0) |
/// | N     | N evenly spaced values from min to max (with bias = 0) |
///
/// ```js
/// // spread 4 oscillators across a frequency range
/// $sine($spread(0, 5, 4))
///
/// // 8 channels biased toward the minimum
/// $spread(0, 5, 8, { bias: -3 })
/// ```
#[module(name = "$spread", channels_param = "count", args(min, max, count))]
pub struct Spread {
    outputs: SpreadOutputs,
    params: SpreadParams,
}

impl Spread {
    fn update(&mut self, _sample_rate: f32) {
        let count = self.channel_count().max(1) as usize;
        let min_val = self.params.min.value_or(0.0);
        let max_val = self.params.max.value_or(0.0);
        let bias = self.params.bias.value_or(0.0);

        // Compute the power curve exponent from bias.
        // bias = 0 → exponent = 1 (linear)
        // bias > 0 → exponent < 1 (values shift toward max)
        // bias < 0 → exponent > 1 (values shift toward min)
        let exponent = (2.0_f32).powf(-bias);

        for i in 0..count {
            let t = if count == 1 {
                0.5_f32
            } else {
                i as f32 / (count - 1) as f32
            };

            // Apply bias curve
            let t_biased = t.powf(exponent);

            let value = min_val + (max_val - min_val) * t_biased;
            self.outputs.sample.set(i, value);
        }
    }
}

message_handlers!(impl Spread {});
