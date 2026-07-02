//! Surge XT filter ports — the `$unstable.filter.*` family.
//!
//! Each module ports one Surge `FilterType` group (passband/slope/drive enums select
//! the subtype) onto a shared scalar engine ([`filter_core`]) + coefficient maker
//! ([`coeffs`]). The `$unstable.filter.` DSL prefix lives only in each
//! `#[module(name = …)]` string.
//!
//! Ported from https://github.com/surge-synthesizer/sst-filters (GPL-3.0).

use std::collections::HashMap;

use crate::params::ParamsDeserializer;
use crate::types::{Module, ModuleSchema, SampleableConstructor};

/// Stamp a `$unstable.filter.*` module from its [`filter_core::Filter`] kernel.
///
/// Every module shares the same shape: `input` / `cutoff` / `resonance` (the three
/// positional args), zero or more config-object enum params, one output, and the
/// [`filter_core::run`] per-sample body. This emits that boilerplate so each module
/// only writes the parts that differ — its enums and the `mode` they select.
macro_rules! filter_module {
    (
        $(#[$doc:meta])*
        name = $name:literal, ident = $Struct:ident, kernel = $Kernel:ty,
        output_doc = $output_doc:literal,
        params = { $( $(#[$fdoc:meta])* $field:ident : $ftype:ty ),* $(,)? },
        mode = |$p:ident| $mode:expr $(,)?
    ) => {
        paste::paste! {
            #[derive(Clone, deserr::Deserr, schemars::JsonSchema, Connect, ChannelCount, SignalParams)]
            #[serde(rename_all = "camelCase")]
            #[deserr(rename_all = camelCase, deny_unknown_fields)]
            struct [<$Struct Params>] {
                /// signal input (bipolar, ±5 V)
                input: crate::poly::PolySignal,
                /// cutoff frequency in V/Oct (0V = C4)
                #[signal(type = pitch, default = 0.0, range = (-5.0, 5.0))]
                cutoff: crate::poly::PolySignal,
                /// filter resonance (0–5); high values approach self-oscillation
                #[signal(type = control, default = 0.0, range = (0.0, 5.0))]
                #[deserr(default)]
                resonance: Option<crate::poly::PolySignal>,
                $(
                    $(#[$fdoc])*
                    #[serde(default)]
                    #[deserr(default)]
                    $field: $ftype,
                )*
            }

            #[derive(Outputs, schemars::JsonSchema)]
            #[serde(rename_all = "camelCase")]
            struct [<$Struct Outputs>] {
                #[output("output", $output_doc, default, range = (-5.0, 5.0))]
                sample: crate::poly::PolyOutput,
            }

            $(#[$doc])*
            #[module(name = $name, args(input, cutoff, resonance), patch_update)]
            pub struct $Struct {
                outputs: [<$Struct Outputs>],
                state: crate::dsp::filters::surge::filter_core::FilterModuleState<
                    <$Kernel as crate::dsp::filters::surge::filter_core::Filter>::Mode,
                >,
                channel_state: ::std::boxed::Box<[crate::dsp::filters::surge::filter_core::FilterChannel<
                    <$Kernel as crate::dsp::filters::surge::filter_core::Filter>::Extra,
                >]>,
                params: [<$Struct Params>],
            }

            impl $Struct {
                #[inline]
                fn mode(&self) -> <$Kernel as crate::dsp::filters::surge::filter_core::Filter>::Mode {
                    let $p = &self.params;
                    $mode
                }

                fn update(&mut self, sample_rate: f32) {
                    let mode = self.mode();
                    let channels = self.channel_count();
                    crate::dsp::filters::surge::filter_core::run::<$Kernel>(
                        channels,
                        &self.params.input,
                        &self.params.cutoff,
                        &self.params.resonance,
                        mode,
                        sample_rate,
                        &mut self.state,
                        &mut self.channel_state,
                        &mut self.outputs.sample,
                    );
                }
            }

            impl crate::types::PatchUpdateHandler for $Struct {
                fn on_patch_update(&mut self) {
                    let mode = self.mode();
                    crate::dsp::filters::surge::filter_core::on_patch_update::<$Kernel>(
                        &mut self.state,
                        &mut self.channel_state,
                        mode,
                    );
                }
            }

            message_handlers!(impl $Struct {});
        }
    };
}

pub mod ap;
pub mod biquad;
pub mod bp;
pub mod coeffs;
pub mod comb;
pub mod cutoff_warp;
pub mod cytomic;
pub mod fastmath;
pub mod filter_core;
pub mod hp;
pub mod k35;
pub mod ladders;
pub mod lp;
pub mod notch;
pub mod obxd2;
pub mod obxd4;
pub mod res_warp;
pub mod sah;
pub mod sinc;
pub mod tripole;
pub mod vintage;

pub fn install_constructors(map: &mut HashMap<String, SampleableConstructor>) {
    lp::LpFilter::install_constructor(map);
    hp::HpFilter::install_constructor(map);
    bp::BpFilter::install_constructor(map);
    notch::NotchFilter::install_constructor(map);
    ap::ApFilter::install_constructor(map);
    ladders::LegacyLadderFilter::install_constructor(map);
    ladders::DiodeLadderFilter::install_constructor(map);
    vintage::VintageLadderFilter::install_constructor(map);
    k35::K35LpFilter::install_constructor(map);
    k35::K35HpFilter::install_constructor(map);
    obxd2::Obxd2LpFilter::install_constructor(map);
    obxd2::Obxd2BpFilter::install_constructor(map);
    obxd2::Obxd2HpFilter::install_constructor(map);
    obxd2::Obxd2NotchFilter::install_constructor(map);
    obxd4::Obxd4Filter::install_constructor(map);
    obxd4::XpanderFilter::install_constructor(map);
    cutoff_warp::CutoffWarpLpFilter::install_constructor(map);
    cutoff_warp::CutoffWarpHpFilter::install_constructor(map);
    cutoff_warp::CutoffWarpBpFilter::install_constructor(map);
    cutoff_warp::CutoffWarpNotchFilter::install_constructor(map);
    cutoff_warp::CutoffWarpApFilter::install_constructor(map);
    res_warp::ResWarpLpFilter::install_constructor(map);
    res_warp::ResWarpHpFilter::install_constructor(map);
    res_warp::ResWarpBpFilter::install_constructor(map);
    res_warp::ResWarpNotchFilter::install_constructor(map);
    res_warp::ResWarpApFilter::install_constructor(map);
    tripole::TriPoleFilter::install_constructor(map);
    cytomic::FastSvfFilter::install_constructor(map);
    sah::SahFilter::install_constructor(map);
    comb::CombFilter::install_constructor(map);
}

pub fn install_params_deserializers(map: &mut HashMap<String, ParamsDeserializer>) {
    lp::LpFilter::install_params_deserializer(map);
    hp::HpFilter::install_params_deserializer(map);
    bp::BpFilter::install_params_deserializer(map);
    notch::NotchFilter::install_params_deserializer(map);
    ap::ApFilter::install_params_deserializer(map);
    ladders::LegacyLadderFilter::install_params_deserializer(map);
    ladders::DiodeLadderFilter::install_params_deserializer(map);
    vintage::VintageLadderFilter::install_params_deserializer(map);
    k35::K35LpFilter::install_params_deserializer(map);
    k35::K35HpFilter::install_params_deserializer(map);
    obxd2::Obxd2LpFilter::install_params_deserializer(map);
    obxd2::Obxd2BpFilter::install_params_deserializer(map);
    obxd2::Obxd2HpFilter::install_params_deserializer(map);
    obxd2::Obxd2NotchFilter::install_params_deserializer(map);
    obxd4::Obxd4Filter::install_params_deserializer(map);
    obxd4::XpanderFilter::install_params_deserializer(map);
    cutoff_warp::CutoffWarpLpFilter::install_params_deserializer(map);
    cutoff_warp::CutoffWarpHpFilter::install_params_deserializer(map);
    cutoff_warp::CutoffWarpBpFilter::install_params_deserializer(map);
    cutoff_warp::CutoffWarpNotchFilter::install_params_deserializer(map);
    cutoff_warp::CutoffWarpApFilter::install_params_deserializer(map);
    res_warp::ResWarpLpFilter::install_params_deserializer(map);
    res_warp::ResWarpHpFilter::install_params_deserializer(map);
    res_warp::ResWarpBpFilter::install_params_deserializer(map);
    res_warp::ResWarpNotchFilter::install_params_deserializer(map);
    res_warp::ResWarpApFilter::install_params_deserializer(map);
    tripole::TriPoleFilter::install_params_deserializer(map);
    cytomic::FastSvfFilter::install_params_deserializer(map);
    sah::SahFilter::install_params_deserializer(map);
    comb::CombFilter::install_params_deserializer(map);
}

pub fn schemas() -> Vec<ModuleSchema> {
    vec![
        lp::LpFilter::get_schema(),
        hp::HpFilter::get_schema(),
        bp::BpFilter::get_schema(),
        notch::NotchFilter::get_schema(),
        ap::ApFilter::get_schema(),
        ladders::LegacyLadderFilter::get_schema(),
        ladders::DiodeLadderFilter::get_schema(),
        vintage::VintageLadderFilter::get_schema(),
        k35::K35LpFilter::get_schema(),
        k35::K35HpFilter::get_schema(),
        obxd2::Obxd2LpFilter::get_schema(),
        obxd2::Obxd2BpFilter::get_schema(),
        obxd2::Obxd2HpFilter::get_schema(),
        obxd2::Obxd2NotchFilter::get_schema(),
        obxd4::Obxd4Filter::get_schema(),
        obxd4::XpanderFilter::get_schema(),
        cutoff_warp::CutoffWarpLpFilter::get_schema(),
        cutoff_warp::CutoffWarpHpFilter::get_schema(),
        cutoff_warp::CutoffWarpBpFilter::get_schema(),
        cutoff_warp::CutoffWarpNotchFilter::get_schema(),
        cutoff_warp::CutoffWarpApFilter::get_schema(),
        res_warp::ResWarpLpFilter::get_schema(),
        res_warp::ResWarpHpFilter::get_schema(),
        res_warp::ResWarpBpFilter::get_schema(),
        res_warp::ResWarpNotchFilter::get_schema(),
        res_warp::ResWarpApFilter::get_schema(),
        tripole::TriPoleFilter::get_schema(),
        cytomic::FastSvfFilter::get_schema(),
        sah::SahFilter::get_schema(),
        comb::CombFilter::get_schema(),
    ]
}
