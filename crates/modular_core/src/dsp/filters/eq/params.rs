//! Section-list params for `$unstable.eq`, their validation, and the Q mapping.

use deserr::{DeserializeError, Deserr, ErrorKind, IntoValue, Sequence, ValuePointerRef};
use schemars::JsonSchema;

use crate::param_errors::ModuleParamErrors;
use crate::poly::{MonoSignal, PORT_MAX_CHANNELS, PolySignal};

pub const MAX_EQ_SECTIONS: usize = 8;
pub const DEFAULT_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;
pub const MIN_Q: f32 = 0.1;
pub const MAX_Q: f32 = 20.0;

/// Filter shape of one EQ section.
#[derive(Clone, Copy, Debug, Default, Deserr, JsonSchema, PartialEq, Eq, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase)]
pub enum EqSectionType {
    /// Bell boost/cut around freq.
    #[default]
    Peak,
    /// Shelving boost/cut below freq.
    LowShelf,
    /// Shelving boost/cut above freq.
    HighShelf,
    /// 12 dB/oct lowpass at freq (gain ignored).
    Lowpass,
    /// 12 dB/oct highpass at freq (gain ignored).
    Highpass,
    /// Band-reject at freq (gain ignored).
    Notch,
}

/// One EQ section. `freq`, `gain`, and `q` accept modulation; a polyphonic
/// cable is summed to mono.
#[derive(Clone, Deserr, JsonSchema, Connect)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields)]
pub struct EqSection {
    /// section type (default peak)
    #[serde(rename = "type", default)]
    #[deserr(rename = "type", default)]
    pub r#type: EqSectionType,
    /// center/corner frequency in V/Oct (0 V = C4)
    pub freq: MonoSignal,
    /// boost/cut in dB (-24 to 24; peak and shelf types only, default 0)
    #[deserr(default)]
    pub gain: Option<MonoSignal>,
    /// bandwidth Q (0.1-20, default 0.707); on shelves, below 0.707 gentles the
    /// slope and above it adds a resonant bump at the corner
    #[deserr(default)]
    pub q: Option<MonoSignal>,
}

/// The section chain: one list shared by every channel, or one list per
/// channel (channel `c` uses list `c % len`).
#[derive(Clone, Connect)]
pub enum EqSections {
    Shared(Vec<EqSection>),
    PerChannel(Vec<Vec<EqSection>>),
}

impl Default for EqSections {
    fn default() -> Self {
        EqSections::Shared(Vec::new())
    }
}

/// A sequence whose first element is itself a sequence is the per-channel
/// form; anything else is one shared list.
impl<E: DeserializeError> Deserr<E> for EqSections {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        let deserr::Value::Sequence(seq) = value else {
            return Err(deserr::take_cf_content(E::error::<V>(
                None,
                ErrorKind::Unexpected {
                    msg: "sections must be an array of sections, or an array of section arrays (one per channel)"
                        .to_string(),
                },
                location,
            )));
        };
        let items: Vec<deserr::Value<V>> = seq.into_iter().map(IntoValue::into_value).collect();
        if matches!(items.first(), Some(deserr::Value::Sequence(_))) {
            let mut lists = Vec::with_capacity(items.len());
            for (i, item) in items.into_iter().enumerate() {
                lists.push(<Vec<EqSection> as Deserr<E>>::deserialize_from_value(
                    item,
                    location.push_index(i),
                )?);
            }
            Ok(EqSections::PerChannel(lists))
        } else {
            let mut list = Vec::with_capacity(items.len());
            for (i, item) in items.into_iter().enumerate() {
                list.push(<EqSection as Deserr<E>>::deserialize_from_value(
                    item,
                    location.push_index(i),
                )?);
            }
            Ok(EqSections::Shared(list))
        }
    }
}

impl JsonSchema for EqSections {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("EqSections")
    }

    fn json_schema(r#gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        #[derive(JsonSchema)]
        #[serde(untagged)]
        #[allow(dead_code)]
        enum EqSectionsSchema {
            Shared(Vec<EqSection>),
            PerChannel(Vec<Vec<EqSection>>),
        }
        EqSectionsSchema::json_schema(r#gen)
    }
}

#[derive(Clone, Deserr, JsonSchema, Connect, ChannelCount, SignalParams)]
#[serde(rename_all = "camelCase")]
#[deserr(rename_all = camelCase, deny_unknown_fields, validate = validate_eq_params -> ModuleParamErrors)]
pub struct ParametricEqParams {
    /// signal input
    pub input: PolySignal,
    /// up to 8 filter sections applied in series, or one such list per channel
    /// (default none: the input passes through)
    #[serde(default)]
    #[deserr(default)]
    pub sections: EqSections,
    /// narrow peak sections as their boost or cut grows (default false)
    #[serde(default)]
    #[deserr(default)]
    pub adaptive_q: bool,
    /// show the interactive EQ panel under the call in the editor (default
    /// false); the engine ignores it
    #[serde(default)]
    #[deserr(default)]
    pub ui: bool,
}

