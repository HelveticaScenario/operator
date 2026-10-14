use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::block_input;
use crate::poly::{PolyOutput, PolySignal};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ClampParams {
    /// signal to clamp
    input: PolySignal,
    /// lower bound — if omitted the signal is unclamped below
    #[deserr(default)]
    #[block_input(optional)]
    min: Option<PolySignal>,
    /// upper bound — if omitted the signal is unclamped above
    #[deserr(default)]
    #[block_input(optional)]
    max: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ClampOutputs {
    #[output("output", "clamped signal output", default)]
    sample: PolyOutput,
}

/// Constrains a signal between a minimum and maximum value.
///
/// Bounds are independently optional — omit **min** or **max** to leave
/// that side unclamped.
///
/// ```js
/// // clamp a sine into the 0–5 V range
/// $clamp($sine('440hz'), 0, 5)
///
/// // one-sided: floor at 0 V, no ceiling
/// $clamp($sine('1hz'), { min: 0 })
/// ```
#[module(name = "$clamp", args(input), block)]
pub struct Clamp {
    outputs: ClampOutputs,
    params: ClampParams,
}

/// `x` held within the connected bounds; two bounds in either order.
#[inline(always)]
fn clamp(x: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    match (min, max) {
        (Some(a), Some(b)) => {
            let (lo, hi) = if b < a { (b, a) } else { (a, b) };
            x.clamp(lo, hi)
        }
        (Some(min), None) if x < min => min,
        (None, Some(max)) if x > max => max,
        _ => x,
    }
}

impl Clamp {
    fn update(&mut self, _sample_rate: f32) {
        let p = &self.params;
        for i in 0..self.channel_count() {
            let min = p.min.as_ref().map(|min| min.get_value(i));
            let max = p.max.as_ref().map(|max| max.get_value(i));
            self.outputs
                .sample
                .set(i, clamp(p.input.get_value(i), min, max));
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut ClampBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let mut scratch = Default::default();
        let inputs = self
            .params
            .block_inputs(range.clone(), self.channel_count(), &mut scratch)?;
        let (input, out) = (inputs.input, &mut outputs.sample);
        match (inputs.min, inputs.max) {
            (Some(min), Some(max)) => {
                block_input::map(out, range, [input, min, max], |[x, min, max]| {
                    clamp(x, Some(min), Some(max))
                });
            }
            (Some(min), None) => {
                block_input::map(out, range, [input, min], |[x, min]| {
                    clamp(x, Some(min), None)
                });
            }
            (None, Some(max)) => {
                block_input::map(out, range, [input, max], |[x, max]| {
                    clamp(x, None, Some(max))
                });
            }
            (None, None) => block_input::map(out, range, [input], |[x]| x),
        }
        Some(())
    }
}

message_handlers!(impl Clamp {});
