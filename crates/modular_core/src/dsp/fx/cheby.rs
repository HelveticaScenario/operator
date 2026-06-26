//! Chebyshev polynomial waveshaping effect module.
//!
//! Adapted from the 4ms Ensemble Oscillator warp mode.
//! Copyright 4ms Company. Used under GPL v3.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::fx::enosc_tables::{aa_cheby, interpolate_cheby};
use crate::dsp::utils::dc_blocker::{DEFAULT_DC_BLOCK_FC_HZ, DcBlocker};
use crate::dsp::utils::fade::{DEFAULT_FADE_IN_SECS, FadeIn};
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};
use crate::types::Clickless;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct ChebyParams {
    /// input signal to shape (bipolar, typically -5 to 5)
    input: PolySignal,
    /// harmonic richness (0–5). At 0 the signal is clean; at 5 the highest harmonic content dominates
    #[signal(range = (0.0, 5.0))]
    amount: PolySignal,
    /// pitch of the source signal in V/Oct (optional, reduces aliasing at high frequencies)
    #[signal(type = pitch)]
    #[deserr(default)]
    freq: Option<PolySignal>,
    /// when true (default), a one-pole high-pass (~20 Hz) on the output removes
    /// the DC offset that the even-order harmonics introduce, keeping the
    /// signal centred
    #[serde(rename = "blockDC", default = "default_true")]
    #[deserr(rename = "blockDC", default = default_true())]
    block_dc: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ChebyOutputs {
    #[output("output", "waveshaped signal output", default, range = (-5.0, 5.0))]
    sample: PolyOutput,
}

#[derive(Default, Clone, Copy)]
struct ChannelState {
    amount: Clickless,
    /// DC blocker (one-pole high-pass) on the output — the even-order harmonics
    /// introduce a DC offset this removes.
    dc_blocker: DcBlocker,
    /// Onset fade-in — suppresses the click when the module is first added
    /// (`amount` snaps to its full value on the first sample, so the output
    /// would otherwise jump from silence).
    fade: FadeIn,
}

/// Module-level state holding the sample-rate-derived constants.
#[derive(Default)]
struct ChebyState {
    dc_block_coeff: f32,
    /// Per-sample increment for the onset fade-in.
    fade_inc: f32,
}

/// Harmonic waveshaping effect that adds controlled overtone content.
///
/// At low amounts the signal passes through cleanly; turning it up
/// progressively emphasizes higher harmonics (2nd, 3rd, … up to 16th),
/// thickening and brightening the tone.
#[module(name = "$cheby", args(input, amount), has_init)]
pub struct Cheby {
    outputs: ChebyOutputs,
    state: ChebyState,
    channel_state: Box<[ChannelState]>,
    params: ChebyParams,
}

impl Cheby {
    /// Compute the sample-rate-dependent constants once at construction.
    /// Invoked by the `#[module]` proc macro on the main thread. Both values are
    /// sample-rate-only, so they stay correct even though `transfer_state_from`
    /// swaps the whole `state` struct on a patch update.
    fn init(&mut self, sample_rate: f32) {
        self.state.dc_block_coeff = DcBlocker::coeff(DEFAULT_DC_BLOCK_FC_HZ, sample_rate);
        self.state.fade_inc = FadeIn::increment(DEFAULT_FADE_IN_SECS, sample_rate);
    }

    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let freq_connected = !self.params.freq.is_disconnected();
        let block_dc = self.params.block_dc;
        let dc_coeff = self.state.dc_block_coeff;
        let fade_inc = self.state.fade_inc;

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];

            let input = self.params.input.get_value(ch);
            let amount_raw = self.params.amount.get_value(ch);

            // Smooth amount parameter to avoid clicks
            state.amount.update(amount_raw);
            let amount = *state.amount;

            // Normalize amount from [0, 5] to [0, 1] for table lookup
            let amount_norm = (amount / 5.0).clamp(0.0, 1.0);

            // Apply anti-aliasing when freq is connected
            let amount_norm = if freq_connected {
                let freq_hz = voct_to_hz(self.params.freq.value_or_zero(ch));
                aa_cheby(freq_hz / sample_rate, amount_norm)
            } else {
                amount_norm
            };

            // Normalize input from typical [-5, 5] range to [-1, 1]
            let input_norm = (input / 5.0).clamp(-1.0, 1.0);

            // Apply Chebyshev waveshaping
            let shaped = interpolate_cheby(input_norm, amount_norm);

            // DC blocker on the output — the even-order harmonics introduce a
            // DC offset that a one-pole high-pass (~20 Hz) removes. Bypassed
            // when `block_dc` is false.
            let out = if block_dc {
                state.dc_blocker.process(shaped, dc_coeff)
            } else {
                shaped
            };

            // Fade in over the first few ms to suppress the onset click when
            // the module is first added.
            let fade = state.fade.advance(fade_inc);

            // Scale back to output range
            self.outputs.sample.set(ch, out * 5.0 * fade);
        }
    }
}

