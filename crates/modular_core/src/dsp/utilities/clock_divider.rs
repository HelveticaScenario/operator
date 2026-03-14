use napi::Result;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::dsp::utils::{min_gate_samples, SchmittTrigger, TempGate, TempGateState};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS};
use crate::types::ClockMessages;

fn default_division() -> u32 {
    1
}

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct ClockDividerParams {
    /// division factor (e.g. 2 = output fires every other tick)
    pub division: u32,
    /// clock signal to divide
    #[signal(type = trig, range = (0.0, 5.0))]
    pub input: PolySignal,
    /// trigger to reset the counter to 0
    #[signal(type = trig, range = (0.0, 5.0))]
    pub reset: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ClockDividerOutputs {
    #[output("output", "divided clock output", default)]
    pub output: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    counter: u32,
    input_schmitt: SchmittTrigger,
    reset_schmitt: SchmittTrigger,
    trigger_gate: TempGate,
}

/// State for the ClockDivider module.
#[derive(Default)]
struct ClockDividerState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// Divides an incoming clock signal so it fires less often.
///
/// Feed it a clock and set **division** to an integer — the output will
/// tick once every *n* input ticks. Useful for creating slower rhythmic
/// subdivisions from a master clock.
///
/// ```js
/// // Pulses every other bar of the root clock:
/// $clockDivider($clock.barTrigger, 2)
/// ```
#[module(name = "$clockDivider", args(input, division))]
pub struct ClockDivider {
    params: ClockDividerParams,
    outputs: ClockDividerOutputs,
    state: ClockDividerState,
}

message_handlers!(impl ClockDivider {
    Clock(m) => ClockDivider::on_clock_message,
});

impl ClockDivider {
    fn on_clock_message(&mut self, m: &ClockMessages) -> Result<()> {
        match m {
            ClockMessages::Start => {
                // Reset all channel counters on start
                for state in self.state.channels.iter_mut() {
                    state.counter = 0;
                }
            }
            ClockMessages::Stop => {
                // No special handling needed on stop
            }
        }
        Ok(())
    }

    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let division = self.params.division.max(1);
        let hold = min_gate_samples(sample_rate);

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            // Reset counter on rising edge of reset trigger
            if state
                .reset_schmitt
                .process(self.params.reset.value_or_zero(ch))
            {
                state.counter = 0;
            }

            if state.input_schmitt.process(self.params.input.get_value(ch)) {
                if state.counter == 0 {
                    state
                        .trigger_gate
                        .set_state(TempGateState::High, TempGateState::Low, hold);
                }

                state.counter += 1;
                if state.counter >= division {
                    state.counter = 0;
                }
            }

            self.outputs.output.set(ch, state.trigger_gate.process());
        }
    }
}
