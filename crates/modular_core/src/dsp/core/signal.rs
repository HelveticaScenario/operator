use schemars::JsonSchema;
use serde::Deserialize;

use crate::poly::{PolyOutput, PolySignal};

#[derive(Clone, Deserialize, JsonSchema, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct SignalParams {
    /// Input signal to forward.
    source: PolySignal,
}

impl crate::types::Connect for SignalParams {
    fn connect(&mut self, patch: &crate::Patch) {
        self.source.connect(patch);
    }
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SignalOutputs {
    #[output("output", "forwarded signal", default)]
    sample: PolyOutput,
}

/// Utility module for routing, naming, and exposing signals in a patch.
#[module(name = "$signal", args(source))]
pub struct Signal {
    outputs: SignalOutputs,
    params: SignalParams,
}

impl Signal {
    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        for i in 0..channels as usize {
            let val = self.params.source.get_value(i);
            self.outputs.sample.set(i, val);
        }
    }
}

message_handlers!(impl Signal {});
