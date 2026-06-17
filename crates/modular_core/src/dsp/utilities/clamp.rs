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
    #[output("output", "clamped signal output", default, range = (-5.0, 5.0), dynamic_range)]
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
/// $clamp(signal, { min: 0 })
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

            // Compose the output range as the image of the input's declared
            // range under the clamp transfer function. The clamp is monotonic,
            // so the image of `[in_min, in_max]` is just its endpoints passed
            // through the same bound logic the value path uses — always
            // ordered, and correct even when the input range lies wholly
            // outside the window (the output collapses onto one boundary,
            // giving a degenerate `[c, c]`). Fall back to the static output
            // range when the input range is unknown.
            let (in_min, in_max) = self
                .params
                .input
                .get_range(i)
                .unwrap_or((f32::NEG_INFINITY, f32::INFINITY));
            let (lo, hi) = match (has_min, has_max) {
                (true, true) => {
                    let a = self.params.min.value_or_zero(i);
                    let b = self.params.max.value_or_zero(i);
                    // Order the window the same way the value path does above.
                    let (win_lo, win_hi) = if b < a { (b, a) } else { (a, b) };
                    (in_min.clamp(win_lo, win_hi), in_max.clamp(win_lo, win_hi))
                }
                (true, false) => {
                    let min_val = self.params.min.value_or_zero(i);
                    (in_min.max(min_val), in_max.max(min_val))
                }
                (false, true) => {
                    let max_val = self.params.max.value_or_zero(i);
                    (in_min.min(max_val), in_max.min(max_val))
                }
                (false, false) => (in_min, in_max),
            };
            // Publish only when both ends are finite. A one-sided clamp over an
            // input with no declared range stays half-infinite — there is no
            // meaningful bounded range to publish, so the static fallback
            // applies. The monotonic image is already ordered, so no `lo <= hi`
            // guard is needed.
            if lo.is_finite() && hi.is_finite() {
                self.outputs.sample.set_range(i, lo, hi);
            }
        }
    }
}

message_handlers!(impl Clamp {});
