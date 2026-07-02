//! `$unstable.shape.fold` — Surge XT's Wavefolder group.
//!
//! Ported from sst-waveshapers (GPL-3.0-or-later).

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::shape::shape_core::Shaper;
use crate::dsp::shape::shapers;
use crate::dsp::utils::adaa::Adaa;

/// Wavefolding algorithm.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum FoldMode {
    /// A single, smooth fold.
    #[default]
    Soft,
    /// A single sharp fold.
    Single,
    /// A double fold — more harmonics.
    Dual,
    /// West-coast (Buchla-style) fold — complex, metallic harmonics.
    Westcoast,
    /// Triangle fold — hard and buzzy.
    Linear,
    /// Sine fold — wraps the signal smoothly through a sine.
    Sine,
}

/// Holds the ADAA memory the piecewise folders need, and the mode that memory
/// belongs to — the memory resets on a mode change (see [`Adaa::reset`]).
#[derive(Clone, Copy, Default)]
pub struct FoldShaper {
    adaa: Adaa,
    adaa_mode: FoldMode,
}

impl Shaper for FoldShaper {
    type Mode = FoldMode;

    #[inline]
    fn shape(&mut self, x: f32, drive: f32, mode: FoldMode, _dc: f32) -> f32 {
        if mode != self.adaa_mode {
            self.adaa.reset();
            self.adaa_mode = mode;
        }
        match mode {
            FoldMode::Soft => shapers::soft_one_fold(x, drive),
            FoldMode::Single => {
                let d = x * drive;
                let (f, ad) = shapers::SINGLE_FOLD.evaluate(d);
                self.adaa.process(d, f, ad)
            }
            FoldMode::Dual => {
                let d = x * drive;
                let (f, ad) = shapers::DUAL_FOLD.evaluate(d);
                self.adaa.process(d, f, ad)
            }
            FoldMode::Westcoast => {
                let d = x * drive;
                let (f, ad) = shapers::WESTCOAST_FOLD.evaluate(d);
                self.adaa.process(d, f, ad)
            }
            FoldMode::Linear => shapers::linear_fold(x, drive),
            FoldMode::Sine => shapers::sine_shaper(x, drive, true),
        }
    }
}

shape_module! {
    /// Wavefolding, from Surge XT's Wavefolder waveshapers — folds peaks back on
    /// themselves to add rich harmonics that grow with level. `mode` selects the
    /// fold shape (soft, single, dual, westcoast, linear, sine); `drive` pushes
    /// harder into the folds as it rises (−5..5, 0 = unity).
    ///
    /// ## Example
    /// ```js
    /// $unstable.shape.fold($sine('c3'), 'westcoast', 3).out()
    /// ```
    name = "$unstable.shape.fold", ident = Fold, mode = FoldMode, shaper = FoldShaper
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::shape::shape_core::{ShapeChannel, ShapeModuleState, run};
    use crate::poly::{PolyOutput, PolySignal};
    use crate::types::Signal;

    /// The ADAA memory belongs to one fold shape; switching modes mid-stream must
    /// re-seed it. Differencing the new antiderivative against the old memory
    /// divides their offset by a near-zero step — a multi-hundred-volt spike.
    #[test]
    fn mode_switch_does_not_spike() {
        let sr = 48_000.0;
        let mut state = ShapeModuleState::default();
        state.init(sr);
        let mut ch: Vec<ShapeChannel<FoldShaper>> = vec![ShapeChannel::default()];
        let mut out = PolyOutput::mono(0.0);
        let drive = Some(PolySignal::mono(Signal::Volts(3.0)));
        let sig = |i: usize| (2.0 * std::f32::consts::PI * 220.0 * (i as f32 / sr)).sin() * 5.0;

        for i in 0..4000 {
            let input = PolySignal::mono(Signal::Volts(sig(i)));
            run::<FoldShaper>(1, &input, &drive, FoldMode::Dual, &state, &mut ch, &mut out);
        }
        for i in 0..256 {
            let input = PolySignal::mono(Signal::Volts(sig(4000 + i)));
            run::<FoldShaper>(
                1,
                &input,
                &drive,
                FoldMode::Westcoast,
                &state,
                &mut ch,
                &mut out,
            );
            let y = out.get(0);
            assert!(
                y.abs() < 8.0,
                "fold mode switch spiked: {y} at sample {i} after the switch"
            );
        }
    }
}
