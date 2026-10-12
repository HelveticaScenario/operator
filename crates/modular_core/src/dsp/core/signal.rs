use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::block_input::{self, InputScratch};
use crate::poly::{PolyOutput, PolySignal};

#[derive(Clone, Deserr, JsonSchema, ChannelCount, SignalParams, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct SignalParams {
    /// Input signal to forward.
    source: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SignalOutputs {
    #[output("output", "forwarded signal", default)]
    sample: PolyOutput,
}

/// Utility module for routing, naming, and exposing signals in a patch.
#[module(name = "$signal", args(source), block)]
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

    fn process_block(
        &mut self,
        outputs: &mut SignalBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let mut scratch = InputScratch::new();
        let source = self
            .params
            .source
            .block_input(range.clone(), channels, &mut scratch)?;
        block_input::map(&mut outputs.sample, range, [source], |[x]| x);
        Some(())
    }
}

message_handlers!(impl Signal {});
