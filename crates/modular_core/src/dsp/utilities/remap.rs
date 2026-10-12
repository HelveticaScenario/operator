use deserr::Deserr;
use schemars::JsonSchema;

use std::ops::Range;

use crate::block_input::InputScratch;
use crate::dsp::utils::{map_range, sanitize};
use crate::poly::{PolyOutput, PolySignal};
use crate::types::Clickless;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct RemapParams {
    /// signal input to remap
    input: PolySignal,
    /// minimum of input range
    #[signal(default = -5.0)]
    in_min: PolySignal,
    /// maximum of input range
    #[signal(default = 5.0)]
    in_max: PolySignal,
    /// minimum of output range
    #[signal(default = -5.0)]
    out_min: PolySignal,
    /// maximum of output range
    #[signal(default = 5.0)]
    out_max: PolySignal,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    in_min: Clickless,
    in_max: Clickless,
    out_min: Clickless,
    out_max: Clickless,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RemapOutputs {
    #[output("output", "remapped signal output", default)]
    sample: PolyOutput,
}

/// Linearly rescales a signal from one voltage range to another.
///
/// Maps **input** from \[inMin, inMax\] to \[outMin, outMax\]. Useful for
/// converting between different voltage standards or reshaping control
/// signals.
///
/// ```js
/// // convert a 0–5 V envelope to a -5–5 V bipolar signal
/// $remap($adsr($clock.beatTrigger, {attack: 0.3, decay: 0.2, sustain: 4, release: 0.5}), -5, 5, 0, 5)
///
/// // convert a -5–5 V signal to 0–1 V
/// $remap($sine('1hz'), 0, 1, -5, 5)
/// ```
#[module(name = "$remap", args(input, outMin, outMax, inMin, inMax), block)]
pub struct Remap {
    outputs: RemapOutputs,
    channel_state: Box<[ChannelState]>,
    params: RemapParams,
}

impl ChannelState {
    /// Smooth the range toward its targets, so changes do not click, and map
    /// `x` through it.
    #[inline(always)]
    fn step(&mut self, x: f32, in_min: f32, in_max: f32, out_min: f32, out_max: f32) -> f32 {
        self.in_min.update(in_min);
        self.in_max.update(in_max);
        self.out_min.update(out_min);
        self.out_max.update(out_max);
        map_range(x, *self.in_min, *self.in_max, *self.out_min, *self.out_max)
    }
}

impl Remap {
    fn update(&mut self, _sample_rate: f32) {
        let p = &self.params;
        for (i, state) in self.channel_state.iter_mut().enumerate() {
            let value = state.step(
                p.input.get_value(i),
                p.in_min.get_value(i),
                p.in_max.get_value(i),
                p.out_min.get_value(i),
                p.out_max.get_value(i),
            );
            self.outputs.sample.set(i, value);
        }
    }

    fn process_block(
        &mut self,
        outputs: &mut RemapBlockOutputs,
        range: Range<usize>,
        _sample_rate: f32,
    ) -> Option<()> {
        let channels = self.channel_count();
        let p = &self.params;
        let (mut s0, mut s1, mut s2, mut s3, mut s4) = (
            InputScratch::new(),
            InputScratch::new(),
            InputScratch::new(),
            InputScratch::new(),
            InputScratch::new(),
        );
        let input = p.input.block_input(range.clone(), channels, &mut s0)?;
        let in_min = p.in_min.block_input(range.clone(), channels, &mut s1)?;
        let in_max = p.in_max.block_input(range.clone(), channels, &mut s2)?;
        let out_min = p.out_min.block_input(range.clone(), channels, &mut s3)?;
        let out_max = p.out_max.block_input(range.clone(), channels, &mut s4)?;
        for i in range {
            let rows = input
                .row(i)
                .iter()
                .zip(in_min.row(i))
                .zip(in_max.row(i))
                .zip(out_min.row(i))
                .zip(out_max.row(i));
            let slots = outputs
                .sample
                .slot_mut(i)
                .iter_mut()
                .zip(self.channel_state.iter_mut());
            for ((out, state), ((((&x, &in_min), &in_max), &out_min), &out_max)) in slots.zip(rows)
            {
                *out = sanitize(state.step(x, in_min, in_max, out_min, out_max));
            }
        }
        Some(())
    }
}

message_handlers!(impl Remap {});
