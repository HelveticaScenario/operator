//! `$unstable.shape.digital` — Surge XT's digital (sample-quantizing) effect waveshaper.
//!
//! Ported from sst-waveshapers (GPL-3.0).

use crate::dsp::shape::shape_core::Shaper;
use crate::dsp::shape::shapers;

/// Stateless — quantization carries no history.
#[derive(Clone, Copy, Default)]
pub struct DigitalShaper;

impl Shaper for DigitalShaper {
    type Mode = ();

    #[inline]
    fn shape(&mut self, x: f32, drive: f32, _mode: (), _dc: f32) -> f32 {
        shapers::digital(x, drive)
    }
}

shape_module! {
    /// Bit-crusher, from Surge XT's Effect waveshaper — steps the signal down to
    /// coarse levels for a lo-fi, digital edge. `drive` sets how coarse the steps
    /// are: higher is crunchier (−5..5, 0 = unity).
    ///
    /// ## Example
    /// ```js
    /// $unstable.shape.digital($saw('c3'), 2).out()
    /// ```
    name = "$unstable.shape.digital", ident = Digital, shaper = DigitalShaper
}
