//! Slice selector param for `$sampler`.

use std::borrow::Cow;

use deserr::{DeserializeError, ErrorKind, IntoValue, Sequence, ValueKind, ValuePointerRef};
use schemars::JsonSchema;

use crate::{
    poly::{MonoSignal, PORT_MAX_CHANNELS, PolySignal},
    types::Signal,
};

/// Slice points plus the mono signal that picks a slice at each gate onset.
///
/// Accepts either a bare Mono<Signal> (shorthand for `[[0], Mono<Signal>]` — one slice
/// spanning the whole file) or a `[points, Mono<Signal>]` tuple. Points are
/// fractions of the total WAV length; slice `i` runs from `points[i]` to the
/// next point (or the end of the file for the last one). Points need not be
/// sorted — a point at or past its successor yields a zero-length slice.
#[derive(Clone, Debug, Connect)]
pub(crate) struct SliceParam {
    /// Non-empty; every point finite and in [0, 1]. Enforced at deserialize.
    pub points: Vec<f64>,
    pub signal: MonoSignal,
}

impl SliceParam {
    fn from_mono(signal: MonoSignal) -> Self {
        Self {
            points: vec![0.0],
            signal,
        }
    }
}

// Hand-written: the tuple form and the bare-signal form are distinguished by
// whether the first array element is itself an array (a Signal is never a
// JSON array, so this is unambiguous).
impl<E: DeserializeError> deserr::Deserr<E> for SliceParam {
    fn deserialize_from_value<V: IntoValue>(
        value: deserr::Value<V>,
        location: ValuePointerRef<'_>,
    ) -> Result<Self, E> {
        let unexpected = |msg: String, loc: ValuePointerRef<'_>| {
            deserr::take_cf_content(E::error::<V>(None, ErrorKind::Unexpected { msg }, loc))
        };
        match value {
            deserr::Value::Sequence(seq) => {
                let len = seq.len();
                let mut iter = seq.into_iter();
                let Some(first) = iter.next() else {
                    return Err(unexpected(
                        "slice cannot be an empty array; pass a Mono<Signal> or [points, Mono<Signal>]"
                            .to_string(),
                        location,
                    ));
                };
                if first.kind() == ValueKind::Sequence {
                    // Tuple form: [points, signal]
                    if len != 2 {
                        return Err(unexpected(
                            format!(
                                "slice tuple must be [points, Mono<Signal>] (exactly 2 elements), got {len}"
                            ),
                            location,
                        ));
                    }
                    let points_loc = location.push_index(0);
                    let points = <Vec<f64> as deserr::Deserr<E>>::deserialize_from_value(
                        first.into_value(),
                        points_loc,
                    )?;
                    if points.is_empty() {
                        return Err(unexpected(
                            "slice points must contain at least one point".to_string(),
                            points_loc,
                        ));
                    }
                    for &p in &points {
                        if !p.is_finite() || !(0.0..=1.0).contains(&p) {
                            return Err(unexpected(
                                format!("slice points must be finite fractions in [0, 1], got {p}"),
                                points_loc,
                            ));
                        }
                    }
                    let signal = <MonoSignal as deserr::Deserr<E>>::deserialize_from_value(
                        iter.next().expect("length checked above").into_value(),
                        location.push_index(1),
                    )?;
                    Ok(Self { points, signal })
                } else {
                    // Bare poly-array shorthand ([sig, sig, ...] summed to
                    // mono). The sequence is consumed, so parse element-wise.
                    let mut signals = Vec::with_capacity(len);
                    signals.push(<Signal as deserr::Deserr<E>>::deserialize_from_value(
                        first.into_value(),
                        location.push_index(0),
                    )?);
                    for (i, item) in iter.enumerate() {
                        signals.push(<Signal as deserr::Deserr<E>>::deserialize_from_value(
                            item.into_value(),
                            location.push_index(i + 1),
                        )?);
                    }
                    if signals.len() > PORT_MAX_CHANNELS {
                        return Err(unexpected(
                            format!("PolySignal cannot exceed {PORT_MAX_CHANNELS} channels"),
                            location,
                        ));
                    }
                    Ok(Self::from_mono(MonoSignal::from_poly(PolySignal::poly(
                        &signals,
                    ))))
                }
            }
            other => Ok(Self::from_mono(
                <MonoSignal as deserr::Deserr<E>>::deserialize_from_value(other, location)?,
            )),
        }
    }
}

impl JsonSchema for SliceParam {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("SamplerSlice")
    }

    // Inlined so the anyOf (with its MonoSignal $ref) sits directly on the
    // param property — patch validation scans property schemas for Signal
    // refs to know where cables can appear, and does not resolve $defs.
    fn inline_schema() -> bool {
        true
    }

    fn json_schema(r#gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mono = r#gen.subschema_for::<MonoSignal>();
        schemars::json_schema!({
            "anyOf": [
                (mono.clone()),
                {
                    "type": "array",
                    "prefixItems": [
                        { "type": "array", "items": { "type": "number" }, "minItems": 1 },
                        (mono)
                    ],
                    "minItems": 2,
                    "maxItems": 2
                }
            ]
        })
    }
}
