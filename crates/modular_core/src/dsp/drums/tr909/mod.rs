//! Roland TR-909 analog voices, exposed as `$tr909.*`.

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod bd;
mod circuit;
pub mod cp;
mod noise;
pub mod rs;
pub mod sd;
pub mod tom;
mod vco;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    bd::Bd909::install_constructor(map);
    sd::Sd909::install_constructor(map);
    rs::Rs909::install_constructor(map);
    cp::Cp909::install_constructor(map);
    tom::Lt909::install_constructor(map);
    tom::Mt909::install_constructor(map);
    tom::Ht909::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    bd::Bd909::install_params_deserializer(map);
    sd::Sd909::install_params_deserializer(map);
    rs::Rs909::install_params_deserializer(map);
    cp::Cp909::install_params_deserializer(map);
    tom::Lt909::install_params_deserializer(map);
    tom::Mt909::install_params_deserializer(map);
    tom::Ht909::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        bd::Bd909::get_schema(),
        sd::Sd909::get_schema(),
        rs::Rs909::get_schema(),
        cp::Cp909::get_schema(),
        tom::Lt909::get_schema(),
        tom::Mt909::get_schema(),
        tom::Ht909::get_schema(),
    ]
}
