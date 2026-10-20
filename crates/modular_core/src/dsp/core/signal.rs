use deserr::Deserr;
use schemars::JsonSchema;

use crate::poly::{PolyOutput, PolySignal};

#[derive(Clone, Deserr, JsonSchema, ChannelCount, SignalParams, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct SignalParams {
    /// Input signal to forward.
    value: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SignalOutputs {
    #[output("output", "forwarded signal", default)]
    sample: PolyOutput,
}

/// Utility module for routing, naming, and exposing signals in a patch.
#[module(name = "$signal", args(value))]
pub struct Signal {
    outputs: SignalOutputs,
    params: SignalParams,
}

impl Signal {
    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        for i in 0..channels as usize {
            let val = self.params.value.get_value(i);
            self.outputs.sample.set(i, val);
        }
    }
}

message_handlers!(impl Signal {});
