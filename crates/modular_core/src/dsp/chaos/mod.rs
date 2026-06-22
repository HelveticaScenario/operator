//! Chaos and strange-attractor signal sources.
//!
//! Contains modules that generate signals from dynamical systems exhibiting
//! chaotic or complex behaviour.

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod lorenz;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    lorenz::Lorenz::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    lorenz::Lorenz::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![lorenz::Lorenz::get_schema()]
}
