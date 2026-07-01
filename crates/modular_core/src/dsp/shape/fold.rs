//! `$unstable.shape.fold` — Surge XT's Wavefolder group.
//!
//! Ported from sst-waveshapers (GPL-3.0).

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

/// Holds the ADAA memory the piecewise folders need.
#[derive(Clone, Copy, Default)]
pub struct FoldShaper {
    adaa: Adaa,
}

impl Shaper for FoldShaper {
    type Mode = FoldMode;

    #[inline]
    fn shape(&mut self, x: f32, drive: f32, mode: FoldMode, _dc: f32) -> f32 {
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
