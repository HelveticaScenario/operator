//! Quantization-noise grit effect.
//!
//! Recreates the bright, granular digital fizz that a coarsely-quantized signal
//! produces when its quantization error is differentiated — the same character
//! the DPW saw oscillators emitted at low rates before their differencing was
//! widened to f64. The signal is snapped to a coarse voltage grid (with dither
//! so the result is broadband noise rather than a tonal staircase buzz), the
//! quantized value is differentiated to brighten it, and that grit is added back
//! onto the dry signal.

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::utils::rng::{LcgRng, seed_base};
use crate::poly::{PolyOutput, PolySignal, PolySignalExt};
use crate::types::Clickless;

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
struct QuantNoiseParams {
    /// input signal to grit up (bipolar, typically -5 to 5)
    input: PolySignal,
    /// grit amount — gain on the differentiated quantization residual that is
    /// added to the signal (0 = clean, higher = more fizz)
    #[signal(default = 1.0, range = (0.0, 5.0))]
    amount: PolySignal,
    /// quantization step in volts: the grid the signal is snapped to. Coarser
    /// (larger) steps give bigger, grainier grit.
    #[signal(default = 0.25, range = (0.0, 5.0))]
    #[deserr(default)]
    step: Option<PolySignal>,
    /// dither depth (0–1). At 0 the grit is a tonal staircase buzz that tracks
    /// the signal; at 1 the quantization is fully dithered into broadband noise.
    #[signal(default = 1.0, range = (0.0, 1.0))]
    #[deserr(default)]
    dither: Option<PolySignal>,
}

#[derive(Outputs, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct QuantNoiseOutputs {
    #[output("output", "input with quantization-noise grit added", default)]
    sample: PolyOutput,
}

/// Smallest allowed quantization step, in volts. Guards the divide and keeps the
/// grid from collapsing onto a single level (which would emit silence).
const MIN_STEP: f32 = 1.0e-4;

#[derive(Default, Clone, Copy)]
struct ChannelState {
    amount: Clickless,
    step: Clickless,
    /// Previous quantized value, for the differentiation that brightens the grit.
    prev_q: f32,
    /// False until the first sample has seeded `prev_q`. A fresh channel primes
    /// itself so the differentiation does not spike on the opening sample (which
    /// would click); a channel carried over by `transfer_state_from` arrives
    /// already primed and keeps its running `prev_q`.
    primed: bool,
    /// Per-channel dither source.
    rng: LcgRng,
}

/// Module-level state for the quantization-noise effect.
#[derive(Default)]
struct QuantNoiseState {
    /// Set once the per-channel dither RNGs have been seeded off the module's
    /// stable heap address (see `seed`).
    seeded: bool,
}

/// Adds bright, granular quantization noise to a signal.
///
/// The signal is snapped to a coarse voltage grid (`step`), dithered so the
/// result is broadband noise instead of a tonal buzz, then the quantized value
/// is differentiated and scaled by `amount` and summed back onto the dry signal.
/// Because the grit is the signal's own (dithered) quantization residual, it
/// tracks the signal's level and motion — feed it a slow LFO for a fizz that
/// swells with the modulator, or audio for a crunchy digital edge.
///
/// ## Example
///
/// ```js
/// $quantNoise($sine('c'), 2).out() // grainy, fizzy sine
/// ```
#[module(name = "$quantNoise", args(input, amount), patch_update)]
pub struct QuantNoise {
    outputs: QuantNoiseOutputs,
    state: QuantNoiseState,
    channel_state: Box<[ChannelState]>,
    params: QuantNoiseParams,
}

impl QuantNoise {
    /// Seed each channel's dither RNG off this module's stable heap address so
    /// every instance gets a distinct stream; mixing the channel index with an
    /// odd golden-ratio constant decorrelates the channels.
    ///
    /// Called from `on_patch_update`, not `init`: `init` runs while the module
    /// still lives in a transient stack slot that is reused across
    /// constructions, so every instance would capture the same address and seed
    /// identically. By `on_patch_update` the module sits at its stable
    /// per-instance heap address.
    ///
    /// Guarded by `seeded` (which, with the RNG state, rides
    /// `transfer_state_from`): `on_patch_update` runs after the state swap, so
    /// re-seeding unconditionally would clobber the stream carried over from the
    /// previous patch on every edit. Seeding once keeps the dither continuous.
    fn seed(&mut self) {
        let base = seed_base(self);
        for (ch, state) in self.channel_state.iter_mut().enumerate() {
            state.rng.seed(base, ch);
        }
        self.state.seeded = true;
    }