message_handlers!(impl Cheby {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    fn make(input: f32, amount: f32, block_dc: bool) -> Cheby {
        let mut outputs = ChebyOutputs::default();
        outputs.set_all_channels(1);
        let mut cheby = Cheby {
            params: ChebyParams {
                input: PolySignal::mono(Signal::Volts(input)),
                amount: PolySignal::mono(Signal::Volts(amount)),
                freq: None,
                block_dc,
            },
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
            state: ChebyState::default(),
            channel_state: vec![ChannelState::default(); 1].into_boxed_slice(),
        };
        cheby.init(48000.0);
        cheby
    }

    #[test]
    fn dc_blocker_removes_constant_offset() {
        // A constant (DC) input produces a constant shaped output; with the
        // blocker enabled it must decay toward zero.
        let mut cheby = make(3.0, 5.0, true);
        let mut last = 0.0;
        for _ in 0..8000 {
            cheby.update(48000.0);
            last = cheby.outputs.sample.get(0);
        }
        assert!(
            last.abs() < 0.05,
            "expected near 0 V after DC blocker, got {last}"
        );
    }

    #[test]
    fn block_dc_false_does_not_filter() {
        // With blocking disabled the output is a pure waveshaping of the
        // constant input: it holds steady at a non-zero DC level instead of
        // decaying, proving the parameter gates the filter.
        let mut cheby = make(3.0, 5.0, false);
        for _ in 0..8000 {
            cheby.update(48000.0);
        }
        let settled = cheby.outputs.sample.get(0);
        for _ in 0..2000 {
            cheby.update(48000.0);
        }
        let later = cheby.outputs.sample.get(0);
        assert!(
            settled.abs() > 1e-3,
            "shaper should produce a non-zero DC level for this input, got {settled}"
        );
        assert!(
            (settled - later).abs() < 1e-4,
            "bypassed output should hold steady, got {settled} then {later}"
        );
    }

    #[test]
    fn fades_in_on_fresh_add() {
        // A freshly added module with a non-zero amount must start near silence
        // and ramp up, rather than jumping straight to full level (the click the
        // user reported). `block_dc` off so a constant input keeps a steady
        // settled level to compare against.
        let mut cheby = make(3.0, 1.28, false);
        cheby.update(48000.0);
        let first = cheby.outputs.sample.get(0).abs();
        for _ in 0..8000 {
            cheby.update(48000.0);
        }
        let settled = cheby.outputs.sample.get(0).abs();
        assert!(
            settled > 1e-3,
            "settled output should be non-zero, got {settled}"
        );
        assert!(
            first < settled * 0.1,
            "onset should be faded in (first {first} should be well below settled {settled})"
        );
    }

    #[test]
    fn carried_over_channel_does_not_refade() {
        // A channel whose state was carried over by `transfer_state_from`
        // arrives with `fade` already at 1.0, so it must play at full level
        // immediately instead of re-fading on a patch edit.
        let mut cheby = make(3.0, 1.28, false);
        cheby.channel_state[0].fade.gain = 1.0;
        cheby.update(48000.0);
        let first = cheby.outputs.sample.get(0).abs();
        for _ in 0..8000 {
            cheby.update(48000.0);
        }
        let settled = cheby.outputs.sample.get(0).abs();
        assert!(
            settled > 1e-3,
            "settled output should be non-zero, got {settled}"
        );
        assert!(
            (first - settled).abs() < 1e-4,
            "carried-over channel should not re-fade (first {first} vs settled {settled})"
        );
    }
}
