//! `$unstable.shape.trigonometric` — Surge XT's Trigonometric waveshaper group.
//!
//! Ported from sst-waveshapers (GPL-3.0-or-later).

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::shape::shape_core::Shaper;
use crate::dsp::shape::shapers;

/// Trigonometric shaping algorithm.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum TrigonometricMode {
    /// Adds a single sine ripple to the signal.
    #[default]
    SinPlusX,
    /// Adds 2 sine ripples riding on the signal.
    #[serde(rename = "sin2x")]
    #[deserr(rename = "sin2x")]
    Sin2x,
    /// Adds 3 sine ripples riding on the signal.
    #[serde(rename = "sin3x")]
    #[deserr(rename = "sin3x")]
    Sin3x,
    /// Adds 7 sine ripples riding on the signal.
    #[serde(rename = "sin7x")]
    #[deserr(rename = "sin7x")]
    Sin7x,
    /// Adds 10 sine ripples riding on the signal.
    #[serde(rename = "sin10x")]
    #[deserr(rename = "sin10x")]
    Sin10x,
    /// Maps the signal through 2 sine cycles — dense harmonics.
    Cyc2,
    /// Maps the signal through 7 sine cycles — denser harmonics.
    Cyc7,
    /// Maps the signal through 10 sine cycles — densest harmonics.
    Cyc10,
    /// 2 sine cycles, tapered toward the edges.
    Cyc2Bound,
    /// 7 sine cycles, tapered toward the edges.
    Cyc7Bound,
    /// 10 sine cycles, tapered toward the edges.
    Cyc10Bound,
}

/// Stateless — the trigonometric modes carry no history.
#[derive(Clone, Copy, Default)]
pub struct TrigonometricShaper;

impl Shaper for TrigonometricShaper {
    type Mode = TrigonometricMode;

    #[inline]
    fn shape(&mut self, x: f32, drive: f32, mode: TrigonometricMode, _dc: f32) -> f32 {
        use TrigonometricMode::*;
        match mode {
            SinPlusX => shapers::sin_plus_x(x, drive),
            Sin2x => shapers::sin_nx_plus_x_bound(x, drive, 2.0),
            Sin3x => shapers::sin_nx_plus_x_bound(x, drive, 3.0),
            Sin7x => shapers::sin_nx_plus_x_bound(x, drive, 7.0),
            Sin10x => shapers::sin_nx_plus_x_bound(x, drive, 10.0),
            Cyc2 => shapers::sin_nx(x, drive, 2.0),
            Cyc7 => shapers::sin_nx(x, drive, 7.0),
            Cyc10 => shapers::sin_nx(x, drive, 10.0),
            Cyc2Bound => shapers::sin_nx_bound(x, drive, 2.0),
            Cyc7Bound => shapers::sin_nx_bound(x, drive, 7.0),
            Cyc10Bound => shapers::sin_nx_bound(x, drive, 10.0),
        }
    }
}

shape_module! {
    /// Sine-based shaping, from Surge XT's Trigonometric waveshapers — reshapes
    /// the signal through sine curves for dense, ringing harmonics. `mode`
    /// selects the pattern (sinPlusX, sin2x…sin10x, cyc2…cyc10, and edge-tapered
    /// variants); `drive` sets the level going in (−5..5, 0 = unity).
    ///
    /// ## Example
    /// ```js
    /// $unstable.shape.trigonometric($sine('c3'), 'cyc7', 2).out()
    /// ```
    name = "$unstable.shape.trigonometric", ident = Trigonometric, mode = TrigonometricMode, shaper = TrigonometricShaper
}
