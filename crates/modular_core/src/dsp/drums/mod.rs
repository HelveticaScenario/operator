//! Drum voices modelled on classic drum machines, grouped by machine.

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{ModuleSchema, SampleableConstructor};

mod analog;
mod clap;
mod group;
mod input;
mod lanes;
mod mna;
mod pitch_level;
#[cfg(test)]
mod tests;
pub mod tr808;
pub mod tr909;
mod voice;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    tr808::install_constructors(map);
    tr909::install_constructors(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    tr808::install_params_deserializers(map);
    tr909::install_params_deserializers(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    let mut schemas = tr808::schemas();
    schemas.extend(tr909::schemas());
    schemas
}
