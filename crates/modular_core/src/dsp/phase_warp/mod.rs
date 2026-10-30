//! `PhaseWarp` — a function-object param type that reshapes a 0–1 phase.
//!
//! Each variant carries `PolySignal` fields for its dynamic parameters; the consuming
//! module resolves them via `connect()`, then per sample on the audio thread reads them
//! once per channel with [`PhaseWarp::resolve`] and warps each phase with
//! [`PhaseWarp::evaluate`]. The DSL builds these with the `$phase.*` helpers.

pub mod shapes;

use std::borrow::Cow;
use std::result::Result as StdResult;

use deserr::{DeserializeError, ErrorKind, IntoValue, Map as DeserrMap, ValuePointerRef};
use schemars::JsonSchema;
use serde::Serialize;

use crate::poly::PolySignal;
use crate::types::SwapGlide;

/// Most `feedback` stages one warp may contain; each needs its own slot in
/// [`PhaseWarpState`].
pub const MAX_FEEDBACK_STAGES: usize = 4;

/// Most stages with a signal one warp may contain; each needs its own slot in
/// [`PhaseWarpGlides`] and [`ResolvedWarp`].
pub const MAX_SIGNAL_STAGES: usize = 8;

/// Runtime state a [`PhaseWarp`] carries between samples: one lowpass per
/// `feedback` stage, assigned in evaluation order. Every phase the warp shapes
/// independently (a channel, an oscillator voice) needs its own copy.
#[derive(Clone, Copy, Debug, Default)]
pub struct PhaseWarpState {
    feedback: [f32; MAX_FEEDBACK_STAGES],
}

/// Which kind of stage owns a glide slot. A slot handed to a different kind
/// (the warp's shape changed in a patch edit) restarts on its new value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum StageKind {
    #[default]
    Unused,
    Mirror,
    Bend,
    Sync,
    Fold,
    Pwm,
    Pulsar,
    Crush,
    Feedback,
}

impl StageKind {
    /// The stage's shape parameter for a knob value. Every amount is a 0–5
    /// knob (bend: -5..5).
    #[inline]
    fn prepare(self, value: f32) -> f32 {
        match self {
            StageKind::Unused => value,
            StageKind::Mirror
            | StageKind::Bend
            | StageKind::Sync
            | StageKind::Fold
            | StageKind::Pwm => value / 5.0,
            StageKind::Pulsar => shapes::pulsar_amount(value),
            StageKind::Crush => shapes::crush_amount(value),
            StageKind::Feedback => shapes::feedback_amount(value),
        }
    }
}

/// One [`SwapGlide`] per signal stage, assigned in evaluation order, so warp
/// amounts glide across patch edits like any other param. One copy per
/// channel: the values are per channel, not per voice.
#[derive(Clone, Copy, Debug, Default)]
pub struct PhaseWarpGlides {
    glides: [SwapGlide; MAX_SIGNAL_STAGES],
    kinds: [StageKind; MAX_SIGNAL_STAGES],
}

/// Every stage's shape parameter for one channel and sample, read, glided and
/// prepared by [`PhaseWarp::resolve`], in evaluation order.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResolvedWarp {
    values: [f32; MAX_SIGNAL_STAGES],
}

/// A parameterized phase warp, used by modules that read a phase (`$wavetable`).
///
/// JSON shape (serialized/deserialized):
/// ```json
/// { "type": "identity" }
/// { "type": "mirror",   "amount": <signal> }
/// { "type": "bend",     "amount": <signal> }
/// { "type": "sync",     "ratio":  <signal> }
/// { "type": "fold",     "amount": <signal> }
/// { "type": "pwm",      "width":  <signal> }
/// { "type": "pulsar",   "amount": <signal> }
/// { "type": "crush",    "amount": <signal> }
/// { "type": "feedback", "amount": <signal> }
/// { "type": "pipe",     "first":  <warp>, "second": <warp> }
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
    Pulsar {
        amount: PolySignal,
    },
    Crush {
        amount: PolySignal,
    },
    Feedback {
        amount: PolySignal,
    },
    /// Compose two warps: output phase of `first` becomes input phase of `second`.
    Pipe {
        first: Box<PhaseWarp>,
        second: Box<PhaseWarp>,
    },
}

impl PhaseWarp {
    /// The stage's kind and signal; `None` for `identity` and `pipe`.
    fn stage(&self) -> Option<(StageKind, &PolySignal)> {
        match self {
            PhaseWarp::Identity | PhaseWarp::Pipe { .. } => None,
            PhaseWarp::Mirror { amount } => Some((StageKind::Mirror, amount)),
            PhaseWarp::Bend { amount } => Some((StageKind::Bend, amount)),
            PhaseWarp::Sync { ratio } => Some((StageKind::Sync, ratio)),
            PhaseWarp::Fold { amount } => Some((StageKind::Fold, amount)),
            PhaseWarp::Pwm { width } => Some((StageKind::Pwm, width)),
            PhaseWarp::Pulsar { amount } => Some((StageKind::Pulsar, amount)),
            PhaseWarp::Crush { amount } => Some((StageKind::Crush, amount)),
            PhaseWarp::Feedback { amount } => Some((StageKind::Feedback, amount)),
        }
    }

