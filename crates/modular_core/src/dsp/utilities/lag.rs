use crate::{
    dsp::utils::sanitize,
    poly::{PolyOutput, PolySignal, PolySignalExt},
};
use deserr::Deserr;
use schemars::JsonSchema;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct LagProcessorParams {
    /// signal input
    input: PolySignal,
    /// rise rate — seconds to slew 1 volt upward (default 0.01)
    #[signal(default = 0.01, range = (0.0, 10.0))]
    #[deserr(default)]
    rise: Option<PolySignal>,
    /// fall rate — seconds to slew 1 volt downward (default 0.01)
    #[signal(default = 0.01, range = (0.0, 10.0))]
    #[deserr(default)]
    fall: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LagProcessorOutputs {
    #[output("output", "slewed signal", default)]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct SlewChannelState {
    current_value: f32,
    initialized: bool,
}

/// Slew limiter that smooths abrupt voltage changes.
///
/// Separate **rise** and **fall** times control how quickly the output can
/// increase or decrease. Times are specified in **seconds per volt** — the
/// time the output takes to slew by 1 V. For example, a rise time of `0.1`
/// means the output climbs 1 V in 0.1 s; a 5 V gate signal would therefore
/// take 0.5 s to reach full height.
///
/// Use `$slew` to add portamento to pitch signals, smooth noisy control
/// voltages, or create envelope-like shapes from gate signals.
///
/// ```js
/// // portamento: glide between notes (0.1 s per volt of pitch change)
/// $sine($slew($cycle($p('c3 e3 g3')), { rise: 0.1, fall: 0.1 }))
/// ```
#[module(name = "$slew", args(input))]
pub struct LagProcessor {
    outputs: LagProcessorOutputs,
    params: LagProcessorParams,
    channel_state: Box<[SlewChannelState]>,
}

impl LagProcessor {
    pub fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];
            let input = self.params.input.get_value(ch);
            if !state.initialized {
                state.current_value = sanitize(input);
                state.initialized = true;
            }
            // Arrived at a held input: a step would add +0.0, which changes
            // nothing but -0.0.
            if input.to_bits() == state.current_value.to_bits()
                && input.to_bits() != (-0.0f32).to_bits()
            {
                self.outputs.sample.set(ch, state.current_value);
                continue;
            }

            let fall_time = self.params.fall.value_or(ch, 0.01).max(0.001);
            let rise_time = self.params.rise.value_or(ch, fall_time).max(0.001);

            // Calculate max change per sample
            // time is seconds for 1.0v change (full scale)
            // Slew rate = 1.0 / time (V/s)
            // Max delta per sample = Slew rate / sample_rate
            let max_rise = 1.0 / (rise_time * sample_rate);
            let max_fall = 1.0 / (fall_time * sample_rate);

            let diff = input - state.current_value;

            let change = if diff > 0.0 {
                diff.min(max_rise)
            } else {
                diff.max(-max_fall)
            };

            state.current_value += change;
            state.current_value = sanitize(state.current_value);
            self.outputs.sample.set(ch, state.current_value);
        }
    }
}

message_handlers!(impl LagProcessor {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    const SR: f32 = 48_000.0;

    fn make() -> LagProcessor {
        let mut outputs = LagProcessorOutputs::default();
        outputs.set_all_channels(1);
        LagProcessor {
            params: LagProcessorParams {
                input: PolySignal::mono(Signal::Volts(0.0)),
                rise: Some(PolySignal::mono(Signal::Volts(0.002))),
                fall: None,
            },
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
            channel_state: vec![SlewChannelState::default()].into_boxed_slice(),
        }
    }

    /// One step of the slew limiter toward `input`, rise 0.002 s/V and the
    /// default fall.
    fn reference_step(current: &mut Option<f32>, input: f32) -> f32 {
        let value = current.get_or_insert(sanitize(input));
        let max_rise = 1.0 / (0.002f32.max(0.001) * SR);
        let max_fall = 1.0 / (0.01f32.max(0.001) * SR);
        let diff = input - *value;
        let change = if diff > 0.0 {
            diff.min(max_rise)
        } else {
            diff.max(-max_fall)
        };
        *value = sanitize(*value + change);
        *value
    }

    #[test]
    fn held_inputs_match_the_stepped_limiter_bit_for_bit() {
        let held = [
            (-0.0, 10),
            (5.0, 3000),
            (0.0, 3000),
            (-0.0, 10),
            (2.5, 2000),
        ];
        let mut module = make();
        let mut reference = None;
        for (input, samples) in held {
            module.params.input = PolySignal::mono(Signal::Volts(input));
            for _ in 0..samples {
                module.update(SR);
                let want = reference_step(&mut reference, input);
                assert_eq!(module.outputs.sample.get(0).to_bits(), want.to_bits());
            }
        }
    }
}
