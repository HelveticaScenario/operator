//! Signal sources driven by simulated systems: strange attractors and
//! physical models that exhibit chaotic or complex behaviour.

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod lorenz;
pub mod swell;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    lorenz::Lorenz::install_constructor(map);
    swell::Swell::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    lorenz::Lorenz::install_params_deserializer(map);
    swell::Swell::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![lorenz::Lorenz::get_schema(), swell::Swell::get_schema()]
}
