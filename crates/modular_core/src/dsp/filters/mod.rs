use crate::module_state::ModuleStateBuilder;
use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};
use std::collections::HashMap;

pub mod bandpass;
pub mod eq;
pub mod highpass;
pub mod jup6f;
pub mod lowpass;
pub mod resonant_svf;
pub mod surge;
pub mod svf_core;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    lowpass::LowpassFilter::install_constructor(map);
    highpass::HighpassFilter::install_constructor(map);
    bandpass::BandpassFilter::install_constructor(map);
    jup6f::Jup6f::install_constructor(map);
    eq::ParametricEq::install_constructor(map);
    surge::install_constructors(map);
}

/// Register the per-module editor-state builders for this category.
pub fn install_module_state_builders(map: &mut HashMap<String, ModuleStateBuilder>) {
    map.insert(
        <eq::ParametricEq as Module>::MODULE_TYPE.to_string(),
        eq::eq_state_builder,
    );
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    lowpass::LowpassFilter::install_params_deserializer(map);
    highpass::HighpassFilter::install_params_deserializer(map);
    bandpass::BandpassFilter::install_params_deserializer(map);
    jup6f::Jup6f::install_params_deserializer(map);
    eq::ParametricEq::install_params_deserializer(map);
    surge::install_params_deserializers(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    let mut schemas = vec![
        lowpass::LowpassFilter::get_schema(),
        highpass::HighpassFilter::get_schema(),
        bandpass::BandpassFilter::get_schema(),
        jup6f::Jup6f::get_schema(),
        eq::ParametricEq::get_schema(),
    ];
    schemas.extend(surge::schemas());
    schemas
}
