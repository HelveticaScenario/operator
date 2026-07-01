//! `$unstable.shape.fuzz` — Surge XT's Fuzz waveshaper group.
//!
//! The fuzz curves are frozen, seeded noise tables; `prime` builds them on the
//! main thread so the fill never runs on the audio thread. Ported from
//! sst-waveshapers (GPL-3.0).

use deserr::Deserr;
use schemars::JsonSchema;

use crate::dsp::shape::shape_core::Shaper;
use crate::dsp::shape::shapers::{self, FuzzKind};
use crate::dsp::utils::dc_blocker::DcBlocker;

/// Fuzz algorithm.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum FuzzMode {
    /// Hard, gated fuzz.
    #[default]
    Fuzz,
    /// Softer fuzz.
    Soft,
    /// Heavier, noisier fuzz.
    Heavy,
    /// Fuzz focused around the zero crossings.
    Center,
    /// Fuzz focused on the loud peaks.
    SoftEdge,
}

/// Holds the DC blocker applied after the fuzz table.
#[derive(Clone, Copy, Default)]
pub struct FuzzShaper {
    dc: DcBlocker,
}

impl Shaper for FuzzShaper {
    type Mode = FuzzMode;

    #[inline]
    fn shape(&mut self, x: f32, drive: f32, mode: FuzzMode, dc: f32) -> f32 {
        // The pre-shaper (clip or tanh) and which frozen table to read.
        let (driven, kind) = match mode {
            FuzzMode::Fuzz => (shapers::clip(x, drive), FuzzKind::Standard),
            FuzzMode::Heavy => (shapers::clip(x, drive), FuzzKind::Heavy),
            FuzzMode::Soft => (shapers::tanh(x, drive), FuzzKind::Standard),
            FuzzMode::Center => (shapers::tanh(x, drive), FuzzKind::Center),
            FuzzMode::SoftEdge => (shapers::tanh(x, drive), FuzzKind::Edge),
        };
        let looked = shapers::fuzz_lookup(kind, driven);
        self.dc.process(looked, dc)
    }

    fn prime() {
        shapers::prime_fuzz_tables();
    }
}

shape_module! {
    /// Fuzz, from Surge XT's Fuzz waveshapers — a gritty, broken-up distortion.
    /// `mode` selects the flavour (fuzz, soft, heavy, center, softEdge); `drive`
    /// sets the level going in (−5..5, 0 = unity).
    ///
    /// ## Example
    /// ```js
    /// $unstable.shape.fuzz($saw('c3'), 'heavy', 2).out()
    /// ```
    name = "$unstable.shape.fuzz", ident = Fuzz, mode = FuzzMode, shaper = FuzzShaper
}
