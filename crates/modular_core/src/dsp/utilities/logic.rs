use crate::{
    PORT_MAX_CHANNELS,
    dsp::utils::{TempGate, TempGateState, min_gate_samples},
    poly::{PolyOutput, PolySignal},
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct RisingEdgeDetectorParams {
    /// signal to detect rising edges in
    input: PolySignal,
}

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct FallingEdgeDetectorParams {
    /// signal to detect falling edges in
    input: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct EdgeDetectorOutputs {
    #[output(
        "output",
        "edge detection pulse (5V when edge detected, 0V otherwise)",
        default
    )]
    output: PolyOutput,
}

#[derive(Clone, Copy)]
struct EdgeChannelState {
    last_input: f32,
    trigger_gate: TempGate,
}

impl Default for EdgeChannelState {
    fn default() -> Self {
        Self {
            last_input: 0.0,
            trigger_gate: TempGate::new_gate(TempGateState::Low),
        }
    }
}

/// State for the RisingEdgeDetector module.
#[derive(Default)]
struct RisingEdgeDetectorState {
    channels: [EdgeChannelState; PORT_MAX_CHANNELS],
}

/// Detects rising edges in a signal and emits a short pulse.
///
/// Outputs 5 V for a single sample whenever the input increases.
/// Useful for converting ramps, envelopes, or continuous signals into
/// gate/trigger events.
///
/// ```js
/// // trigger a percussion envelope on every rising edge of a slow oscillator
/// $perc($rising($sine('4hz')))
/// ```
#[module(name = "$rising", args(input))]
pub struct RisingEdgeDetector {
    outputs: EdgeDetectorOutputs,
    params: RisingEdgeDetectorParams,
    state: RisingEdgeDetectorState,
}

impl RisingEdgeDetector {
    pub fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let hold = min_gate_samples(sample_rate);

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];
            let input = self.params.input.get_value(ch);

            if input > state.last_input {
                state
                    .trigger_gate
                    .set_state(TempGateState::High, TempGateState::Low, hold);
            }

            state.last_input = input;
            self.outputs.output.set(ch, state.trigger_gate.process());
        }
    }
}

message_handlers!(impl RisingEdgeDetector {});

/// State for the FallingEdgeDetector module.
#[derive(Default)]
struct FallingEdgeDetectorState {
    channels: [EdgeChannelState; PORT_MAX_CHANNELS],
}

/// Detects falling edges in a signal and emits a short pulse.
///
/// Outputs 5 V for a single sample whenever the input decreases.
/// Useful for triggering events on the "off" transition of a gate or
/// on the downward slope of an LFO.
///
/// ```js
/// // trigger on every falling edge of a gate
/// $perc($falling(gate))
/// ```
#[module(name = "$falling", args(input))]
pub struct FallingEdgeDetector {
    outputs: EdgeDetectorOutputs,
    params: FallingEdgeDetectorParams,
    state: FallingEdgeDetectorState,
}

impl FallingEdgeDetector {
    pub fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let hold = min_gate_samples(sample_rate);

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];
            let input = self.params.input.get_value(ch);

            if input < state.last_input {
                state
                    .trigger_gate
                    .set_state(TempGateState::High, TempGateState::Low, hold);
            }

            state.last_input = input;
            self.outputs.output.set(ch, state.trigger_gate.process());
        }
    }
}

message_handlers!(impl FallingEdgeDetector {});
