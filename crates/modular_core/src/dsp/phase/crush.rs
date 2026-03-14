//! Bit-crushing / XOR phase-distortion module.
//!
//! Adapted from the 4ms Ensemble Oscillator twist mode.
//! Copyright 4ms Company. Used under GPL v3.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::poly::{PolyOutput, PolySignal, PolySignalExt, PORT_MAX_CHANNELS};
use crate::types::Clickless;

#[derive(Clone, Deserialize, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
struct CrushParams {
    /// input phase (0 to 1)
    #[signal(range = (0.0, 1.0))]
    input: PolySignal,
    /// crush amount (0-5, where 0 = clean, 5 = maximum distortion)
    #[signal(range = (0.0, 5.0))]
    amount: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CrushOutputs {
    #[output("output", "crushed phase output", default, range = (0.0, 1.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    amount: Clickless,
}

/// State for the Crush module.
#[derive(Default)]
struct CrushState {
    channels: [ChannelState; PORT_MAX_CHANNELS],
}

/// Phase effect: digital bit-crush distortion.
///
/// Transforms a 0–1 phase signal by quantizing it into coarse steps,
/// creating glitchy, staircase-like phase patterns. Feed the output into
/// a phase oscillator (`$pSine`, `$pSaw`, `$pPulse`) to hear the result
/// as gritty digital artifacts — fractured harmonics at low settings,
/// aggressive bit-reduction at high settings.
///
/// # Example
///
/// ```js
/// // Bit-crush a ramp phase and convert to audio with $pSine
/// $pSine($crush($ramp('c3'), 2)).out()
/// ```
#[module(name = "$crush", args(input, amount))]
pub struct Crush {
    outputs: CrushOutputs,
    state: CrushState,
    params: CrushParams,
}

impl Crush {
    fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.state.channels[ch];

            let input = self.params.input.get_value(ch);
            let amount_raw = self.params.amount.value_or(ch, 0.0);

            // Smooth amount parameter
            state.amount.update(amount_raw);
            let amount = *state.amount;

            // Normalize amount from [0, 5] to [0, 1]
            let amount_norm = (amount / 5.0).clamp(0.0, 1.0);

            // Apply reference scaling curve: 0.5 * x^2
            // Quadratic onset with cap at 0.5 for gentler XOR distortion
            let amount_norm = amount_norm * amount_norm * 0.5;
            // No AA for crush — aliasing is intentional

            // Input is already a phase [0, 1]
            let input_phase = input.clamp(0.0, 1.0);

            // Convert to 32-bit fixed point matching reference u0_32/u0_16 types
            let phase_u32 = (input_phase * 4_294_967_295.0) as u32; // u0_32
            let phase_u16 = (phase_u32 >> 16) as u16; // u0_16::narrow(phase)
            let am_u16 = (amount_norm * 65535.0) as u16; // u0_16(amount)

            // Reference: Distortion::twist<CRUSH>
            //   x ^= Bitfield<32>(u0_16::narrow(phase) * am)
            //   x ^= Bitfield<32>(0.25_u0_16 * am)
            let mut x = phase_u32;
            x ^= phase_u16 as u32 * am_u16 as u32;
            x ^= 16384u32 * am_u16 as u32; // 0.25 in u0_16 = 16384

            // Convert crushed phase back to [0, 1]
            let crushed_phase = x as f32 / 4_294_967_295.0;

            // Output the distorted phase
            self.outputs.sample.set(ch, crushed_phase);
        }
    }
}

message_handlers!(impl Crush {});
