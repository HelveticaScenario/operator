use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::{hz_to_voct, voct_to_hz};
use crate::poly::{PolyOutput, PolySignal};

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
#[module(name = "$mulHz", args(input, factor))]
pub struct MulHz {
    outputs: MulHzOutputs,
    params: MulHzParams,
}

impl MulHz {
    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();

        for i in 0..channels as usize {
            let freq = voct_to_hz(self.params.input.get_value(i));
            let result = (freq * self.params.factor.get_value(i)).max(MIN_HZ);
            self.outputs.sample.set(i, hz_to_voct(result));
        }
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
