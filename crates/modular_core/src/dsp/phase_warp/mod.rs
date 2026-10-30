//! `PhaseWarp` — a function-object param type that reshapes a 0–1 phase.
//!
//! Each variant carries `PolySignal` fields for its dynamic parameters; the consuming
//! module resolves them via `connect()` and calls `warp.evaluate(x, channel)` per sample
//! on the audio thread. The DSL builds these with the `$phase.*` helpers.

pub mod shapes;

use std::borrow::Cow;
use std::result::Result as StdResult;

use deserr::{DeserializeError, ErrorKind, IntoValue, Map as DeserrMap, ValuePointerRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::poly::PolySignal;

/// A parameterized phase warp, used by modules that read a phase (`$wavetable`).
///
/// JSON shape (serialized/deserialized):
/// ```json
/// { "type": "identity" }
/// { "type": "mirror", "amount": <signal> }
/// { "type": "bend",   "amount": <signal> }
/// { "type": "sync",   "ratio":  <signal> }
/// { "type": "fold",   "amount": <signal> }
/// { "type": "pwm",    "width":  <signal> }
/// { "type": "pipe",   "first":  <warp>, "second": <warp> }
/// ```
#[derive(Clone, Debug, Connect)]
pub enum PhaseWarp {
    Identity,
    Mirror {
        amount: PolySignal,
    },
    Bend {
        amount: PolySignal,
    },
    Sync {
        ratio: PolySignal,
    },
    Fold {
        amount: PolySignal,
    },
    Pwm {
        width: PolySignal,
    },
    /// Compose two warps: output phase of `first` becomes input phase of `second`.
    Pipe {
        first: Box<PhaseWarp>,
        second: Box<PhaseWarp>,
    },
}

impl PhaseWarp {
    /// Evaluate the warp at position `x` for the given channel.
    ///
    /// Must be cheap and allocation-free — called per-sample on the audio thread.
    #[inline]
    pub fn evaluate(&self, x: f32, channel: usize) -> f32 {
        match self {
            PhaseWarp::Identity => x,
            PhaseWarp::Mirror { amount } => shapes::mirror(x, amount.get_value(channel)),
            PhaseWarp::Bend { amount } => shapes::bend(x, amount.get_value(channel)),
            PhaseWarp::Sync { ratio } => shapes::sync(x, ratio.get_value(channel)),
            PhaseWarp::Fold { amount } => shapes::fold(x, amount.get_value(channel)),
            PhaseWarp::Pwm { width } => shapes::pwm(x, width.get_value(channel)),
            PhaseWarp::Pipe { first, second } => {
                second.evaluate(first.evaluate(x, channel), channel)
            }
        }
    }

    pub fn channels(&self) -> usize {
        match self {
            PhaseWarp::Identity => 0,
            PhaseWarp::Mirror { amount } => amount.channels(),
            PhaseWarp::Bend { amount } => amount.channels(),
            PhaseWarp::Sync { ratio } => ratio.channels(),
            PhaseWarp::Fold { amount } => amount.channels(),
            PhaseWarp::Pwm { width } => width.channels(),
            PhaseWarp::Pipe { first, second } => first.channels().max(second.channels()),
        }
    }
}

// --- Serde Serialize / Deserialize (internally-tagged on "type") ---

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum PhaseWarpSerde {
    Identity,
    Mirror {
        amount: PolySignal,
    },
    Bend {
        amount: PolySignal,
    },
    Sync {
        ratio: PolySignal,
    },
    Fold {
        amount: PolySignal,
    },
    Pwm {
        width: PolySignal,
    },
    Pipe {
        first: Box<PhaseWarpSerde>,
        second: Box<PhaseWarpSerde>,
    },
}

impl From<PhaseWarpSerde> for PhaseWarp {
    fn from(s: PhaseWarpSerde) -> Self {
        match s {
            PhaseWarpSerde::Identity => PhaseWarp::Identity,
            PhaseWarpSerde::Mirror { amount } => PhaseWarp::Mirror { amount },
            PhaseWarpSerde::Bend { amount } => PhaseWarp::Bend { amount },
            PhaseWarpSerde::Sync { ratio } => PhaseWarp::Sync { ratio },
            PhaseWarpSerde::Fold { amount } => PhaseWarp::Fold { amount },
            PhaseWarpSerde::Pwm { width } => PhaseWarp::Pwm { width },
            PhaseWarpSerde::Pipe { first, second } => PhaseWarp::Pipe {
                first: Box::new(PhaseWarp::from(*first)),
                second: Box::new(PhaseWarp::from(*second)),
            },
        }
    }
}

impl<'a> From<&'a PhaseWarp> for PhaseWarpSerdeRef<'a> {
    fn from(t: &'a PhaseWarp) -> Self {
        match t {
            PhaseWarp::Identity => PhaseWarpSerdeRef::Identity,
            PhaseWarp::Mirror { amount } => PhaseWarpSerdeRef::Mirror { amount },
            PhaseWarp::Bend { amount } => PhaseWarpSerdeRef::Bend { amount },
            PhaseWarp::Sync { ratio } => PhaseWarpSerdeRef::Sync { ratio },
            PhaseWarp::Fold { amount } => PhaseWarpSerdeRef::Fold { amount },
            PhaseWarp::Pwm { width } => PhaseWarpSerdeRef::Pwm { width },
            PhaseWarp::Pipe { first, second } => PhaseWarpSerdeRef::Pipe {
                first: Box::new(PhaseWarpSerdeRef::from(first.as_ref())),
                second: Box::new(PhaseWarpSerdeRef::from(second.as_ref())),
            },
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum PhaseWarpSerdeRef<'a> {
    Identity,
    Mirror {
        amount: &'a PolySignal,
    },
    Bend {
        amount: &'a PolySignal,
    },
    Sync {
        ratio: &'a PolySignal,
    },
    Fold {
        amount: &'a PolySignal,
    },
    Pwm {
        width: &'a PolySignal,
    },
    Pipe {
        first: Box<PhaseWarpSerdeRef<'a>>,
        second: Box<PhaseWarpSerdeRef<'a>>,
    },
}

impl<'de> Deserialize<'de> for PhaseWarp {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        PhaseWarpSerde::deserialize(deserializer).map(PhaseWarp::from)
    }
}

impl Serialize for PhaseWarp {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        PhaseWarpSerdeRef::from(self).serialize(serializer)
    }
}

