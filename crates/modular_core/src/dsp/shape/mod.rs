//! Waveshaper (`$unstable.shape.*`) module family.
//!
//! Each module wraps one of Surge XT's waveshaper groups (a `mode` enum selects
//! the algorithm), except `digital` and `sine`, which are single-algorithm.
//!
//! The DSP kernels are ported from `sst-waveshapers` by the Surge Synth Team
//! (Copyright 2018-2025, various authors), released under the GNU General Public
//! License version 3 or later; source at
//! <https://github.com/surge-synthesizer/sst-waveshapers>. This port is a
//! derivative work under that license, compatible with Operator's AGPL-3.0.

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

pub mod shape_core;
pub mod shapers;

/// Stamp a `$unstable.shape.*` module from its `mode` enum + [`shape_core::Shaper`] impl.
///
/// Every module shares the same shape: `input` (positional 1), `mode`
/// (positional 2, when present), `drive` (optional, positional 2 or 3), one
/// output, and the [`shape_core::run`] per-sample body. This emits that
/// boilerplate so each module only writes the parts that differ — the enum and
/// its dispatch.
macro_rules! shape_module {
    // Modules with a `mode` enum selecting the algorithm.
    (
        $(#[$doc:meta])*
        name = $name:literal, ident = $Struct:ident, mode = $Mode:ty, shaper = $Shaper:ty $(,)?
    ) => {
        paste::paste! {
            #[derive(Clone, deserr::Deserr, schemars::JsonSchema, Connect, ChannelCount, SignalParams)]
            #[serde(rename_all = "camelCase")]
            #[deserr(rename_all = camelCase, deny_unknown_fields)]
            struct [<$Struct Params>] {
                /// input signal to shape (bipolar, ±5 V)
                input: crate::poly::PolySignal,
                /// shaper algorithm
                mode: $Mode,
                /// drive amount (-5..5, 0 = unity); higher pushes harder into the shaper
                #[signal(default = 0.0, range = (-5.0, 5.0))]
                #[deserr(default)]
                drive: Option<crate::poly::PolySignal>,
            }

            #[derive(Outputs, schemars::JsonSchema)]
            #[serde(rename_all = "camelCase")]
            struct [<$Struct Outputs>] {
                #[output("output", "shaped signal output", default, range = (-5.0, 5.0))]
                sample: crate::poly::PolyOutput,
            }

            $(#[$doc])*
            #[module(name = $name, args(input, mode, drive), has_init)]
            pub struct $Struct {
                outputs: [<$Struct Outputs>],
                state: crate::dsp::shape::shape_core::ShapeModuleState,
                channel_state: ::std::boxed::Box<[crate::dsp::shape::shape_core::ShapeChannel<$Shaper>]>,
                params: [<$Struct Params>],
            }

            impl $Struct {
                fn init(&mut self, sample_rate: f32) {
                    self.state.init(sample_rate);
                    <$Shaper as crate::dsp::shape::shape_core::Shaper>::prime();
                }

                fn update(&mut self, _sample_rate: f32) {
                    let channels = self.channel_count();
                    crate::dsp::shape::shape_core::run(
                        channels,
                        &self.params.input,
                        &self.params.drive,
                        self.params.mode,
                        &self.state,
                        &mut self.channel_state,
                        &mut self.outputs.sample,
                    );
                }
            }

            message_handlers!(impl $Struct {});
        }
    };

    // Single-algorithm modules (`digital`, `sine`): no `mode`, drive is positional 2.
    (
        $(#[$doc:meta])*
        name = $name:literal, ident = $Struct:ident, shaper = $Shaper:ty $(,)?
    ) => {
        paste::paste! {
            #[derive(Clone, deserr::Deserr, schemars::JsonSchema, Connect, ChannelCount, SignalParams)]
            #[serde(rename_all = "camelCase")]
            #[deserr(rename_all = camelCase, deny_unknown_fields)]
            struct [<$Struct Params>] {
                /// input signal to shape (bipolar, ±5 V)
                input: crate::poly::PolySignal,
                /// drive amount (-5..5, 0 = unity); higher pushes harder into the shaper
                #[signal(default = 0.0, range = (-5.0, 5.0))]
                #[deserr(default)]
                drive: Option<crate::poly::PolySignal>,
            }

            #[derive(Outputs, schemars::JsonSchema)]
            #[serde(rename_all = "camelCase")]
            struct [<$Struct Outputs>] {
                #[output("output", "shaped signal output", default, range = (-5.0, 5.0))]
                sample: crate::poly::PolyOutput,
            }

            $(#[$doc])*
            #[module(name = $name, args(input, drive), has_init)]
            pub struct $Struct {
                outputs: [<$Struct Outputs>],
                state: crate::dsp::shape::shape_core::ShapeModuleState,
                channel_state: ::std::boxed::Box<[crate::dsp::shape::shape_core::ShapeChannel<$Shaper>]>,
                params: [<$Struct Params>],
            }

            impl $Struct {
                fn init(&mut self, sample_rate: f32) {
                    self.state.init(sample_rate);
                    <$Shaper as crate::dsp::shape::shape_core::Shaper>::prime();
                }

                fn update(&mut self, _sample_rate: f32) {
                    let channels = self.channel_count();
                    crate::dsp::shape::shape_core::run(
                        channels,
                        &self.params.input,
                        &self.params.drive,
                        (),
                        &self.state,
                        &mut self.channel_state,
                        &mut self.outputs.sample,
                    );
                }
            }

            message_handlers!(impl $Struct {});
        }
    };
}

pub mod digital;
pub mod fold;
pub mod fuzz;
pub mod harmonic;
pub mod rectify;
pub mod saturate;
pub mod sine;
pub mod trigonometric;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    saturate::Saturate::install_constructor(map);
    harmonic::Harmonic::install_constructor(map);
    rectify::Rectify::install_constructor(map);
    fold::Fold::install_constructor(map);
    fuzz::Fuzz::install_constructor(map);
    trigonometric::Trigonometric::install_constructor(map);
    digital::Digital::install_constructor(map);
    sine::Sine::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    saturate::Saturate::install_params_deserializer(map);
    harmonic::Harmonic::install_params_deserializer(map);
    rectify::Rectify::install_params_deserializer(map);
    fold::Fold::install_params_deserializer(map);
    fuzz::Fuzz::install_params_deserializer(map);
    trigonometric::Trigonometric::install_params_deserializer(map);
    digital::Digital::install_params_deserializer(map);
    sine::Sine::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        saturate::Saturate::get_schema(),
        harmonic::Harmonic::get_schema(),
        rectify::Rectify::get_schema(),
        fold::Fold::get_schema(),
        fuzz::Fuzz::get_schema(),
        trigonometric::Trigonometric::get_schema(),
        digital::Digital::get_schema(),
        sine::Sine::get_schema(),
    ]
}
