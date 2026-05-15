use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod adsr;
pub mod buffer;
pub mod clamp;
pub mod clock_divider;
pub mod curve;
pub mod delay;
pub mod lag;
pub mod logic;
pub mod math;
pub mod percussion_envelope;
pub mod quantizer;
pub mod remap;
pub mod sample_and_hold;
pub mod scale;
pub mod scale_and_shift;
pub mod spread;
pub mod unison;
pub mod wrap;

// Re-export useful types
pub use crate::dsp::utils::SchmittTrigger;
pub use scale::{FixedRoot, ScaleSnapper, validate_scale_type};

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    adsr::Adsr::install_constructor(map);
    buffer::BufRead::install_constructor(map);
    buffer::BufferWrite::install_constructor(map);
    clamp::Clamp::install_constructor(map);
    clock_divider::ClockDivider::install_constructor(map);
    curve::Curve::install_constructor(map);
    delay::DelayRead::install_constructor(map);
    lag::LagProcessor::install_constructor(map);
    logic::RisingEdgeDetector::install_constructor(map);
    logic::FallingEdgeDetector::install_constructor(map);
    math::Math::install_constructor(map);
    remap::Remap::install_constructor(map);
    sample_and_hold::SampleAndHold::install_constructor(map);
    sample_and_hold::TrackAndHold::install_constructor(map);
    percussion_envelope::PercussionEnvelope::install_constructor(map);
    quantizer::Quantizer::install_constructor(map);
    scale_and_shift::ScaleAndShift::install_constructor(map);
    spread::Spread::install_constructor(map);
    unison::Unison::install_constructor(map);
    wrap::Wrap::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    adsr::Adsr::install_params_deserializer(map);
    buffer::BufRead::install_params_deserializer(map);
    buffer::BufferWrite::install_params_deserializer(map);
    clamp::Clamp::install_params_deserializer(map);
    clock_divider::ClockDivider::install_params_deserializer(map);
    curve::Curve::install_params_deserializer(map);
    delay::DelayRead::install_params_deserializer(map);
    lag::LagProcessor::install_params_deserializer(map);
    logic::RisingEdgeDetector::install_params_deserializer(map);
    logic::FallingEdgeDetector::install_params_deserializer(map);
    math::Math::install_params_deserializer(map);
    remap::Remap::install_params_deserializer(map);
    sample_and_hold::SampleAndHold::install_params_deserializer(map);
    sample_and_hold::TrackAndHold::install_params_deserializer(map);
    percussion_envelope::PercussionEnvelope::install_params_deserializer(map);
    quantizer::Quantizer::install_params_deserializer(map);
    scale_and_shift::ScaleAndShift::install_params_deserializer(map);
    spread::Spread::install_params_deserializer(map);
    unison::Unison::install_params_deserializer(map);
    wrap::Wrap::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        adsr::Adsr::get_schema(),
        buffer::BufRead::get_schema(),
        buffer::BufferWrite::get_schema(),
        clamp::Clamp::get_schema(),
        clock_divider::ClockDivider::get_schema(),
        curve::Curve::get_schema(),
        delay::DelayRead::get_schema(),
        lag::LagProcessor::get_schema(),
        logic::RisingEdgeDetector::get_schema(),
        logic::FallingEdgeDetector::get_schema(),
        math::Math::get_schema(),
        remap::Remap::get_schema(),
        sample_and_hold::SampleAndHold::get_schema(),
        sample_and_hold::TrackAndHold::get_schema(),
        percussion_envelope::PercussionEnvelope::get_schema(),
        quantizer::Quantizer::get_schema(),
        scale_and_shift::ScaleAndShift::get_schema(),
        spread::Spread::get_schema(),
        unison::Unison::get_schema(),
        wrap::Wrap::get_schema(),
    ]
}
