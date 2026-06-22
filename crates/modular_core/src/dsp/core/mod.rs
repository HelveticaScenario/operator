use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod audio_in;
pub mod clock;
pub mod mix;
pub mod mix_down;
pub mod signal;
pub mod stereo_mixer;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    signal::Signal::install_constructor(map);
    mix::Mix::install_constructor(map);
    mix_down::MixDown::install_constructor(map);
    stereo_mixer::StereoMixer::install_constructor(map);
    clock::Clock::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    signal::Signal::install_params_deserializer(map);
    mix::Mix::install_params_deserializer(map);
    mix_down::MixDown::install_params_deserializer(map);
    stereo_mixer::StereoMixer::install_params_deserializer(map);
    clock::Clock::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        signal::Signal::get_schema(),
        mix::Mix::get_schema(),
        mix_down::MixDown::get_schema(),
        stereo_mixer::StereoMixer::get_schema(),
        clock::Clock::get_schema(),
    ]
}
