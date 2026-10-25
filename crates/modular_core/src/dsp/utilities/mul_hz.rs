use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::block_input::{self, InputScratch};
use crate::dsp::utils::{hz_to_voct, voct_to_hz};
use crate::poly::{PolyOutput, PolySignal};
use crate::types::SwapGlide;

/// Lower bound for the post-scale frequency, keeping `hz_to_voct` away from log2(≤0).
const MIN_HZ: f32 = 1e-4;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct MulHzParams {
    /// V/Oct pitch signal to scale
    #[signal(type = pitch)]
    input: PolySignal,
    /// frequency multiplier (1 = unity, 2 = octave up, 0.5 = octave down)
    factor: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MulHzOutputs {
    #[output("output", "V/Oct pitch output", default)]
    sample: PolyOutput,
}

/// Multiplies a pitch's frequency by a factor.
///
/// The input V/Oct pitch is converted to Hz, multiplied by **factor**, then the
/// result is converted back to V/Oct. A factor of 1.5 is a just perfect fifth.
///
/// ```js
/// // up a just fifth
/// $mulHz($saw('C4'), 1.5)
/// ```
#[module(name = "$mulHz", args(input, factor), block)]
pub struct MulHz {
    outputs: MulHzOutputs,
    params: MulHzParams,
    channel_state: Box<[SwapGlide]>,
}

/// The V/Oct pitch `voct` with its frequency multiplied by `factor`.
#[inline(always)]
fn mul_hz(voct: f32, factor: f32) -> f32 {
    hz_to_voct((voct_to_hz(voct) * factor).max(MIN_HZ))
}

impl MulHz {
    fn update(&mut self, sample_rate: f32) {
        let factor = &self.params.factor;
        for i in 0..self.channel_count() {
            let factor_value = self.channel_state[i].follow(
                factor.get_value(i),
                factor.glide_mark(i),
                sample_rate,
            );
            let value = mul_hz(self.params.input.get_value(i), factor_value);
            self.outputs.sample.set(i, value);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut MulHzBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let glides = &mut self.channel_state[..channels];
        let factor = &self.params.factor;
        if factor.glides_pending(glides) {
            return None;
        }
        let (mut s0, mut s1) = (InputScratch::new(), InputScratch::new());
        let input = self
            .params
            .input
            .block_input(range.clone(), channels, &mut s0)?;
        let factor_input = factor.block_input(range.clone(), channels, &mut s1)?;
        factor.settle_glides(glides, factor_input.row(range.end - 1));
        block_input::map(
            &mut outputs.sample,
            range,
            [input, factor_input],
            |[x, factor]| mul_hz(x, factor),
        );
        Some(())
    }
}

message_handlers!(impl MulHz {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    fn make(input_voct: f32, factor: f32) -> MulHz {
        let mut outputs = MulHzOutputs::default();
        outputs.set_all_channels(1);
        MulHz {
            params: MulHzParams {
                input: PolySignal::mono(Signal::Volts(input_voct)),
                factor: PolySignal::mono(Signal::Volts(factor)),
            },
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
            channel_state: Box::new([SwapGlide::default()]),
        }
    }

    #[test]
    fn multiplies_frequency() {
        // Unity factor leaves the pitch unchanged.
        let mut m = make(0.0, 1.0);
        m.update(48000.0);
        assert!(m.outputs.sample.get(0).abs() < 1e-5);

        // Factor 2 = up one octave (+1 V).
        let mut m = make(0.0, 2.0);
        m.update(48000.0);
        assert!((m.outputs.sample.get(0) - 1.0).abs() < 1e-5);

        // Factor 0.5 = down one octave (-1 V).
        let mut m = make(0.0, 0.5);
        m.update(48000.0);
        assert!((m.outputs.sample.get(0) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn floors_non_positive_frequency() {
        // Factor 0 drives frequency to 0; output must stay finite.
        let mut m = make(0.0, 0.0);
        m.update(48000.0);
        assert!(m.outputs.sample.get(0).is_finite());
    }
}
