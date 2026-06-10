use std::collections::HashMap;

use deserr::Deserr;
use schemars::JsonSchema;
use serde::Serialize;

use crate::dsp::utils::{GATE_DETECTION_HIGH_THRESHOLD, voct_to_hz};
use crate::params::ParamsDeserializer;
use crate::patch::Patch;
use crate::types::{Connect, Module, ModuleSchema, SampleableConstructor};

/// FM synthesis mode for oscillators.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserr, Serialize, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum FmMode {
    /// Through-zero FM: frequency can go negative (phase runs backward)
    #[default]
    ThroughZero,
    /// Linear FM: like through-zero but frequency clamped to >= 0
    Lin,
    /// Exponential FM: modulator added to pitch in V/Oct space
    Exp,
}

/// Calculate frequency with FM modulation applied.
///
/// Given a base pitch in V/Oct, an FM modulation value, and an FM mode,
/// returns the modulated frequency in Hz.
#[inline]
pub fn apply_fm(pitch: f32, fm: f32, fm_mode: FmMode) -> f32 {
    match fm_mode {
        FmMode::Exp => voct_to_hz(pitch + fm),
        FmMode::Lin => (voct_to_hz(pitch) * (1.0 + fm)).max(0.0),
        FmMode::ThroughZero => voct_to_hz(pitch) * (1.0 + fm),
    }
}

/// Subsample position in `[0, 1)` at which a rising sync edge crossed the gate
/// high threshold, linearly interpolated between the previous and current sync
/// samples. Used to place the [`sync_blep`] correction within the sample.
#[inline]
pub fn sync_edge_fraction(prev: f32, curr: f32) -> f32 {
    let denom = curr - prev;
    if denom > 1.0e-12 {
        ((GATE_DETECTION_HIGH_THRESHOLD - prev) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Two-point PolyBLEP correction for a hard-sync phase reset.
///
/// The reset is a step of size `jump` (the naive output *after* the reset minus
/// the value *before* it) located `frac` of a sample into the upcoming sample
/// interval. Returns `(this_sample, next_sample)`: residuals to add to the
/// naive output now and to carry into the following sample. Calibrated to match
/// the ±1-normalized PolyBLEP used for the oscillators' natural edges, so the
/// synced sample lands on the band-limited midpoint when `frac == 0`.
#[inline]
pub fn sync_blep(jump: f32, frac: f32) -> (f32, f32) {
    let half = 0.5 * jump;
    let before = 1.0 - frac;
    (half * before * before, -half * frac * frac)
}

pub mod noise;
pub mod p_pulse;
pub mod p_saw;
pub mod p_sine;
pub mod plaits;
pub mod pulse;
pub mod saw;
pub mod sine;
pub mod supersaw;
pub mod wavetable;
pub mod wavetable_prep;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    sine::SineOscillator::install_constructor(map);
    saw::SawOscillator::install_constructor(map);
    pulse::PulseOscillator::install_constructor(map);
    p_sine::PSineOscillator::install_constructor(map);
    p_saw::PSawOscillator::install_constructor(map);
    p_pulse::PPulseOscillator::install_constructor(map);
    noise::Noise::install_constructor(map);
    plaits::Plaits::install_constructor(map);
    supersaw::Supersaw::install_constructor(map);
    wavetable::WavetableOsc::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    sine::SineOscillator::install_params_deserializer(map);
    saw::SawOscillator::install_params_deserializer(map);
    pulse::PulseOscillator::install_params_deserializer(map);
    p_sine::PSineOscillator::install_params_deserializer(map);
    p_saw::PSawOscillator::install_params_deserializer(map);
    p_pulse::PPulseOscillator::install_params_deserializer(map);
    noise::Noise::install_params_deserializer(map);
    plaits::Plaits::install_params_deserializer(map);
    supersaw::Supersaw::install_params_deserializer(map);
    wavetable::WavetableOsc::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        sine::SineOscillator::get_schema(),
        saw::SawOscillator::get_schema(),
        pulse::PulseOscillator::get_schema(),
        p_sine::PSineOscillator::get_schema(),
        p_saw::PSawOscillator::get_schema(),
        p_pulse::PPulseOscillator::get_schema(),
        noise::Noise::get_schema(),
        plaits::Plaits::get_schema(),
        supersaw::Supersaw::get_schema(),
        wavetable::WavetableOsc::get_schema(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_edge_fraction_interpolates_threshold_crossing() {
        // Crossing 1.0V on the way up from 0 to 5 happens 1/5 of the way in.
        assert!((sync_edge_fraction(0.0, 5.0) - 0.2).abs() < 1e-6);
        // Midway crossing.
        assert!((sync_edge_fraction(0.5, 1.5) - 0.5).abs() < 1e-6);
        // Already above threshold at the start → crossing at the very start.
        assert_eq!(sync_edge_fraction(1.0, 5.0), 0.0);
        // Non-rising / degenerate inputs fall back to 0.
        assert_eq!(sync_edge_fraction(2.0, 1.0), 0.0);
        assert_eq!(sync_edge_fraction(3.0, 3.0), 0.0);
    }

    #[test]
    fn sync_blep_lands_the_discontinuity_sample_on_the_midpoint() {
        let m = 2.0;
        // frac == 0: the whole correction is on this sample → before + M/2.
        let (now, carry) = sync_blep(m, 0.0);
        assert!((now - 1.0).abs() < 1e-6);
        assert!(carry.abs() < 1e-6);
        // frac → 1: nothing now, the next sample carries −M/2 → after − M/2.
        let (now, carry) = sync_blep(m, 1.0);
        assert!(now.abs() < 1e-6);
        assert!((carry + 1.0).abs() < 1e-6);
        // A zero jump produces no correction at all.
        assert_eq!(sync_blep(0.0, 0.3), (0.0, 0.0));
    }
}