    /// Read every stage's signal for `channel`, gliding values whose source
    /// changed in the last patch swap. Call once per channel and sample, then
    /// [`evaluate`](Self::evaluate) each phase the channel warps.
    ///
    /// Allocation-free — called per sample on the audio thread.
    #[inline]
    pub fn resolve(
        &self,
        channel: usize,
        sample_rate: f32,
        glides: &mut PhaseWarpGlides,
    ) -> ResolvedWarp {
        let mut resolved = ResolvedWarp::default();
        let mut slot = 0;
        self.resolve_from(channel, sample_rate, glides, &mut resolved, &mut slot);
        resolved
    }

    fn resolve_from(
        &self,
        channel: usize,
        sample_rate: f32,
        glides: &mut PhaseWarpGlides,
        resolved: &mut ResolvedWarp,
        slot: &mut usize,
    ) {
        if let PhaseWarp::Pipe { first, second } = self {
            first.resolve_from(channel, sample_rate, glides, resolved, slot);
            second.resolve_from(channel, sample_rate, glides, resolved, slot);
            return;
        }
        let Some((kind, signal)) = self.stage() else {
            return;
        };
        // Deserialization caps the stage count, so the slot is in range.
        let i = *slot;
        *slot += 1;
        if glides.kinds[i] != kind {
            glides.kinds[i] = kind;
            glides.glides[i] = SwapGlide::default();
        }
        let value = glides.glides[i].follow(
            signal.get_value(channel),
            signal.glide_mark(channel),
            sample_rate,
        );
        resolved.values[i] = kind.prepare(value);
    }

    /// Warp phase `x` with the stage values `resolved` for its channel.
    /// `freq_norm` is the oscillator frequency over the sample rate, used to
    /// antialias the pulsar and feedback stages; pass 0 when the pitch is
    /// unknown.
    ///
    /// Allocation-free — called per sample on the audio thread.
    #[inline]
    pub fn evaluate(
        &self,
        x: f32,
        resolved: &ResolvedWarp,
        freq_norm: f32,
        state: &mut PhaseWarpState,
    ) -> f32 {
        let (mut value_slot, mut feedback_slot) = (0, 0);
        self.evaluate_from(
            x,
            resolved,
            freq_norm,
            state,
            &mut value_slot,
            &mut feedback_slot,
        )
    }

    fn evaluate_from(
        &self,
        x: f32,
        resolved: &ResolvedWarp,
        freq_norm: f32,
        state: &mut PhaseWarpState,
        value_slot: &mut usize,
        feedback_slot: &mut usize,
    ) -> f32 {
        if let PhaseWarp::Pipe { first, second } = self {
            let y = first.evaluate_from(x, resolved, freq_norm, state, value_slot, feedback_slot);
            return second.evaluate_from(y, resolved, freq_norm, state, value_slot, feedback_slot);
        }
        let Some((kind, _)) = self.stage() else {
            return x;
        };
        let v = resolved.values[*value_slot];
        *value_slot += 1;
        match kind {
            StageKind::Unused => x,
            StageKind::Mirror => shapes::mirror(x, v),
            StageKind::Bend => shapes::bend(x, v),
            StageKind::Sync => shapes::sync(x, v),
            StageKind::Fold => shapes::fold(x, v),
            StageKind::Pwm => shapes::pwm(x, v),
            StageKind::Pulsar => shapes::pulsar(x, v, freq_norm),
            StageKind::Crush => shapes::crush(x, v),
            StageKind::Feedback => {
                let lp = &mut state.feedback[*feedback_slot];
                *feedback_slot += 1;
                shapes::feedback(x, v, freq_norm, lp)
            }
        }
    }

    pub fn channels(&self) -> usize {
        match self {
            PhaseWarp::Pipe { first, second } => first.channels().max(second.channels()),
            _ => self.stage().map_or(0, |(_, signal)| signal.channels()),
        }
    }

    /// The number of `feedback` stages and of stages with a signal.
    fn stage_counts(&self) -> (usize, usize) {
        match self {
            PhaseWarp::Pipe { first, second } => {
                let (a, b) = (first.stage_counts(), second.stage_counts());
                (a.0 + b.0, a.1 + b.1)
            }
            _ => match self.stage() {
                Some((StageKind::Feedback, _)) => (1, 1),
                Some(_) => (0, 1),
                None => (0, 0),
            },
        }
    }
}

// --- Serde Serialize (internally-tagged on "type") ---

