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
pub mod fold;
pub mod overdrive;
pub mod plate;
pub mod segment;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    fold::Fold::install_constructor(map);
    cheby::Cheby::install_constructor(map);
    dattorro::Dattorro::install_constructor(map);
    overdrive::Overdrive::install_constructor(map);
    plate::Plate::install_constructor(map);
    segment::Segment::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    fold::Fold::install_params_deserializer(map);
    cheby::Cheby::install_params_deserializer(map);
    dattorro::Dattorro::install_params_deserializer(map);
    overdrive::Overdrive::install_params_deserializer(map);
    plate::Plate::install_params_deserializer(map);
    segment::Segment::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        fold::Fold::get_schema(),
        cheby::Cheby::get_schema(),
        dattorro::Dattorro::get_schema(),
        overdrive::Overdrive::get_schema(),
        plate::Plate::get_schema(),
        segment::Segment::get_schema(),
    ]
}
