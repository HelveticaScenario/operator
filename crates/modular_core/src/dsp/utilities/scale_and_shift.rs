use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::block_input;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};
use crate::types::SwapGlide;

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
    channel_state: Box<[ChannelState]>,
}

#[derive(Clone, Copy, Default)]
struct ChannelState {
    scale: SwapGlide,
    shift: SwapGlide,
}

/// `input` scaled (5 V is unity gain) and then offset.
#[inline(always)]
fn scale_and_shift(input: f32, scale: f32, shift: f32) -> f32 {
    input * (scale / 5.0) + shift
}

impl ScaleAndShift {
    /// Outside a glide, reads each param raw, exactly as `process_block` does.
    fn update(&mut self, sample_rate: f32) {
        for i in 0..self.channel_count() {
            let state = &mut self.channel_state[i];
            let scale = glided(
                &mut state.scale,
                &self.params.scale,
                ScaleAndShiftParams::SCALE_DEFAULT,
                i,
                sample_rate,
            );
            let shift = glided(
                &mut state.shift,
                &self.params.shift,
                ScaleAndShiftParams::SHIFT_DEFAULT,
                i,
                sample_rate,
            );
            let value = scale_and_shift(self.params.input.get_value(i), scale, shift);
            self.outputs.sample.set(i, value);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut ScaleAndShiftBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let (scale, shift) = (&self.params.scale, &self.params.shift);
        let gliding = self.channel_state[..channels]
            .iter()
            .enumerate()
            .any(|(i, cs)| {
                cs.scale.will_glide(scale.glide_mark(i)) || cs.shift.will_glide(shift.glide_mark(i))
            });
        if gliding {
            return None;
        }
        let mut scratch = Default::default();
        let inputs = self
            .params
            .block_inputs(range.clone(), channels, &mut scratch)?;
        let last = range.end - 1;
        block_input::map(
            &mut outputs.sample,
            range,
            [inputs.input, inputs.scale, inputs.shift],
            |[x, scale, shift]| scale_and_shift(x, scale, shift),
        );
        for (i, cs) in self.channel_state[..channels].iter_mut().enumerate() {
            cs.scale
                .settle(inputs.scale.row(last)[i], scale.glide_mark(i));
            cs.shift
                .settle(inputs.shift.row(last)[i], shift.glide_mark(i));
        }
        Some(())
    }
}

/// The glided value of an optional param at channel `ch`; the raw value while
/// no glide runs.
#[inline(always)]
fn glided(
    glide: &mut SwapGlide,
    param: &Option<PolySignal>,
    default: f32,
    ch: usize,
    sample_rate: f32,
) -> f32 {
    glide.follow(
        param.value_or(ch, default),
        param.glide_mark(ch),
        sample_rate,
    )
}

message_handlers!(impl ScaleAndShift {});
