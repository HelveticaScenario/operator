use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod sampler;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    sampler::Sampler::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    sampler::Sampler::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![sampler::Sampler::get_schema()]
}
