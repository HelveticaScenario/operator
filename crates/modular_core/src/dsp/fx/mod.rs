//! Effects (FX) modules category.
//!
//! Contains waveshaping and distortion effects adapted from
//! the 4ms Ensemble Oscillator warp and twist modes.
//! Copyright 4ms Company. Used under GPL v3.

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod enosc_tables;

pub mod cheby;
pub mod dattorro;
pub mod filterdc;
pub mod fold;
pub mod overdrive;
pub mod plate;
pub mod quant_noise;
pub mod reverb;
pub mod reverb2;
pub mod segment;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    fold::Fold::install_constructor(map);
    cheby::Cheby::install_constructor(map);
    dattorro::Dattorro::install_constructor(map);
    overdrive::Overdrive::install_constructor(map);
    filterdc::FilterDc::install_constructor(map);
    plate::Plate::install_constructor(map);
    quant_noise::QuantNoise::install_constructor(map);
    reverb::Reverb::install_constructor(map);
    reverb2::Reverb2::install_constructor(map);
    segment::Segment::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    fold::Fold::install_params_deserializer(map);
    cheby::Cheby::install_params_deserializer(map);
    dattorro::Dattorro::install_params_deserializer(map);
    overdrive::Overdrive::install_params_deserializer(map);
    filterdc::FilterDc::install_params_deserializer(map);
    plate::Plate::install_params_deserializer(map);
    quant_noise::QuantNoise::install_params_deserializer(map);
    reverb::Reverb::install_params_deserializer(map);
    reverb2::Reverb2::install_params_deserializer(map);
    segment::Segment::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        fold::Fold::get_schema(),
        cheby::Cheby::get_schema(),
        dattorro::Dattorro::get_schema(),
        overdrive::Overdrive::get_schema(),
        filterdc::FilterDc::get_schema(),
        plate::Plate::get_schema(),
        quant_noise::QuantNoise::get_schema(),
        reverb::Reverb::get_schema(),
        reverb2::Reverb2::get_schema(),
        segment::Segment::get_schema(),
    ]
}
