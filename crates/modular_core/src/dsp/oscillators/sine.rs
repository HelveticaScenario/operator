use crate::{
    dsp::{
        consts::{LUT_SINE, LUT_SINE_SIZE},
        utils::{interpolate, voct_to_hz},
    },
    poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS},
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct SineOscillatorParams {
    /// pitch in V/Oct (0V = C4)
    #[signal(type = pitch)]
    freq: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SineOscillatorOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Per-channel oscillator state
#[derive(Default, Clone, Copy)]
struct ChannelState {
    phase: f32,
}

/// State for the SineOscillator module.
#[derive(Default)]
struct SineOscillatorState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// A sine wave oscillator.
///
/// ## Example
///
/// ```js
/// $sine('c4').out()
/// ```
#[module(name = "$sine", args(freq))]
pub struct SineOscillator {
    outputs: SineOscillatorOutputs,
    state: SineOscillatorState,
    params: SineOscillatorParams,
}

impl SineOscillator {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            let frequency = voct_to_hz(self.params.freq.value_or(ch, 0.0)) / sample_rate;
            state.phase += frequency;
            while state.phase >= 1.0 {
                state.phase -= 1.0;
            }
            let sine = interpolate(LUT_SINE, state.phase, LUT_SINE_SIZE);
            self.outputs.sample.set(ch, sine * 5.0);
        }
    }
}

message_handlers!(impl SineOscillator {});
