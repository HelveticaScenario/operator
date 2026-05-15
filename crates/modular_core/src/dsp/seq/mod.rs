//! Sequencer modules for the modular synthesizer.
//!
//! This module provides:
//! - `Seq`: A Strudel/TidalCycles style sequencer using the new pattern system
//! - `IntervalSeq`: A scale-degree sequencer with additive patterns

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod interval_seq;
pub mod scale;
pub mod seq;
pub mod seq_value;
pub mod step;
pub mod track;

pub use interval_seq::{IntervalPatternParam, IntervalSeq, IntervalValue};
pub use scale::{FixedRoot, ScaleRoot, ScaleSnapper};
pub use seq_value::{SeqPatternParam, SeqValue};

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    seq::Seq::install_constructor(map);
    track::Track::install_constructor(map);
    interval_seq::IntervalSeq::install_constructor(map);
    step::Step::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    seq::Seq::install_params_deserializer(map);
    track::Track::install_params_deserializer(map);
    interval_seq::IntervalSeq::install_params_deserializer(map);
    step::Step::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        seq::Seq::get_schema(),
        track::Track::get_schema(),
        interval_seq::IntervalSeq::get_schema(),
        step::Step::get_schema(),
    ]
}
