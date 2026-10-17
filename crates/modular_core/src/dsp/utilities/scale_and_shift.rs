use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::block_input;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams, BlockInputs)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ScaleAndShiftParams {
    /// signal to scale and shift
    input: PolySignal,
    /// scale factor (0–10V range; 5V = unity gain, 0V = silence, -5V = inverted, 10V = 2x)
    #[signal(default = 5.0, range = (0.0, 10.0))]
    #[deserr(default)]
    scale: Option<PolySignal>,
    /// DC offset added to the scaled signal (in volts)
    #[deserr(default)]
    shift: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ScaleAndShiftOutputs {
    #[output("output", "signal output", default)]
    sample: PolyOutput,
}

/// Scales and offsets a signal — the classic attenuverter + DC offset.
///
/// - **scale** — gain factor (0–10 V; 5 V = unity, 0 V = silence,
///   values above 5 V amplify, negative values invert).
/// - **shift** — DC offset added after scaling (in volts).
///
/// ```js
/// // invert a slow sine and shift it into 0–5 V range
/// $scaleAndShift($sine('1hz'), -2.5, 2.5)
/// ```
#[module(name = "$scaleAndShift", args(input, scale, shift), block)]
pub struct ScaleAndShift {
    outputs: ScaleAndShiftOutputs,
    params: ScaleAndShiftParams,
}

/// `input` scaled (5 V is unity gain) and then offset.
#[inline(always)]
fn scale_and_shift(input: f32, scale: f32, shift: f32) -> f32 {
    input * (scale / 5.0) + shift
}

impl ScaleAndShift {
    fn update(&mut self, _sample_rate: f32) {
        for i in 0..self.channel_count() {
            let value = scale_and_shift(
                self.params.input.get_value(i),
                self.params.scale.value_or(i, 5.0),
                self.params.shift.value_or(i, 0.0),
            );
            self.outputs.sample.set(i, value);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut ScaleAndShiftBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let mut scratch = Default::default();
        let inputs = self
            .params
            .block_inputs(range.clone(), self.channel_count(), &mut scratch)?;
        block_input::map(
            &mut outputs.sample,
            range,
            [inputs.input, inputs.scale, inputs.shift],
            |[x, scale, shift]| x * (scale / 5.0) + shift,
        );
        Some(())
    }
}

message_handlers!(impl ScaleAndShift {});
