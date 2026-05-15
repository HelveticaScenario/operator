//! Pulsar synthesis phase-distortion module.
//!
//! Adapted from the 4ms Ensemble Oscillator twist mode.
//! Copyright 4ms Company. Used under GPL v3.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::fx::enosc_tables::aa_pulsar;
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PORT_MAX_CHANNELS, PolyOutput, PolySignal, PolySignalExt};
use crate::types::Clickless;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct PulsarParams {
    /// input phase (0 to 1)
    #[signal(range = (0.0, 1.0))]
    input: PolySignal,
    /// compression amount (0-5, where 0 = no compression, 5 = maximum compression)
    #[signal(range = (0.0, 5.0))]
    #[deserr(default)]
    amount: Option<PolySignal>,
    /// pitch in V/Oct (optional, reduces aliasing at high frequencies)
    #[signal(type = pitch)]
    #[deserr(default)]
    freq: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PulsarOutputs {
    #[output("output", "pulsar phase output", default, range = (0.0, 1.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    amount: Clickless,
}

/// State for the Pulsar module.
#[derive(Default)]
struct PulsarState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// Phase effect: pulsar synthesis distortion.
///
/// Transforms a 0–1 phase signal by compressing the active portion of each
/// cycle into a narrower window, leaving the rest silent. Feed the output
/// into a phase oscillator (`$pSine`, `$pSaw`, `$pPulse`) to hear pulsed
/// waveforms — at higher amounts the pulse becomes extremely narrow,
/// producing bright, impulse-like timbres useful for excitation signals
/// and metallic tones.
///
/// # Example
///
/// ```js
/// // Compress the phase with pulsar and convert to audio
/// $pSine($pulsar($ramp('c3'), 3)).out()
/// ```
#[module(name = "$pulsar", args(input, amount))]
pub struct Pulsar {
    outputs: PulsarOutputs,
    state: PulsarState,
    params: PulsarParams,
}

impl Pulsar {
    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let freq_connected = self.params.freq.is_some();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            let input = self.params.input.get_value(ch);
            let amount_raw = self.params.amount.value_or(ch, 0.0);

            // Smooth amount parameter to avoid clicks
            state.amount.update(amount_raw);
            let amount = *state.amount;

            // Map amount from [0, 5] to multiplier [1, 64]
            // Reference scaling curve: 2^(x^2 * 6)
            // Exponential of quadratic — stays near 1 for most of the range,
            // then rapidly increases. Range: 1..64
            let amount_norm = (amount / 5.0).clamp(0.0, 1.0);
            let curved = amount_norm * amount_norm;
            let multiplier = f32::exp2(curved * 6.0);

            // Apply anti-aliasing when freq is connected
            let multiplier = if freq_connected {
                let freq_hz = voct_to_hz(self.params.freq.value_or_zero(ch));
                aa_pulsar(freq_hz / sample_rate, multiplier)
            } else {
                multiplier
            };

            // Input is already a phase [0, 1]
            let phase = input;

            // Multiply phase and saturate to [0, 1]
            // Reference: u0_32::wrap((u0_16::narrow(phase) * p).min(1))
            let compressed_phase = (phase * multiplier).min(1.0);

            // Output the distorted phase
            self.outputs.sample.set(ch, compressed_phase);
        }
    }
}

message_handlers!(impl Pulsar {});
