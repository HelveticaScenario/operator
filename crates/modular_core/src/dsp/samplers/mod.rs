use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod grains;
pub mod sampler;
mod slice;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    grains::Grains::install_constructor(map);
    sampler::Sampler::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    grains::Grains::install_params_deserializer(map);
    sampler::Sampler::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![grains::Grains::get_schema(), sampler::Sampler::get_schema()]
}