// --- deserr impl (mirrors the serde shape: object with "type" discriminant) ---

impl<E: DeserializeError> deserr::Deserr<E> for PhaseWarp {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> StdResult<Self, E> {
        let mut map = match value {
            deserr::Value::Map(m) => m,
            _ => {
                return Err(deserr::take_cf_content(E::error::<V>(
                    None,
                    ErrorKind::Unexpected {
                        msg: "a phase warp must be an object with a \"type\" field".to_string(),
                    },
                    location,
                )));
            }
        };

        let type_val = map.remove("type").and_then(|v| match v.into_value() {
            deserr::Value::String(s) => Some(s),
            _ => None,
        });

        let type_str = type_val.ok_or_else(|| {
            deserr::take_cf_content(E::error::<V>(
                None,
                ErrorKind::MissingField { field: "type" },
                location,
            ))
        })?;

        // Helper: pull a named PolySignal field out of the remaining map.
        fn take_signal<E2: DeserializeError, V2: IntoValue>(
            map: &mut impl DeserrMap<Value = V2>,
            field: &'static str,
            location: ValuePointerRef<'_>,
        ) -> StdResult<PolySignal, E2> {
            let raw = map.remove(field).ok_or_else(|| {
                deserr::take_cf_content(E2::error::<V2>(
                    None,
                    ErrorKind::MissingField { field },
                    location,
                ))
            })?;
            PolySignal::deserialize_from_value(raw.into_value(), location)
        }

        match type_str.as_str() {
            "identity" => Ok(PhaseWarp::Identity),
            "mirror" => {
                let amount = take_signal::<E, V>(&mut map, "amount", location)?;
                Ok(PhaseWarp::Mirror { amount })
            }
            "bend" => {
                let amount = take_signal::<E, V>(&mut map, "amount", location)?;
                Ok(PhaseWarp::Bend { amount })
            }
            "sync" => {
                let ratio = take_signal::<E, V>(&mut map, "ratio", location)?;
                Ok(PhaseWarp::Sync { ratio })
            }
            "fold" => {
                let amount = take_signal::<E, V>(&mut map, "amount", location)?;
                Ok(PhaseWarp::Fold { amount })
            }
            "pwm" => {
                let width = take_signal::<E, V>(&mut map, "width", location)?;
                Ok(PhaseWarp::Pwm { width })
            }
            "pipe" => {
                let first_raw = map.remove("first").ok_or_else(|| {
                    deserr::take_cf_content(E::error::<V>(
                        None,
                        ErrorKind::MissingField { field: "first" },
                        location,
                    ))
                })?;
                let second_raw = map.remove("second").ok_or_else(|| {
                    deserr::take_cf_content(E::error::<V>(
                        None,
                        ErrorKind::MissingField { field: "second" },
                        location,
                    ))
                })?;
                let first = PhaseWarp::deserialize_from_value(first_raw.into_value(), location)?;
                let second = PhaseWarp::deserialize_from_value(second_raw.into_value(), location)?;
                Ok(PhaseWarp::Pipe {
                    first: Box::new(first),
                    second: Box::new(second),
                })
            }
            other => Err(deserr::take_cf_content(E::error::<V>(
                None,
                ErrorKind::Unexpected {
                    msg: format!("unknown phase warp type: {}", other),
                },
                location,
            ))),
        }
    }
}

impl JsonSchema for PhaseWarp {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PhaseWarp")
    }

    fn json_schema(r#gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Schema matches the serialized form: internally-tagged on "type", camelCase.
        #[derive(JsonSchema)]
        #[serde(tag = "type", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum PhaseWarpSchema {
            Identity,
            Mirror {
                amount: PolySignal,
            },
            Bend {
                amount: PolySignal,
            },
            Sync {
                ratio: PolySignal,
            },
            Fold {
                amount: PolySignal,
            },
            Pwm {
                width: PolySignal,
            },
            Pipe {
                first: Box<PhaseWarp>,
                second: Box<PhaseWarp>,
            },
        }
        PhaseWarpSchema::json_schema(r#gen)
    }
}

#[cfg(test)]
mod tests;
