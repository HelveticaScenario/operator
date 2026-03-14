use crate::{
    dsp::utils::wrap,
    poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS},
    types::Clickless,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct PPulseOscillatorParams {
    /// phasor input (0–1, wraps at boundaries)
    #[signal(range = (0.0, 1.0))]
    phase: PolySignal,
    /// pulse width (0-5, 2.5 is square)
    #[signal(default = 2.5, range = (0.0, 5.0))]
    width: Option<PolySignal>,
    /// pulse width modulation CV — added to the width parameter
    pwm: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PPulseOscillatorOutputs {
    #[output("output", "signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

/// Per-channel state for width smoothing and phase tracking
#[derive(Default, Clone, Copy)]
struct ChannelState {
    width: Clickless,
    prev_phase: f32,
}

/// State for the PPulseOscillator module.
#[derive(Default)]
struct PPulseOscillatorState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// Phase-driven pulse/square oscillator with pulse width modulation.
///
/// Instead of a frequency input, this oscillator is driven by an external
/// phasor signal (0–1). Connect a `ramp` or other phase source to `phase`
/// and use phase-distortion modules between them for complex timbres.
///
/// The `width` parameter sets the duty cycle: 0 = narrow pulse,
/// 2.5 = square wave, 5 = inverted narrow pulse.
/// `pwm` is added to `width` for modulation.
///
/// Output range is **±5V**.
#[module(name = "$pPulse", args(phase))]
pub struct PPulseOscillator {
    outputs: PPulseOscillatorOutputs,
    state: PPulseOscillatorState,
    params: PPulseOscillatorParams,
}

impl PPulseOscillator {
    fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            let base_width = self.params.width.value_or(ch, 2.5);
            let pwm = self.params.pwm.value_or(ch, 0.0);
            state.width.update((base_width + pwm).clamp(0.0, 5.0));

            let phase = wrap(0.0..1.0, self.params.phase.get_value(ch));

            // Derive phase increment from external phasor
            let mut phase_increment = phase - state.prev_phase;
            if phase_increment < -0.5 {
                phase_increment += 1.0;
            } else if phase_increment > 0.5 {
                phase_increment -= 1.0;
            }
            phase_increment = phase_increment.abs().clamp(0.0, 0.5);

            state.prev_phase = phase;

            // Pulse width (0.0 to 1.0, 0.5 is square wave)
            let pulse_width = (*state.width / 5.0).clamp(0.01, 0.99);

            // Naive pulse wave
            let mut pulse = if phase < pulse_width { 1.0 } else { -1.0 };

            // Apply PolyBLEP at the rising edge (phase = 0)
            pulse += poly_blep(phase, phase_increment);

            // Apply PolyBLEP at the falling edge (phase = pulse_width)
            pulse -= poly_blep(
                if phase >= pulse_width {
                    phase - pulse_width
                } else {
                    phase - pulse_width + 1.0
                },
                phase_increment,
            );

            self.outputs.sample.set(ch, pulse * 5.0);
        }
    }
}

/// PolyBLEP (Polynomial Band-Limited Step) correction.
/// Smooths step discontinuities by replacing the hard edge with a
/// polynomial residual within one sample of the transition.
#[inline(always)]
fn poly_blep(phase: f32, phase_increment: f32) -> f32 {
    if phase_increment < 1.0e-7 {
        return 0.0;
    }
    if phase < phase_increment {
        let t = phase / phase_increment;
        return t + t - t * t - 1.0;
    } else if phase > 1.0 - phase_increment {
        let t = (phase - 1.0) / phase_increment;
        return t * t + t + t + 1.0;
    }
    0.0
}

message_handlers!(impl PPulseOscillator {});
