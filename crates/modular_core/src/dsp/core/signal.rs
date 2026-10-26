use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::block_input::{self, InputScratch};
use crate::poly::{PolyOutput, PolySignal};
use crate::types::SwapGlide;

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
#[module(name = "$signal", args(value), block)]
pub struct Signal {
    outputs: SignalOutputs,
    params: SignalParams,
    channel_state: Box<[SwapGlide]>,
}

impl Signal {
    fn update(&mut self, sample_rate: f32) {
        let value = &self.params.value;
        for i in 0..self.channel_count() {
            let v =
                self.channel_state[i].follow(value.get_value(i), value.glide_mark(i), sample_rate);
            self.outputs.sample.set(i, v);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut SignalBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let glides = &mut self.channel_state[..channels];
        let value = &self.params.value;
        if value.glides_pending(glides) {
            return None;
        }
        let mut scratch = InputScratch::new();
        let input = value.block_input(range.clone(), channels, &mut scratch)?;
        value.settle_glides(glides, input.row(range.end - 1));
        block_input::map(&mut outputs.sample, range, [input], |[x]| x);
        Some(())
    }
}

message_handlers!(impl Signal {});
