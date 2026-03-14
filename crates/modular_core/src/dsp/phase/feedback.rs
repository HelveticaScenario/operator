//! FM feedback phase-distortion module.
//!
//! Adapted from the 4ms Ensemble Oscillator twist mode.
//! Copyright 4ms Company. Used under GPL v3.

use std::f32::consts::PI;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::dsp::fx::enosc_tables::aa_feedback;
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS};
use crate::types::Clickless;

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct FeedbackParams {
    /// input phase (0 to 1)
    #[signal(range = (0.0, 1.0))]
    input: PolySignal,
    /// feedback amount (0-5, where 0 = no feedback, 5 = maximum feedback FM)
    #[signal(range = (0.0, 5.0))]
    amount: Option<PolySignal>,
    /// pitch in V/Oct (optional, reduces aliasing at high frequencies)
    #[signal(type = pitch)]
    freq: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct FeedbackOutputs {
    #[output("output", "feedback-distorted phase output", default, range = (0.0, 1.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    amount: Clickless,
    lp_state: f32, // One-pole LP filter state (matches IOnePoleLp<s1_15, 2>)
}

/// State for the Feedback module.
#[derive(Default)]
struct FeedbackState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// Phase effect: FM feedback distortion.
///
/// Transforms a 0–1 phase signal by feeding the output back into itself,
/// progressively adding harmonic complexity and chaotic motion. Feed the
/// result into a phase oscillator (`$pSine`, `$pSaw`, `$pPulse`) to hear
/// the effect. At low amounts the timbre gains subtle overtones; at high
/// amounts it becomes chaotic and noisy.
///
/// # Example
///
/// ```js
/// // Apply feedback distortion to a ramp phase and convert to audio
/// $pSine($feedback($ramp('c3'), 3)).out()
/// ```
#[module(name = "$feedback", args(input, amount))]
pub struct Feedback {
    outputs: FeedbackOutputs,
    state: FeedbackState,
    params: FeedbackParams,
}

impl Feedback {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let freq_connected = !self.params.freq.is_disconnected();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            let input = self.params.input.get_value(ch);
            let amount_raw = self.params.amount.value_or(ch, 0.0);

            // Smooth amount parameter to avoid clicks
            state.amount.update(amount_raw);
            let amount = *state.amount;

            // Normalize amount from [0, 5] to [0, 1]
            let amount_norm = (amount / 5.0).clamp(0.0, 1.0);

            // Apply reference scaling curve: 0.7 * x^2
            // Quadratic onset with cap at 0.7 to prevent runaway feedback
            let amount_norm = amount_norm * amount_norm * 0.7;

            // Apply anti-aliasing when freq is connected
            let amount_norm = if freq_connected {
                let freq_hz = voct_to_hz(self.params.freq.value_or_zero(ch));
                aa_feedback(freq_hz / sample_rate, amount_norm)
            } else {
                amount_norm
            };

            // Input is already a phase [0, 1]
            let input_phase = input;

            // Reference SineShaper::Process with feedback:
            //   fb = lp_state * feedback_signed
            //   phase += fb.to_unsigned() + u0_32(feedback)
            // fb.to_unsigned() maps signed [-1,1] to unsigned [0,1]
            // u0_32(feedback) adds the amount as a static DC phase offset
            let fb = state.lp_state * amount_norm;
            let fb_unsigned = (fb + 1.0) * 0.5; // Map signed [-1,1] to unsigned [0,1]
            let phase = input_phase + fb_unsigned + amount_norm;

            // Wrap phase to [0, 1]
            let phase = phase - phase.floor();

            // Compute sine internally for the LP feedback loop
            let sine = (phase * 2.0 * PI).sin();

            // Update one-pole LP filter: state += (input - state) / 4
            // Matches IOnePoleLp<s1_15, 2> where SHIFT=2 → coefficient = 1/4
            state.lp_state += (sine - state.lp_state) * 0.25;

            // Output the distorted phase
            self.outputs.sample.set(ch, phase);
        }
    }
}

message_handlers!(impl Feedback {});
