// Test module to verify that overlapping parameter and output names
// produce a runtime panic when the schema is created.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::poly::MonoSignal;

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct TestOverlapParams {
    /// this conflicts with the output name
    output: Option<MonoSignal>,
}

/// Test module with overlapping names.
#[module(name = "$test-overlap")]
pub struct TestOverlap {
    outputs: TestOverlapOutputs,
    params: TestOverlapParams,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TestOverlapOutputs {
    #[output("output", "this conflicts with the param name", default)]
    output: f32,
}

impl TestOverlap {
    fn update(&mut self, _sample_rate: f32) {
        self.outputs.output = 1.0;
    }
}

message_handlers!(impl TestOverlap {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Module;

    #[test]
    #[should_panic(expected = "Parameters and outputs must have unique names")]
    fn test_overlapping_names_panics() {
        // This should panic when get_schema() is called
        let _schema = TestOverlap::get_schema();
    }
}
