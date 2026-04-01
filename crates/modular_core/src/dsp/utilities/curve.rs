use deserr::Deserr;
use schemars::JsonSchema;

use crate::poly::{PolyOutput, PolySignal};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct CurveParams {
    /// signal to apply curve to
    input: PolySignal,
    /// exponent for the power curve (0 = step, 1 = linear, >1 = audio taper)
    #[signal(range = (0.0, 10.0))]
    exp: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CurveOutputs {
    #[output("output", "curved signal output", default)]
    sample: PolyOutput,
}

/// Applies a power curve to a signal, normalised at ±5 V.
///
/// Formula: `sign(x) × 5 × (|x| / 5) ^ exp`
///
/// - **exp = 1** — linear pass-through
/// - **exp > 1** — pushes midrange toward zero (audio taper)
/// - **0 < exp < 1** — pushes midrange toward ±5 V
/// - **exp = 0** — step function (any nonzero → ±5 V)
///
/// ```js
/// $curve($sine('1hz'), 2).out()       // quadratic curve
/// ```
#[module(name = "$curve", args(input, exp))]
pub struct Curve {
    outputs: CurveOutputs,
    params: CurveParams,
}

impl Curve {
    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();

        for i in 0..channels as usize {
            let x = self.params.input.get_value(i);
            let exp = self.params.exp.get_value(i).max(0.0);

            let normalized = (x.abs() / 5.0).max(0.0);
            let curved = x.signum() * 5.0 * normalized.powf(exp);

            self.outputs.sample.set(i, curved);
        }
    }
}

message_handlers!(impl Curve {});
