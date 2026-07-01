//! `$unstable.shape.rectify` — Surge XT's Rectifier waveshaper group.
//!
//! Ported from sst-waveshapers (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::shape::shape_core::Shaper;
use crate::dsp::shape::shapers;
use crate::dsp::utils::adaa::Adaa;
use crate::dsp::utils::dc_blocker::DcBlocker;

/// Rectification algorithm.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum RectifyMode {
    /// Full-wave — flips the negative half up (doubles the pitch).
    #[default]
    Full,
    /// Positive half-wave — keeps only the upper half.
    Positive,
    /// Negative half-wave — keeps only the lower half.
    Negative,
    /// Softer rectification with rounded edges.
    Soft,
}

/// Holds the ADAA memory (and, for soft, a DC blocker).
#[derive(Clone, Copy, Default)]
pub struct RectifyShaper {
    adaa: Adaa,
    dc: DcBlocker,
}

impl Shaper for RectifyShaper {
    type Mode = RectifyMode;

    #[inline]
    fn shape(&mut self, x: f32, drive: f32, mode: RectifyMode, dc: f32) -> f32 {
        match mode {
            RectifyMode::Full => {
                let d = shapers::clip(x, drive);
                let (f, ad) = shapers::fwrect_kernel(d);
                self.adaa.process(d, f, ad)
            }
            RectifyMode::Positive => {
                let d = shapers::clip(x, drive);
                let (f, ad) = shapers::posrect_kernel(d);
                self.adaa.process(d, f, ad)
            }
            RectifyMode::Negative => {
                let d = shapers::clip(x, drive);
                let (f, ad) = shapers::negrect_kernel(d);
                self.adaa.process(d, f, ad)
            }
            RectifyMode::Soft => {
                let (f, ad) = shapers::softrect_kernel(x);
                let r = self.adaa.process(x, f, ad);
                let r = self.dc.process(r, dc);
                shapers::tanh(r, drive)
            }
        }
    }
}

shape_module! {
    /// Rectification, from Surge XT's Rectifier waveshapers — folds or gates the
    /// waveform for a harder, buzzier tone. `mode` selects full-wave,
    /// positive/negative half-wave, or a softer rectifier; `drive` sets the level
    /// going in (−5..5, 0 = unity).
    ///
    /// ## Example
    /// ```js
    /// $unstable.shape.rectify($sine('c3'), 'full', 1).out()
    /// ```
    name = "$unstable.shape.rectify", ident = Rectify, mode = RectifyMode, shaper = RectifyShaper
}
