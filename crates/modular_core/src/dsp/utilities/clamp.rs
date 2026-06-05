use deserr::Deserr;
use schemars::JsonSchema;

use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ClampParams {
    /// signal to clamp
    input: PolySignal,
    /// lower bound — if omitted the signal is unclamped below
    #[deserr(default)]
    min: Option<PolySignal>,
    /// upper bound — if omitted the signal is unclamped above
    #[deserr(default)]
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
/// $clamp($sine('440hz'), 0, 5).out()
/// ```
#[module(name = "$clamp", args(input))]
pub struct Clamp {
    outputs: ClampOutputs,
    params: ClampParams,
}

impl Clamp {
    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();
        let has_min = !self.params.min.is_disconnected();
        let has_max = !self.params.max.is_disconnected();

        for i in 0..channels as usize {
            let mut val = self.params.input.get_value(i);

            match (has_min, has_max) {
                (true, true) => {
                    let a = self.params.min.value_or_zero(i);
                    let b = self.params.max.value_or_zero(i);
                    let (lo, hi) = if b < a { (b, a) } else { (a, b) };
                    val = val.clamp(lo, hi);
                }
                (true, false) => {
                    let min_val = self.params.min.value_or_zero(i);
                    if val < min_val {
                        val = min_val;
                    }
                }
                (false, true) => {
                    let max_val = self.params.max.value_or_zero(i);
                    if val > max_val {
                        val = max_val;
                    }
                }
                (false, false) => {}
            }

            self.outputs.sample.set(i, val);
        }
    }
}

message_handlers!(impl Clamp {});
