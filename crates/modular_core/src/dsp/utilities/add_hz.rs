use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::block_input::{self, InputScratch};
use crate::dsp::utils::{hz_to_voct, voct_to_hz};
use crate::poly::{PolyOutput, PolySignal};
use crate::types::SwapGlide;

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
#[module(name = "$addHz", args(input, offset), block)]
pub struct AddHz {
    outputs: AddHzOutputs,
    params: AddHzParams,
    channel_state: Box<[SwapGlide]>,
}

/// The V/Oct pitch `voct` moved by `offset` Hz.
#[inline(always)]
fn add_hz(voct: f32, offset: f32) -> f32 {
    hz_to_voct((voct_to_hz(voct) + offset).max(MIN_HZ))
}

impl AddHz {
    fn update(&mut self, sample_rate: f32) {
        let offset = &self.params.offset;
        for i in 0..self.channel_count() {
            let offset_value = self.channel_state[i].follow(
                offset.get_value(i),
                offset.glide_mark(i),
                sample_rate,
            );
            let value = add_hz(self.params.input.get_value(i), offset_value);
            self.outputs.sample.set(i, value);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut AddHzBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let glides = &mut self.channel_state[..channels];
        let offset = &self.params.offset;
        if offset.glides_pending(glides) {
            return None;
        }
        let (mut s0, mut s1) = (InputScratch::new(), InputScratch::new());
        let input = self
            .params
            .input
            .block_input(range.clone(), channels, &mut s0)?;
        let offset_input = offset.block_input(range.clone(), channels, &mut s1)?;
        offset.settle_glides(glides, offset_input.row(range.end - 1));
        block_input::map(
            &mut outputs.sample,
            range,
            [input, offset_input],
            |[x, offset]| add_hz(x, offset),
        );
        Some(())
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
            channel_state: Box::new([SwapGlide::default()]),
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
