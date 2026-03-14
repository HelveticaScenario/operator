use crate::{
    dsp::{
        consts::{LUT_SINE, LUT_SINE_SIZE},
        utils::{interpolate, wrap},
    },
    poly::{PolyOutput, PolySignal, PolySignalExt},
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct PSineOscillatorParams {
    /// phasor input (0–1, wraps at boundaries)
    #[signal(range = (0.0, 1.0))]
    phase: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PSineOscillatorOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Phase-driven sine wave oscillator.
///
/// Instead of a frequency input, this oscillator is driven by an external
/// phasor signal (0–1). Connect a `ramp` or other phase source to `phase`
/// and use phase-distortion modules between them for complex timbres.
///
/// Output range is **±5V**.
#[module(name = "$pSine", args(phase))]
pub struct PSineOscillator {
    outputs: PSineOscillatorOutputs,
    params: PSineOscillatorParams,
}

impl PSineOscillator {
    fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let phase = wrap(0.0..1.0, self.params.phase.get_value(ch));
            let sine = interpolate(LUT_SINE, phase, LUT_SINE_SIZE);
            self.outputs.sample.set(ch, sine * 5.0);
        }
    }
}

message_handlers!(impl PSineOscillator {});
