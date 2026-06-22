use deserr::Deserr;
use schemars::JsonSchema;

use crate::{
    dsp::{
        oscillators::{FmMode, apply_fm, sync_blep, sync_edge_fraction},
        utils::SchmittTrigger,
    },
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
    /// hard sync source — rising edges reset the oscillator phase
    #[deserr(default)]
    sync: Option<PolySignal>,
    /// phase offset in [0, 1) added to the internal phase before sampling
    #[signal(default = 0.0, range = (0.0, 1.0))]
    #[deserr(default)]
    phase_offset: Option<PolySignal>,
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
    /// Edge detector for the sync input.
    sync_schmitt: SchmittTrigger,
    /// Previous sync-input sample, for subsample edge interpolation.
    sync_prev: f32,
    /// PolyBLEP residual carried into the next sample from a sync reset.
    blep_carry: f32,
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
    channel_state: Box<[PulseChannelState]>,
    params: PulseOscillatorParams,
}

impl PulseOscillator {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];

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

            // Phase offset shifts the read position without altering the
            // accumulator, so it never drifts.
            let offset = self.params.phase_offset.value_or(ch, 0.0);
            let read_offset = offset.rem_euclid(1.0);
            let read_phase = (state.phase + read_offset).rem_euclid(1.0);

            let naive_pulse = |p: f32| if p < pulse_width { 1.0 } else { -1.0 };

            // Naive pulse plus PolyBLEP at its own rising (phase 0) and falling
            // (phase = width) edges. The sync reset lands in the upcoming
            // interval, so these operate on the real, pre-reset phase.
            let abs_phase_inc = phase_increment.abs();
            let mut body = naive_pulse(read_phase);
            body += poly_blep_pulse(read_phase, abs_phase_inc);
            body -= poly_blep_pulse(
                if read_phase >= pulse_width {
                    read_phase - pulse_width
                } else {
                    read_phase - pulse_width + 1.0
                },
                abs_phase_inc,
            );

            let pending = state.blep_carry;
            state.blep_carry = 0.0;

            // Hard sync: a rising edge resets the phase, with a PolyBLEP placed
            // at the subsample crossing to band-limit the reset discontinuity.
            // (The jump is zero when the pulse was already high at the reset.)
            let mut now = 0.0;
            if let Some(sync) = &self.params.sync {
                let v = sync.get_value(ch);
                if state.sync_schmitt.process(v) {
                    let frac = sync_edge_fraction(state.sync_prev, v);
                    let before = naive_pulse(read_phase);
                    state.phase = 0.0;
                    let after = naive_pulse(read_offset);
                    let (n, carry) = sync_blep(after - before, frac);
                    now = n;
                    state.blep_carry = carry;
                }
                state.sync_prev = v;
            }

            self.outputs.sample.set(ch, (body + pending + now) * 5.0);
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
