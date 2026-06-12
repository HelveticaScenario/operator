use crate::dsp::utils::{SchmittState, SchmittTrigger};
use crate::poly::{PolyOutput, PolySignal};
use deserr::Deserr;
use schemars::JsonSchema;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct SampleAndHoldParams {
    /// signal to sample
    input: PolySignal,
    /// rising edge captures the current input value
    #[signal(type = trig, range = (0.0, 5.0))]
    trigger: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SampleAndHoldOutputs {
    #[output("output", "held voltage", default)]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct SampleAndHoldChannelState {
    trigger: SchmittTrigger,
    held_value: f32,
}

/// Captures and holds a voltage on each trigger.
///
/// When **trigger** receives a rising edge, the current value of **input**
/// is sampled and held at the output until the next trigger. Classic
/// use: sample random noise to generate stepped random melodies.
///
/// ```js
/// // stepped random melody
/// $sine(
///  $quantizer(
///    $sah($noise('white').range(0, 1), $pulse('2hz')),
///    0,
///    'c(maj)',
///  ),
/// )
/// ```
#[module(name = "$sah", args(input, trigger))]
pub struct SampleAndHold {
    outputs: SampleAndHoldOutputs,
    params: SampleAndHoldParams,
    channel_state: Box<[SampleAndHoldChannelState]>,
}

impl SampleAndHold {
    pub fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];
            let input = self.params.input.get_value(ch);
            let trigger = self.params.trigger.get_value(ch);

            if state.trigger.state == SchmittState::Uninitialized {
                state.held_value = input;
            }

            // Make sure to initialize the held value on the first update
            if state.trigger.process(trigger) {
                state.held_value = input;
            }

            self.outputs.sample.set(ch, state.held_value);
        }
    }
}

message_handlers!(impl SampleAndHold {});

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct TrackAndHoldParams {
    /// signal to track
    input: PolySignal,
    /// while gate is low the output follows the input; when gate goes high the last value is held
    #[signal(type = gate, range = (0.0, 5.0))]
    gate: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct TrackAndHoldOutputs {
    #[output("output", "tracked or held voltage", default)]
    sample: PolyOutput,
}

#[derive(Default)]
struct TrackAndHoldChannelState {
    gate: SchmittTrigger,
}

/// Follows the input while the gate is low, and holds the value when the
/// gate goes high.
///
/// The complement of Sample and Hold: the output continuously tracks
/// **input** until **gate** rises, then freezes until the gate falls again.
///
/// ```js
/// // hold a slow sine value while the gate is high
/// $tah($sine('2hz'), gate)
/// ```
#[module(name = "$tah", args(input, gate))]
pub struct TrackAndHold {
    outputs: TrackAndHoldOutputs,
    params: TrackAndHoldParams,
    channel_state: Box<[TrackAndHoldChannelState]>,
}

impl TrackAndHold {
    pub fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];
            let input = self.params.input.get_value(ch);
            let gate = self.params.gate.get_value(ch);

            // Track while gate is low or on rising edge
            state.gate.process(gate);
            if state.gate.state() != crate::dsp::utils::SchmittState::High {
                self.outputs.sample.set(ch, input);
            }
        }
    }
}

message_handlers!(impl TrackAndHold {});
