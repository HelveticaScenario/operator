use crate::{
    PORT_MAX_CHANNELS,
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

/// State for the LagProcessor module.
#[derive(Default)]
struct LagProcessorState {
    channels: [SlewChannelState; PORT_MAX_CHANNELS],
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
/// $sine($slew(sequencer.pitch, { rise: 0.1, fall: 0.1 }))
/// ```
#[module(name = "$slew", args(input))]
pub struct LagProcessor {
    outputs: LagProcessorOutputs,
    params: LagProcessorParams,
    state: LagProcessorState,
}

impl LagProcessor {
    pub fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];
            let input = self.params.input.get_value(ch);
            if !state.initialized {
                state.current_value = sanitize(input);
                state.initialized = true;
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
