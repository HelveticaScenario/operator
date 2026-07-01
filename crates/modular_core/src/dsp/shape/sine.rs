//! `$unstable.shape.sine` — Surge XT's sine effect waveshaper (non-folding).
//!
//! Ported from sst-waveshapers (GPL-3.0-or-later).

use crate::dsp::shape::shape_core::Shaper;
use crate::dsp::shape::shapers;

/// Stateless — the sine shaper carries no history.
#[derive(Clone, Copy, Default)]
pub struct SineShaper;

impl Shaper for SineShaper {
    type Mode = ();

    #[inline]
    fn shape(&mut self, x: f32, drive: f32, _mode: (), _dc: f32) -> f32 {
        shapers::sine_shaper(x, drive, false)
    }
}

shape_module! {
    /// Sine shaper, from Surge XT's Effect waveshaper — reshapes the signal
    /// through a sine curve for smooth added harmonics. Higher `drive` pushes
    /// further through the curve for more harmonics (−5..5, 0 = unity).
    ///
    /// ## Example
    /// ```js
    /// $unstable.shape.sine($sine('c3'), 3).out()
    /// ```
    name = "$unstable.shape.sine", ident = Sine, shaper = SineShaper
}