impl<'a> From<&'a PhaseWarp> for PhaseWarpSerdeRef<'a> {
    fn from(t: &'a PhaseWarp) -> Self {
        match t {
            PhaseWarp::Identity => PhaseWarpSerdeRef::Identity,
            PhaseWarp::Mirror { amount } => PhaseWarpSerdeRef::Mirror { amount },
            PhaseWarp::Bend { amount } => PhaseWarpSerdeRef::Bend { amount },
            PhaseWarp::Sync { ratio } => PhaseWarpSerdeRef::Sync { ratio },
            PhaseWarp::Fold { amount } => PhaseWarpSerdeRef::Fold { amount },
            PhaseWarp::Pwm { width } => PhaseWarpSerdeRef::Pwm { width },
            PhaseWarp::Pulsar { amount } => PhaseWarpSerdeRef::Pulsar { amount },
            PhaseWarp::Crush { amount } => PhaseWarpSerdeRef::Crush { amount },
            PhaseWarp::Feedback { amount } => PhaseWarpSerdeRef::Feedback { amount },
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
    Pulsar {
        amount: &'a PolySignal,
    },
    Crush {
        amount: &'a PolySignal,
    },
    Feedback {
        amount: &'a PolySignal,
    },
    Pipe {
        first: Box<PhaseWarpSerdeRef<'a>>,
        second: Box<PhaseWarpSerdeRef<'a>>,
    },
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
        let unexpected = |msg: String| {
            deserr::take_cf_content(E::error::<V>(None, ErrorKind::Unexpected { msg }, location))
        };

        let mut map = match value {
            deserr::Value::Map(m) => m,
            _ => {
                return Err(unexpected(
                    "a phase warp must be an object with a \"type\" field".to_string(),
                ));
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

        fn take_field<E2: DeserializeError, V2: IntoValue>(
            map: &mut impl DeserrMap<Value = V2>,
            field: &'static str,
            location: ValuePointerRef<'_>,
        ) -> StdResult<V2, E2> {
            map.remove(field).ok_or_else(|| {
                deserr::take_cf_content(E2::error::<V2>(
                    None,
                    ErrorKind::MissingField { field },
                    location,
                ))
            })
        }

        fn take_signal<E2: DeserializeError, V2: IntoValue>(
            map: &mut impl DeserrMap<Value = V2>,
            field: &'static str,
            location: ValuePointerRef<'_>,
        ) -> StdResult<PolySignal, E2> {
            let raw = take_field::<E2, V2>(map, field, location)?;
            PolySignal::deserialize_from_value(raw.into_value(), location)
        }

        match type_str.as_str() {
            "identity" => Ok(PhaseWarp::Identity),
            "mirror" => Ok(PhaseWarp::Mirror {
                amount: take_signal::<E, V>(&mut map, "amount", location)?,
            }),
            "bend" => Ok(PhaseWarp::Bend {
                amount: take_signal::<E, V>(&mut map, "amount", location)?,
            }),
            "sync" => Ok(PhaseWarp::Sync {
                ratio: take_signal::<E, V>(&mut map, "ratio", location)?,
            }),
            "fold" => Ok(PhaseWarp::Fold {
                amount: take_signal::<E, V>(&mut map, "amount", location)?,
            }),
            "pwm" => Ok(PhaseWarp::Pwm {
                width: take_signal::<E, V>(&mut map, "width", location)?,
            }),
            "pulsar" => Ok(PhaseWarp::Pulsar {
                amount: take_signal::<E, V>(&mut map, "amount", location)?,
            }),
            "crush" => Ok(PhaseWarp::Crush {
                amount: take_signal::<E, V>(&mut map, "amount", location)?,
            }),
            "feedback" => Ok(PhaseWarp::Feedback {
                amount: take_signal::<E, V>(&mut map, "amount", location)?,
            }),
            "pipe" => {
                let first_raw = take_field::<E, V>(&mut map, "first", location)?;
                let second_raw = take_field::<E, V>(&mut map, "second", location)?;
                let first = PhaseWarp::deserialize_from_value(first_raw.into_value(), location)?;
                let second = PhaseWarp::deserialize_from_value(second_raw.into_value(), location)?;
                let pipe = PhaseWarp::Pipe {
                    first: Box::new(first),
                    second: Box::new(second),
                };
                // Every composition is a pipe, so checking each one bounds the whole warp.
                let (feedback, signal) = pipe.stage_counts();
                if feedback > MAX_FEEDBACK_STAGES {
                    return Err(unexpected(format!(
                        "a phase warp can contain at most {MAX_FEEDBACK_STAGES} feedback stages"
                    )));
                }
                if signal > MAX_SIGNAL_STAGES {
                    return Err(unexpected(format!(
                        "a phase warp can contain at most {MAX_SIGNAL_STAGES} stages"
                    )));
                }
                Ok(pipe)
            }
            other => Err(unexpected(format!("unknown phase warp type: {other}"))),
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
            Pulsar {
                amount: PolySignal,
            },
            Crush {
                amount: PolySignal,
            },
            Feedback {
                amount: PolySignal,
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
