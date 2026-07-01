//! `$unstable.shape.saturate` — Surge XT's Saturator waveshaper group.
//!
//! Ported from sst-waveshapers (GPL-3.0-or-later).

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::shape::shape_core::Shaper;
use crate::dsp::shape::shapers;

/// Saturation algorithm.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum SaturateMode {
    /// Smooth, warm saturation.
    #[default]
    Soft,
    /// Hard clipping — aggressive, squared-off edges.
    Hard,
    /// Asymmetric saturation — adds even harmonics for a warmer colour.
    Asymmetric,
    /// Medium saturation — sits between soft and hard.
    Medium,
    /// Overdrive with a soft knee before it clips.
    Ojd,
}

/// Stateless — the saturator modes carry no history.
#[derive(Clone, Copy, Default)]
pub struct SaturateShaper;

impl Shaper for SaturateShaper {
    type Mode = SaturateMode;

    #[inline]
    fn shape(&mut self, x: f32, drive: f32, mode: SaturateMode, _dc: f32) -> f32 {
        match mode {
            SaturateMode::Soft => shapers::tanh(x, drive),
            SaturateMode::Hard => shapers::clip(x, drive),
            SaturateMode::Asymmetric => shapers::asym(x, drive),
            SaturateMode::Medium => shapers::zamsat(x, drive),
            SaturateMode::Ojd => shapers::ojd(x, drive),
        }
    }
}

shape_module! {
    /// Saturation and clipping, from Surge XT's Saturator waveshapers. `mode`
    /// sets the character — soft, hard, asymmetric, medium, or ojd — and `drive`
    /// sets how hard the signal is pushed in (−5..5, 0 = unity).
    ///
    /// ## Example
    /// ```js
    /// $unstable.shape.saturate($saw('c3'), 'hard', 3).out()
    /// ```
    name = "$unstable.shape.saturate", ident = Saturate, mode = SaturateMode, shaper = SaturateShaper
}
