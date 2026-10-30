//! Phase modules category.
//!
//! Contains phase generators and phase-distortion effects.
//! Phase modules operate on 0-to-1 phase signals rather than audio waveforms.

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod p_warp;
pub mod ramp;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    p_warp::PWarp::install_constructor(map);
    ramp::Ramp::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    p_warp::PWarp::install_params_deserializer(map);
    ramp::Ramp::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![p_warp::PWarp::get_schema(), ramp::Ramp::get_schema()]
}
