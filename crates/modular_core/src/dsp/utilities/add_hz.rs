use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::{hz_to_voct, voct_to_hz};
use crate::poly::{PolyOutput, PolySignal};

/// Lower bound for the post-offset frequency, keeping `hz_to_voct` away from log2(≤0).
const MIN_HZ: f32 = 1e-4;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct AddHzParams {
    /// V/Oct pitch signal to detune
    #[signal(type = pitch)]
    input: PolySignal,
    /// frequency offset added to the input pitch, in Hz
    offset: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct AddHzOutputs {
    #[output("output", "V/Oct pitch output", default)]
    sample: PolyOutput,
}

/// Offsets a pitch by an absolute frequency amount, in Hz.
///
/// The input V/Oct pitch is converted to Hz, **offset** Hz is added, then the
/// result is converted back to V/Oct. Useful for audible beating detune.
///
/// ```js
/// // detune a saw by +0.5 Hz for slow beating
/// $addHz($saw('C4'), 0.5)
/// ```
#[module(name = "$addHz", args(input, offset))]
pub struct AddHz {
    outputs: AddHzOutputs,
    params: AddHzParams,
}

impl AddHz {
    fn update(&mut self, _sample_rate: f32) {
        let channels = self.channel_count();

        for i in 0..channels as usize {
            let freq = voct_to_hz(self.params.input.get_value(i));
            let result = (freq + self.params.offset.get_value(i)).max(MIN_HZ);
            self.outputs.sample.set(i, hz_to_voct(result));
        }
    }
}

message_handlers!(impl AddHz {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::utils::C4_HZ_F32;
    use crate::types::{OutputStruct, Signal};

    fn make(input_voct: f32, offset_hz: f32) -> AddHz {
        let mut outputs = AddHzOutputs::default();
        outputs.set_all_channels(1);
        AddHz {
            params: AddHzParams {
                input: PolySignal::mono(Signal::Volts(input_voct)),
                offset: PolySignal::mono(Signal::Volts(offset_hz)),
            },
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
        }
    }

    #[test]
    fn adds_hz_offset() {
        // C4 (0 V) with no offset stays at 0 V.
        let mut m = make(0.0, 0.0);
        m.update(48000.0);
        assert!(m.outputs.sample.get(0).abs() < 1e-5);

        // Adding C4's own frequency doubles it → up exactly one octave (+1 V).
        let mut m = make(0.0, C4_HZ_F32);
        m.update(48000.0);
        assert!((m.outputs.sample.get(0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn floors_non_positive_frequency() {
        // A large negative offset drives frequency ≤ 0; output must stay finite.
        let mut m = make(0.0, -10_000.0);
        m.update(48000.0);
        assert!(m.outputs.sample.get(0).is_finite());
    }
}
