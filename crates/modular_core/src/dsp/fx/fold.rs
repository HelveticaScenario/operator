//! Wavefolding effect module.
//!
//! Adapted from the 4ms Ensemble Oscillator warp mode.
//! Copyright 4ms Company. Used under GPL v3.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::dsp::fx::enosc_tables::{aa_fold, lookup_fold};
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS};
use crate::types::Clickless;

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct FoldParams {
    /// input signal to fold (bipolar, typically -5 to 5)
    input: PolySignal,
    /// fold amount (0-5, where 0 = bypass, 5 = maximum folding)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    amount: Option<PolySignal>,
    /// pitch of the source signal in V/Oct (optional, reduces aliasing at high frequencies)
    #[signal(type = pitch)]
    freq: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct FoldOutputs {
    #[output("output", "folded signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    amount: Clickless,
}

/// State for the Fold module.
#[derive(Default)]
struct FoldState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// Wavefolder that reflects the signal back when it exceeds a threshold,
/// producing dense, harmonically rich tones. Higher amounts create more
/// complex, metallic timbres.
#[module(name = "$fold", args(input, amount))]
pub struct Fold {
    outputs: FoldOutputs,
    state: FoldState,
    params: FoldParams,
}

impl Fold {
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

            // Normalize amount from [0, 5] to [0, 1] for table lookup
            let amount_norm = (amount / 5.0).clamp(0.0, 1.0);

            // Apply anti-aliasing when freq is connected
            let amount_norm = if freq_connected {
                let freq_hz = voct_to_hz(self.params.freq.value_or_zero(ch));
                aa_fold(freq_hz / sample_rate, amount_norm)
            } else {
                amount_norm
            };

            // Normalize input from typical [-5, 5] range to [-1, 1]
            let input_norm = (input / 5.0).clamp(-1.0, 1.0);

            // Apply wavefold
            let folded = lookup_fold(input_norm, amount_norm);

            // Scale back to output range
            self.outputs.sample.set(ch, folded * 5.0);
        }
    }
}

message_handlers!(impl Fold {});
