use std::collections::HashMap;

use deserr::Deserr;
use schemars::JsonSchema;
use serde::Serialize;

use crate::dsp::utils::voct_to_hz;
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