    fn update(&mut self, _sample_rate: f32) {
        let num_channels = self.channel_count();

        for ch in 0..num_channels {
            let state = &mut self.channel_state[ch];

            let input = self.params.input.get_value(ch);
            state.amount.update(self.params.amount.get_value(ch));
            state.step.update(self.params.step.value_or(ch, 0.25));
            let amount = (*state.amount).max(0.0);
            let step = (*state.step).max(MIN_STEP);
            let dither = self.params.dither.value_or(ch, 1.0).clamp(0.0, 1.0);

            // Dither up to ±one step so rounding can flip to an adjacent level
            // anywhere in the grid, decorrelating the quantization error into
            // broadband noise rather than a tonal staircase.
            let d = state.rng.next_bipolar() * step * dither;
            let q = ((input + d) / step).round() * step;

            // Prime on the first sample so the difference below starts from the
            // current level instead of zero (which would emit an onset click).
            if !state.primed {
                state.prev_q = q;
                state.primed = true;
            }

            // Differentiate the quantized value to brighten the grit (this is the
            // high-pass that gives the fizz its characteristic bright timbre),
            // then scale and add it back onto the dry signal.
            let grit = (q - state.prev_q) * amount;
            state.prev_q = q;

            self.outputs.sample.set(ch, input + grit);
        }
    }
}

impl crate::types::PatchUpdateHandler for QuantNoise {
    fn on_patch_update(&mut self) {
        if !self.state.seeded {
            self.seed();
        }
    }
}

message_handlers!(impl QuantNoise {});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutputStruct, PatchUpdateHandler, Signal};

    fn make(input: f32, amount: f32, step: f32, dither: f32) -> QuantNoise {
        let mut outputs = QuantNoiseOutputs::default();
        outputs.set_all_channels(1);
        let mut m = QuantNoise {
            params: QuantNoiseParams {
                input: PolySignal::mono(Signal::Volts(input)),
                amount: PolySignal::mono(Signal::Volts(amount)),
                step: Some(PolySignal::mono(Signal::Volts(step))),
                dither: Some(PolySignal::mono(Signal::Volts(dither))),
            },
            outputs,
            _channel_count: 1,
            _block_index: Default::default(),
            state: QuantNoiseState::default(),
            channel_state: vec![ChannelState::default(); 1].into_boxed_slice(),
        };
        m.on_patch_update();
        m
    }

    /// Run `n` samples, returning channel-0 output minus the (constant) dry input
    /// so the result is the grit alone.
    fn grit_series(m: &mut QuantNoise, input: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|_| {
                m.update(48000.0);
                m.outputs.sample.get(0) - input
            })
            .collect()
    }

    #[test]
    fn amount_zero_is_clean_passthrough() {
        let mut m = make(2.0, 0.0, 0.25, 1.0);
        for _ in 0..500 {
            m.update(48000.0);
            assert_eq!(
                m.outputs.sample.get(0),
                2.0,
                "amount 0 must pass dry signal"
            );
        }
    }

    #[test]
    fn dithered_constant_input_produces_broadband_grit() {
        // A constant input would sit on one quantization level (silent grit)
        // without dither; with dither it must produce a bright, bipolar,
        // non-trivial grit signal.
        let mut m = make(2.0, 1.0, 0.25, 1.0);
        let grit = grit_series(&mut m, 2.0, 4000);
        let rms = (grit.iter().map(|g| g * g).sum::<f32>() / grit.len() as f32).sqrt();
        assert!(rms > 1.0e-3, "dithered grit should be audible, rms={rms}");
        // Bright = high-pass: lag-1 autocorrelation should be negative.
        let mean = grit.iter().sum::<f32>() / grit.len() as f32;
        let (mut num, mut den) = (0.0f32, 0.0f32);
        for i in 1..grit.len() {
            num += (grit[i] - mean) * (grit[i - 1] - mean);
            den += (grit[i] - mean).powi(2);
        }
        assert!(
            num / den < 0.0,
            "grit should be bright (high-pass), r1={}",
            num / den
        );
    }

    #[test]
    fn no_dither_constant_input_is_silent() {
        // With dither off, a constant input lands on a fixed grid level every
        // sample, so the differentiated grit is exactly zero.
        let mut m = make(2.0, 1.0, 0.25, 0.0);
        let grit = grit_series(&mut m, 2.0, 500);
        assert!(
            grit.iter().all(|&g| g == 0.0),
            "undithered constant input should yield no grit"
        );
    }

    #[test]
    fn output_is_finite() {
        let mut m = make(0.0, 5.0, 5.0, 1.0);
        for n in 0..2000 {
            let x = (n as f32 * 0.01).sin() * 5.0;
            m.params.input = PolySignal::mono(Signal::Volts(x));
            m.update(48000.0);
            assert!(m.outputs.sample.get(0).is_finite());
        }
    }
}
