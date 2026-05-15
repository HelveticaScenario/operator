//! Phase ramp generator module.
//!
//! Produces a phase ramp from 0 to 1 at a given frequency.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::voct_to_hz;
use crate::poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct RampParams {
    /// pitch in V/Oct (0V = C4)
    #[signal(type = pitch)]
    freq: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RampOutputs {
    #[output("output", "phase ramp output (0 to 1)", default, range = (0.0, 1.0))]
    sample: PolyOutput,
}

/// Per-channel phasor state
#[derive(Default, Clone, Copy)]
struct ChannelState {
    phase: f32,
}

/// State for the Ramp module.
#[derive(Default)]
struct RampState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// Phase ramp generator.
///
/// Produces a rising sawtooth phase signal from 0 to 1 at the given frequency.
/// This is the fundamental building block for phase-based synthesis:
/// feed its output into phase-distortion modules (crush, feedback, pulsar)
/// and then into a waveshaper (e.g. `$pSine`) to produce audio.
#[module(name = "$ramp", args(freq))]
pub struct Ramp {
    outputs: RampOutputs,
    state: RampState,
    params: RampParams,
}

impl Ramp {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let inv_sample_rate = 1.0 / sample_rate;

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            let frequency = voct_to_hz(self.params.freq.get_value(ch));
            let phase_increment = frequency * inv_sample_rate;

            state.phase += phase_increment;

            // Wrap phase to [0, 1)
            if state.phase >= 1.0 {
                state.phase -= 1.0;
            }
            if state.phase < 0.0 {
                state.phase += 1.0;
            }

            self.outputs.sample.set(ch, state.phase);
        }
    }
}

message_handlers!(impl Ramp {});
