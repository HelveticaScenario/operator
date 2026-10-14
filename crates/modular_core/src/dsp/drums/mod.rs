use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod clang;
pub mod clap;
mod common;
pub mod cymbal;
pub mod fm_drum;
pub mod hihat;
pub mod kick;
pub mod snare;
pub mod tom;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    clang::Clang::install_constructor(map);
    clap::Clap::install_constructor(map);
    cymbal::Cymbal::install_constructor(map);
    fm_drum::FmDrum::install_constructor(map);
    hihat::HiHat::install_constructor(map);
    kick::Kick::install_constructor(map);
    snare::Snare::install_constructor(map);
    tom::Tom::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    clang::Clang::install_params_deserializer(map);
    clap::Clap::install_params_deserializer(map);
    cymbal::Cymbal::install_params_deserializer(map);
    fm_drum::FmDrum::install_params_deserializer(map);
    hihat::HiHat::install_params_deserializer(map);
    kick::Kick::install_params_deserializer(map);
    snare::Snare::install_params_deserializer(map);
    tom::Tom::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        clang::Clang::get_schema(),
        clap::Clap::get_schema(),
        cymbal::Cymbal::get_schema(),
        fm_drum::FmDrum::get_schema(),
        hihat::HiHat::get_schema(),
        kick::Kick::get_schema(),
        snare::Snare::get_schema(),
        tom::Tom::get_schema(),
    ]
}
