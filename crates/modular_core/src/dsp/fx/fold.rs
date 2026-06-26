//! Wavefolding effect module.
//!
//! Adapted from the 4ms Ensemble Oscillator warp mode.
//! Copyright 4ms Company. Used under GPL v3.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::fx::enosc_tables::{aa_fold, lookup_fold};
use crate::dsp::utils::fade::{DEFAULT_FADE_IN_SECS, FadeIn};
use crate::dsp::utils::voct_to_hz;
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};
use crate::types::Clickless;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct FoldParams {
    /// input signal to fold (bipolar, typically -5 to 5)
    input: PolySignal,
    /// fold amount (0-5, where 0 = bypass, 5 = maximum folding)
    #[signal(default = 0.0, range = (0.0, 5.0))]
    amount: PolySignal,
    /// pitch of the source signal in V/Oct (optional, reduces aliasing at high frequencies)
    #[signal(type = pitch)]
    #[deserr(default)]
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
    /// Onset fade-in — suppresses the click when the module is first added
    /// (`amount` snaps to its full value on the first sample, so the output
    /// would otherwise jump from silence).
    fade: FadeIn,
}

/// Module-level state holding the sample-rate-derived constants.
#[derive(Default)]
struct FoldState {
    /// Per-sample increment for the onset fade-in.
    fade_inc: f32,
}

/// Wavefolder that reflects the signal back when it exceeds a threshold,
/// producing dense, harmonically rich tones. Higher amounts create more
/// complex, metallic timbres.
#[module(name = "$fold", args(input, amount), has_init)]
pub struct Fold {
    outputs: FoldOutputs,
    state: FoldState,
    channel_state: Box<[ChannelState]>,
    params: FoldParams,
}

impl Fold {
    /// Compute the sample-rate-dependent fade increment once at construction.
    /// Invoked by the `#[module]` proc macro on the main thread. Sample-rate-only,
    /// so it stays correct even though `transfer_state_from` swaps the whole
    /// `state` struct on a patch update.
    fn init(&mut self, sample_rate: f32) {
        self.state.fade_inc = FadeIn::increment(DEFAULT_FADE_IN_SECS, sample_rate);
    }

    fn update(&mut self, sample_rate: f32) {
        let num_channels = self.channel_count();
        let freq_connected = !self.params.freq.is_disconnected();
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
                aa_fold(freq_hz / sample_rate, amount_norm)
            } else {
                amount_norm
            };

            // Normalize input from typical [-5, 5] range to [-1, 1]
            let input_norm = (input / 5.0).clamp(-1.0, 1.0);

            // Apply wavefold
            let folded = lookup_fold(input_norm, amount_norm);

            // Fade in over the first few ms to suppress the onset click when
            // the module is first added.
            let fade = state.fade.advance(fade_inc);

            // Scale back to output range
            self.outputs.sample.set(ch, folded * 5.0 * fade);
        }
    }
}

message_handlers!(impl Fold {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, Signal};

    fn make(input: f32, amount: f32) -> Fold {
        let mut outputs = FoldOutputs::default();
        outputs.set_all_channels(1);
        let mut fold = Fold {
            params: FoldParams {
                input: PolySignal::mono(Signal::Volts(input)),
                amount: PolySignal::mono(Signal::Volts(amount)),
                freq: None,
            },
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
            state: FoldState::default(),
            channel_state: vec![ChannelState::default(); 1].into_boxed_slice(),
        };
        fold.init(48000.0);
        fold
    }

    /// Largest single-sample output jump while `amount` is stepped from 0 to a
    /// low target that crosses the old passthrough threshold (param 0.025). A
    /// constant input means any jump is a transition artifact, not signal.
    fn max_step_through_threshold(input: f32, target_amount: f32) -> f32 {
        let mut fold = make(input, 0.0);
        // Settle the amount smoother at 0.
        for _ in 0..256 {
            fold.update(48000.0);
        }
        let mut prev = fold.outputs.sample.get(0);
        let mut max_step = 0.0_f32;
        // Step the param; the Clickless smoother drives the transition.
        fold.params.amount = PolySignal::mono(Signal::Volts(target_amount));
        for _ in 0..4000 {
            fold.update(48000.0);
            let cur = fold.outputs.sample.get(0);
            max_step = max_step.max((cur - prev).abs());
            prev = cur;
        }
        max_step
    }

    #[test]
    fn no_click_sweeping_through_low_amount() {
        // Regression: stepping `amount` to a low setting used to cross a hard
        // passthrough/fold discontinuity (~2.7 V jump) around param 0.025. The
        // crossfade in `lookup_fold` keeps the transition continuous. Worst-case
        // input is full-scale (input_norm = -1).
        for &input in &[-5.0f32, -2.5, 2.5, 5.0] {
            for &target in &[0.05f32, 0.1, 0.2] {
                let step = max_step_through_threshold(input, target);
                assert!(
                    step < 0.2,
                    "output jumped {step} V (input={input}, target_amount={target}) — \
                     expected a smooth transition"
                );
            }
        }
    }

    #[test]
    fn passthrough_at_zero_amount() {
        // amount = 0 is a clean bypass: output equals the input (once the onset
        // fade-in has completed).
        let mut fold = make(3.3, 0.0);
        for _ in 0..256 {
            fold.update(48000.0);
        }
        assert!((fold.outputs.sample.get(0) - 3.3).abs() < 1e-3);
    }

    #[test]
    fn fades_in_on_fresh_add() {
        // A freshly added module with a non-zero amount must start near silence
        // and ramp up, rather than jumping straight to full level (the onset
        // click). Constant input → a steady settled level to compare against.
        let mut fold = make(3.0, 1.28);
        fold.update(48000.0);
        let first = fold.outputs.sample.get(0).abs();
        for _ in 0..8000 {
            fold.update(48000.0);
        }
        let settled = fold.outputs.sample.get(0).abs();
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
        // arrives with the fade already complete, so it plays at full level
        // immediately instead of re-fading on a patch edit.
        let mut fold = make(3.0, 1.28);
        fold.channel_state[0].fade.gain = 1.0;
        fold.update(48000.0);
        let first = fold.outputs.sample.get(0).abs();
        for _ in 0..8000 {
            fold.update(48000.0);
        }
        let settled = fold.outputs.sample.get(0).abs();
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
