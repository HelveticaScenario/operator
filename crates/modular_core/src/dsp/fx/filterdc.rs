//! DC-blocking high-pass filter module.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::dc_blocker::{DEFAULT_DC_BLOCK_FC_HZ, DcBlocker};
use crate::poly::{PolyOutput, PolySignal};

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct FilterDcParams {
    /// input signal to filter (any range)
    input: PolySignal,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct FilterDcOutputs {
    #[output("output", "DC-blocked output signal", default)]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    /// One-pole high-pass state — `y[n] = x[n] - x[n-1] + R · y[n-1]`.
    dc_blocker: DcBlocker,
}

/// Module-level state holding the sample-rate-derived constant.
#[derive(Default)]
struct FilterDcState {
    dc_block_coeff: f32,
}

/// One-pole high-pass that removes DC offset and sub-audio rumble from a
/// signal while passing audio-rate content. The cutoff sits at ~20 Hz.
#[module(name = "$filterDC", args(input), has_init)]
pub struct FilterDc {
    outputs: FilterDcOutputs,
    state: FilterDcState,
    channel_state: Box<[ChannelState]>,
    params: FilterDcParams,
}

impl FilterDc {
    /// Compute the sample-rate-dependent filter coefficient once at construction.
    /// Invoked by the `#[module]` proc macro on the main thread. Sample-rate-only,
    /// so it stays correct even though `transfer_state_from` swaps the whole
    /// `state` struct on a patch update.
    fn init(&mut self, sample_rate: f32) {
        self.state.dc_block_coeff = DcBlocker::coeff(DEFAULT_DC_BLOCK_FC_HZ, sample_rate);
    }

    fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();
        let dc_coeff = self.state.dc_block_coeff;

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];
            let input = self.params.input.get_value(ch);
            let out = state.dc_blocker.process(input, dc_coeff);
            self.outputs.sample.set(ch, out);
        }
    }
}

message_handlers!(impl FilterDc {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    fn make(input: f32) -> FilterDc {
        let mut outputs = FilterDcOutputs::default();
        outputs.set_all_channels(1);
        let mut filter = FilterDc {
            params: FilterDcParams {
                input: PolySignal::mono(Signal::Volts(input)),
            },
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
            state: FilterDcState::default(),
            channel_state: vec![ChannelState::default(); 1].into_boxed_slice(),
        };
        filter.init(48000.0);
        filter
    }

    #[test]
    fn blocks_constant_offset() {
        let mut filter = make(1.0);
        let mut last = 0.0;
        for _ in 0..4000 {
            filter.update(48000.0);
            last = filter.outputs.sample.get(0);
        }
        assert!(last.abs() < 0.05, "DC should be blocked, got {last}");
    }

    #[test]
    fn passes_audio_rate_signal() {
        use std::f32::consts::PI;
        let mut filter = make(0.0);
        // ~480 Hz at 48 kHz, well above the 20 Hz corner.
        let freq_norm = 0.01_f32;
        let mut peak = 0.0_f32;
        for n in 0..4000 {
            let x = (2.0 * PI * freq_norm * n as f32).sin();
            filter.params.input = PolySignal::mono(Signal::Volts(x));
            filter.update(48000.0);
            if n >= 1000 {
                peak = peak.max(filter.outputs.sample.get(0).abs());
            }
        }
        assert!(peak > 0.5, "AC signal should pass, got peak {peak}");
    }
}
