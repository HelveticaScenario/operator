//! Blades output amp (IC18A): the mode VCAs' current into a transimpedance
//! stage whose feedback is R153 across C86 and four zeners, two facing each
//! way, so the output compresses softly above about 6 V.

use std::sync::LazyLock;

use super::components::{ZENER_FLOOR_V, ZENER_PAIR_CAPACITANCE_0V, zener_clamp};
use super::curve::Curve;

/// Feedback resistor R153.
const R_OUTPUT_FB: f64 = 120.0e3;
/// Gain R153 / R173 = 120k / 62k.
pub(super) const OUTPUT_GAIN: f32 = 120.0 / 62.0;
/// Feedback pole: R153 across C86 (18 pF) and the zener chain, two pairs in
/// series, at its 0 V capacitance.
pub(super) const OUTPUT_POLE_HZ: f32 =
    1.0 / (std::f32::consts::TAU * 120.0e3 * (18.0e-12 + ZENER_PAIR_CAPACITANCE_0V / 2.0));
/// OPA1679 output swing on the module's rails (+11.7 V after the reverse
/// protection Schottky, −12 V), from TI's macromodel.
pub(super) const RAIL_POS: f32 = 10.87;
pub(super) const RAIL_NEG: f32 = -11.14;

static CHAIN: LazyLock<Curve> =
    LazyLock::new(|| Curve::build(|x| zener_clamp(x, 2.0, R_OUTPUT_FB)));

/// Build the chain curve now (main thread) so the audio thread never triggers
/// it.
pub(super) fn prime() {
    LazyLock::force(&CHAIN);
}

/// Output voltage for `v`, the mode VCAs' output expressed as the voltage it
/// would develop across R173 (the amp inverts).
#[inline]
pub(super) fn output_amp(v: f32) -> f32 {
    let x = -OUTPUT_GAIN * v;
    // Below two pairs' floor voltage the chain carries nothing.
    if x.abs() < (2.0 * ZENER_FLOOR_V) as f32 {
        x
    } else {
        CHAIN.eval(x as f64) as f32
    }
}

/// Limits a decimated output to the op-amp's swing.
#[inline]
pub(super) fn rails(v: f32) -> f32 {
    v.clamp(RAIL_NEG, RAIL_POS)
}
