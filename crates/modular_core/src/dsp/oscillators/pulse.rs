use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    PORT_MAX_CHANNELS,
    dsp::oscillators::{FmMode, apply_fm},
    poly::{PolyOutput, PolySignal, PolySignalExt},
    types::Clickless,
};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
#[deserr(deny_unknown_fields)]
struct PulseOscillatorParams {
    /// pitch in V/Oct (0V = C4)
    #[signal(type = pitch)]
    freq: PolySignal,
    /// pulse width (0-5, 2.5 is square)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    #[deserr(default)]
    width: Option<PolySignal>,
    /// pulse width modulation CV — added to the width parameter
    #[deserr(default)]
    pwm: Option<PolySignal>,
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
struct PulseOscillatorOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct PulseChannelState {
    phase: f32,
    width: Clickless,
}

/// State for the PulseOscillator module.
#[derive(Default)]
struct PulseOscillatorState {
    channels: [PulseChannelState; PORT_MAX_CHANNELS],
}

/// Pulse/square wave oscillator with pulse width modulation.
///
/// The `freq` input follows the **V/Oct** standard (0V = C4).
/// The `width` parameter sets the duty cycle: 0 = narrow pulse,
/// 2.5 = square wave, 5 = inverted narrow pulse.
/// `pwm` is added to `width` for modulation.
///
/// Output range is **±5V**.
///
/// ## Example
///
/// ```js
/// $pulse('c3', { width: 2.5 }).out()
/// ```
#[module(name = "$pulse", args(freq))]
pub struct PulseOscillator {
    outputs: PulseOscillatorOutputs,
    state: PulseOscillatorState,
    params: PulseOscillatorParams,
}

impl PulseOscillator {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            let base_width = self.params.width.value_or(ch, 2.5);
            let pwm = self.params.pwm.value_or(ch, 0.0);
            state.width.update((base_width + pwm).clamp(0.0, 5.0));

            let pitch = self.params.freq.get_value(ch);
            let fm = self.params.fm.value_or(ch, 0.0);
            let frequency = apply_fm(pitch, fm, self.params.fm_mode);
            let phase_increment = frequency / sample_rate;

            // Pulse width (0.0 to 1.0, 0.5 is square wave)
            let pulse_width = (*state.width / 5.0).clamp(0.01, 0.99);

            state.phase += phase_increment;

            // Wrap phase (rem_euclid supports negative increments from through-zero FM)
            state.phase = state.phase.rem_euclid(1.0);

            // Naive pulse wave
            let mut naive_pulse = if state.phase < pulse_width { 1.0 } else { -1.0 };

            // Apply PolyBLEP at the rising edge (phase = 0)
            let abs_phase_inc = phase_increment.abs();
            naive_pulse += poly_blep_pulse(state.phase, abs_phase_inc);

            // Apply PolyBLEP at the falling edge (phase = pulse_width)
            naive_pulse -= poly_blep_pulse(
                if state.phase >= pulse_width {
                    state.phase - pulse_width
                } else {
                    state.phase - pulse_width + 1.0
                },
                abs_phase_inc,
            );

            self.outputs.sample.set(ch, naive_pulse * 5.0);
        }
    }
}

// PolyBLEP for pulse wave
fn poly_blep_pulse(phase: f32, phase_increment: f32) -> f32 {
    // Detect discontinuity at phase wrap (0.0)
    if phase < phase_increment {
        let t = phase / phase_increment;
        return t + t - t * t - 1.0;
    }
    // Detect discontinuity approaching 1.0
    else if phase > 1.0 - phase_increment {
        let t = (phase - 1.0) / phase_increment;
        return t * t + t + t + 1.0;
    }
    0.0
}

message_handlers!(impl PulseOscillator {});
