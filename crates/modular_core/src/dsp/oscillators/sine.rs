use crate::{
    dsp::{
        consts::{LUT_SINE, LUT_SINE_SIZE},
        oscillators::{FmMode, apply_fm},
        utils::interpolate,
    },
    poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt},
};
use deserr::Deserr;
use schemars::JsonSchema;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct SineOscillatorParams {
    /// pitch in V/Oct (0V = C4)
    #[signal(type = pitch)]
    freq: PolySignal,
    /// FM input signal (pre-scaled by user)
    #[deserr(default)]
    fm: Option<PolySignal>,
    /// FM mode: throughZero (default), lin, or exp
    #[serde(default)]
    #[deserr(default)]
    fm_mode: FmMode,
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

            let pitch = self.params.freq.get_value(ch);
            let fm = self.params.fm.value_or(ch, 0.0);
            let frequency = apply_fm(pitch, fm, self.params.fm_mode) / sample_rate;
            state.phase += frequency;
            // Wrap phase to [0, 1) — supports negative increments (through-zero FM)
            state.phase = state.phase.rem_euclid(1.0);
            let sine = interpolate(LUT_SINE, state.phase, LUT_SINE_SIZE);
            self.outputs.sample.set(ch, sine * 5.0);
        }
    }
}

message_handlers!(impl SineOscillator {});
