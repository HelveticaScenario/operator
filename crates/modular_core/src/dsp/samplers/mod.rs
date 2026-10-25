use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod media_audio;
pub mod sampler;
mod slice;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    media_audio::MediaAudio::install_constructor(map);
    sampler::Sampler::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    media_audio::MediaAudio::install_params_deserializer(map);
    sampler::Sampler::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        media_audio::MediaAudio::get_schema(),
        sampler::Sampler::get_schema(),
    ]
}
