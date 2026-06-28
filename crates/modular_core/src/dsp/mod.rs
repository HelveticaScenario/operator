use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{ModuleSchema, SampleableConstructor};

pub mod chaos;
pub mod consts;
pub mod core;
pub mod dynamics;
pub mod filters;
pub mod fx;
pub mod midi;
pub mod oscillators;
pub mod phase;
pub mod samplers;
pub mod seq;
pub mod tables;
pub mod utilities;
pub mod utils;

pub fn get_constructors() -> HashMap<String, SampleableConstructor> {
    let mut map = HashMap::new();
    chaos::install_constructors(&mut map);
    core::install_constructors(&mut map);
    dynamics::install_constructors(&mut map);
    fx::install_constructors(&mut map);
    oscillators::install_constructors(&mut map);
    filters::install_constructors(&mut map);
    phase::install_constructors(&mut map);
    utilities::install_constructors(&mut map);
    seq::install_constructors(&mut map);
    midi::install_constructors(&mut map);
    samplers::install_constructors(&mut map);
    map
}

/// Returns a map of `module_type` -> editor-state builder function.
///
/// A builder turns a module's raw params JSON into its pre-allocated live slot
/// plus immutable metadata (see [`crate::module_state`]). Only modules that
/// publish per-module editor state register one — currently just `$cycle`.
pub fn get_module_state_builders() -> HashMap<String, crate::module_state::ModuleStateBuilder> {
    let mut map = HashMap::new();
    seq::install_module_state_builders(&mut map);
    map
}

/// Returns a map of `module_type` -> params deserializer function.
///
/// A params deserializer takes a JSON value (with `__argument_spans` already stripped)
/// and returns a `CachedParams` containing the typed params and derived channel count.
pub fn get_params_deserializers() -> HashMap<String, ParamsDeserializer> {
    let mut map = HashMap::new();
    chaos::install_params_deserializers(&mut map);
    core::install_params_deserializers(&mut map);
    dynamics::install_params_deserializers(&mut map);
    fx::install_params_deserializers(&mut map);
    oscillators::install_params_deserializers(&mut map);
    filters::install_params_deserializers(&mut map);
    phase::install_params_deserializers(&mut map);
    utilities::install_params_deserializers(&mut map);
    seq::install_params_deserializers(&mut map);
    midi::install_params_deserializers(&mut map);
    samplers::install_params_deserializers(&mut map);
    map
}

pub fn schema() -> Vec<ModuleSchema> {
    [
        chaos::schemas(),
        core::schemas(),
        dynamics::schemas(),
        fx::schemas(),
        oscillators::schemas(),
        filters::schemas(),
        phase::schemas(),
        utilities::schemas(),
        seq::schemas(),
        midi::schemas(),
        samplers::schemas(),
    ]
    .concat()
}
