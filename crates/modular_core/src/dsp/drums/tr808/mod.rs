//! Roland TR-808 voices, exposed as `$tr808.*`.

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod bd;
pub mod cb;
mod circuit;
mod cp;
pub mod cy;
pub mod hh;
mod levels;
mod ma;
mod metal;
pub mod rs;
pub mod sd;
mod swing;
pub mod tom;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    bd::Bd808::install_constructor(map);
    sd::Sd808::install_constructor(map);
    rs::Rs808::install_constructor(map);
    rs::Cl808::install_constructor(map);
    cb::Cb808::install_constructor(map);
    cp::Cp808::install_constructor(map);
    ma::Ma808::install_constructor(map);
    cy::Cy808::install_constructor(map);
    hh::Hh808::install_constructor(map);
    tom::Lt808::install_constructor(map);
    tom::Mt808::install_constructor(map);
    tom::Ht808::install_constructor(map);
    tom::Lc808::install_constructor(map);
    tom::Mc808::install_constructor(map);
    tom::Hc808::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    bd::Bd808::install_params_deserializer(map);
    sd::Sd808::install_params_deserializer(map);
    rs::Rs808::install_params_deserializer(map);
    rs::Cl808::install_params_deserializer(map);
    cb::Cb808::install_params_deserializer(map);
    cp::Cp808::install_params_deserializer(map);
    ma::Ma808::install_params_deserializer(map);
    cy::Cy808::install_params_deserializer(map);
    hh::Hh808::install_params_deserializer(map);
    tom::Lt808::install_params_deserializer(map);
    tom::Mt808::install_params_deserializer(map);
    tom::Ht808::install_params_deserializer(map);
    tom::Lc808::install_params_deserializer(map);
    tom::Mc808::install_params_deserializer(map);
    tom::Hc808::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        bd::Bd808::get_schema(),
        sd::Sd808::get_schema(),
        rs::Rs808::get_schema(),
        rs::Cl808::get_schema(),
        cb::Cb808::get_schema(),
        cp::Cp808::get_schema(),
        ma::Ma808::get_schema(),
        cy::Cy808::get_schema(),
        hh::Hh808::get_schema(),
        tom::Lt808::get_schema(),
        tom::Mt808::get_schema(),
        tom::Ht808::get_schema(),
        tom::Lc808::get_schema(),
        tom::Mc808::get_schema(),
        tom::Hc808::get_schema(),
    ]
}
