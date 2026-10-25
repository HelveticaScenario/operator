use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::block_input::{self, InputScratch};
use crate::poly::{PolyOutput, PolySignal};
use crate::types::SwapGlide;

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
/// $curve($sine('1hz'), 2)   // quadratic curve
/// $curve($saw('c3'), 3)     // cubic curve (audio taper)
/// ```
#[module(name = "$curve", args(input, exp), block)]
pub struct Curve {
    outputs: CurveOutputs,
    params: CurveParams,
    channel_state: Box<[SwapGlide]>,
}

/// `x` bent by `exp` across ±5 V, keeping its sign; a negative exponent
/// counts as 0.
#[inline(always)]
fn curve(x: f32, exp: f32) -> f32 {
    let normalized = (x.abs() / 5.0).max(0.0);
    x.signum() * 5.0 * normalized.powf(exp.max(0.0))
}

impl Curve {
    fn update(&mut self, sample_rate: f32) {
        let exp = &self.params.exp;
        for i in 0..self.channel_count() {
            let exp_value =
                self.channel_state[i].follow(exp.get_value(i), exp.glide_mark(i), sample_rate);
            let value = curve(self.params.input.get_value(i), exp_value);
            self.outputs.sample.set(i, value);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut CurveBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let glides = &mut self.channel_state[..channels];
        let exp = &self.params.exp;
        if exp.glides_pending(glides) {
            return None;
        }
        let (mut s0, mut s1) = (InputScratch::new(), InputScratch::new());
        let input = self
            .params
            .input
            .block_input(range.clone(), channels, &mut s0)?;
        let exp_input = exp.block_input(range.clone(), channels, &mut s1)?;
        exp.settle_glides(glides, exp_input.row(range.end - 1));
        block_input::map(
            &mut outputs.sample,
            range,
            [input, exp_input],
            |[x, exp]| curve(x, exp),
        );
        Some(())
    }
}

message_handlers!(impl Curve {});