/// The Q a section's filter runs at.
///
/// Below 0.707, a shelf's `q` maps onto the Audio EQ Cookbook shelf slope
/// `S = 2q²` for a gentler slope (near 0 dB the result equals `q`). From 0.707,
/// where that slope is the steepest without overshoot at any gain, `q` passes
/// through and higher values add a resonant overshoot at the corner. With
/// adaptive Q, a peak narrows by about 2x per 12 dB of boost or cut.
pub fn effective_q(kind: EqSectionType, q: f32, gain_db: f32, adaptive: bool) -> f32 {
    match kind {
        EqSectionType::LowShelf | EqSectionType::HighShelf if q < DEFAULT_Q => {
            let a = 10f32.powf(gain_db / 40.0);
            let s = 2.0 * q * q;
            // At S = 1 the radicand is 2 for any gain, so the mapping meets the
            // pass-through at exactly 0.707.
            1.0 / ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt()
        }
        EqSectionType::Peak if adaptive => (q * 10f32.powf(gain_db.abs() / 40.0)).min(MAX_Q),
        _ => q,
    }
}

fn validate_eq_params(
    params: ParametricEqParams,
    _location: deserr::ValuePointerRef,
) -> Result<ParametricEqParams, ModuleParamErrors> {
    let mut err = ModuleParamErrors::default();
    let check = |list: &[EqSection], path: String, err: &mut ModuleParamErrors| {
        if list.len() > MAX_EQ_SECTIONS {
            err.add(
                path,
                format!("must have at most {MAX_EQ_SECTIONS} sections"),
            );
        }
    };
    match &params.sections {
        EqSections::Shared(list) => check(list, "sections".to_string(), &mut err),
        EqSections::PerChannel(lists) => {
            if lists.len() > PORT_MAX_CHANNELS {
                err.add(
                    "sections".to_string(),
                    format!("must have at most {PORT_MAX_CHANNELS} channel lists"),
                );
            }
            for (i, list) in lists.iter().enumerate() {
                check(list, format!("sections[{i}]"), &mut err);
            }
        }
    }
    if err.is_empty() { Ok(params) } else { Err(err) }
}

/// Channels follow the input, widened to the number of per-channel lists.
/// Section params are mono and never add channels.
pub fn eq_derive_channel_count(params: &ParametricEqParams) -> usize {
    let input = PolySignal::max_channels(&[&params.input]) as usize;
    let lists = match &params.sections {
        EqSections::Shared(_) => 1,
        EqSections::PerChannel(lists) => lists.len(),
    };
    input.max(lists).clamp(1, PORT_MAX_CHANNELS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Signal;

    fn section() -> EqSection {
        EqSection {
            r#type: EqSectionType::Peak,
            freq: PolySignal::mono(Signal::Volts(0.0)).into(),
            gain: None,
            q: None,
        }
    }

    fn params(sections: EqSections) -> ParametricEqParams {
        ParametricEqParams {
            input: PolySignal::mono(Signal::Volts(0.0)),
            sections,
            adaptive_q: false,
            ui: false,
        }
    }

    #[test]
    fn shelf_q_follows_cookbook_slope() {
        for kind in [EqSectionType::LowShelf, EqSectionType::HighShelf] {
            // At 0 dB the slope mapping reproduces q itself.
            for q in [0.1, 0.3, 0.5, DEFAULT_Q] {
                assert!((effective_q(kind, q, 0.0, false) - q).abs() < 1e-5);
            }
            for gain in [-24.0, -6.0, 6.0, 24.0] {
                // The slope mapping meets the pass-through at 0.707.
                let just_below = effective_q(kind, DEFAULT_Q - 1e-4, gain, false);
                assert!(
                    (just_below - DEFAULT_Q).abs() < 1e-3,
                    "gain {gain}: {just_below}"
                );
                for q in [DEFAULT_Q, 2.0, MAX_Q] {
                    assert_eq!(effective_q(kind, q, gain, false), q);
                }
                // Gentler slopes widen further as the shelf gain grows.
                assert!(effective_q(kind, 0.3, gain, false) < 0.3);
                assert_eq!(
                    effective_q(kind, 0.3, gain, true),
                    effective_q(kind, 0.3, gain, false)
                );
            }
        }
    }

    #[test]
    fn validate_rejects_oversized_sections() {
        let origin = deserr::ValuePointerRef::Origin;
        let list = |n: usize| (0..n).map(|_| section()).collect::<Vec<_>>();
        let oversized = EqSections::Shared(list(MAX_EQ_SECTIONS + 1));
        assert!(validate_eq_params(params(oversized), origin).is_err());
        for n in [0, 1, MAX_EQ_SECTIONS] {
            assert!(validate_eq_params(params(EqSections::Shared(list(n))), origin).is_ok());
        }
        let per = EqSections::PerChannel(vec![list(2), list(MAX_EQ_SECTIONS + 1)]);
        assert!(validate_eq_params(params(per), origin).is_err());
        let per = EqSections::PerChannel(vec![list(0), list(MAX_EQ_SECTIONS)]);
        assert!(validate_eq_params(params(per), origin).is_ok());
    }
}
