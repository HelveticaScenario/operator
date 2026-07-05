//! Reference checking for params that can point at other patch entities:
//! `Signal` cables and `buffer_ref` buffer references nested anywhere inside
//! a param's JSON value.

use super::ValidationError;
use modular_core::types::{ModuleSchema, ModuleSpec, Signal, WellKnownModule};
use schemars::Schema;
use std::collections::HashMap;

/// Extract the `properties` object from a schema node.
///
/// Returns a mapping from param name -> schema for that param.
/// If the schema doesn't look like an object schema with properties, returns empty.
pub(super) fn schema_properties(schema: &Schema) -> HashMap<String, Schema> {
    // schemars::Schema is a thin wrapper around a serde_json::Value (object/bool).
    // Properties live under "properties" in the common case; we also tolerate
    // older "schema.properties" shapes.
    let props = schema.as_object().and_then(|obj| {
        obj.get("properties")
            .and_then(|v| v.as_object())
            .or_else(|| {
                obj.get("schema")
                    .and_then(|s| s.as_object())
                    .and_then(|s| s.get("properties"))
                    .and_then(|v| v.as_object())
            })
    });

    props
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| {
                    let schema: Result<Schema, _> = v.clone().try_into();
                    schema.ok().map(|s| (k.clone(), s))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Returns true if `schema_node` describes (or contains) a `Signal` or `Buffer`.
///
/// Why we need this:
/// - Most params are plain numbers/structs and don't reference other patch entities.
/// - Params typed as `Signal` can contain `Cable { module, port }`, and params
///   typed as `Buffer` contain `{ module, port }` buffer references.
///   Those require existence checks against `patch.modules`.
///
/// Implementation strategy:
/// - Look for `$ref` containing "Signal" or "Buffer".
/// - Recurse through combinators (`anyOf/oneOf/allOf`) and `items` for arrays.
pub(super) fn schema_refers_to_module_reference(schema_node: &Schema) -> bool {
    if let Some(obj) = schema_node.as_object() {
        if let Some(r) = obj.get("$ref").and_then(|v| v.as_str()) {
            return r.contains("Signal") || r.contains("Buffer");
        }

        for key in ["anyOf", "oneOf", "allOf"] {
            if let Some(items) = obj.get(key).and_then(|v| v.as_array())
                && items.iter().any(|item| {
                    let schema: Result<Schema, _> = item.clone().try_into();
                    schema
                        .ok()
                        .is_some_and(|s| schema_refers_to_module_reference(&s))
                })
            {
                return true;
            }
        }

        if let Some(items) = obj.get("items") {
            let schema: Result<Schema, _> = items.clone().try_into();
            if let Ok(schema) = schema
                && schema_refers_to_module_reference(&schema)
            {
                return true;
            }
        }

        // Tuple schemas carry per-position schemas in `prefixItems`.
        if let Some(items) = obj.get("prefixItems").and_then(|v| v.as_array())
            && items.iter().any(|item| {
                let schema: Result<Schema, _> = item.clone().try_into();
                schema
                    .ok()
                    .is_some_and(|s| schema_refers_to_module_reference(&s))
            })
        {
            return true;
        }

        // Object schemas can nest Signal references inside `properties`.
        // This is common for complex params (struct-like objects).
        for key in ["properties", "additionalProperties"] {
            if let Some(props) = obj.get(key) {
                // `properties` is a map; `additionalProperties` can be a schema.
                if let Some(map) = props.as_object() {
                    if map.iter().any(|(_, v)| {
                        let schema: Result<Schema, _> = v.clone().try_into();
                        schema
                            .ok()
                            .is_some_and(|s| schema_refers_to_module_reference(&s))
                    }) {
                        return true;
                    }
                } else {
                    let schema: Result<Schema, _> = props.clone().try_into();
                    if schema
                        .ok()
                        .is_some_and(|s| schema_refers_to_module_reference(&s))
                    {
                        return true;
                    }
                }
            }
        }

        // Tolerate older shapes where properties appear under `schema`.
        if let Some(schema_obj) = obj.get("schema").and_then(|v| v.as_object())
            && let Some(props) = schema_obj.get("properties").and_then(|v| v.as_object())
            && props.iter().any(|(_, v)| {
                let schema: Result<Schema, _> = v.clone().try_into();
                schema
                    .ok()
                    .is_some_and(|s| schema_refers_to_module_reference(&s))
            })
        {
            return true;
        }
    }

    false
}

fn validate_signal_reference(
    signal: &Signal,
    field: &str,
    location: &str,
    module_by_id: &HashMap<&str, &ModuleSpec>,
    schema_map: &HashMap<&str, &ModuleSchema>,
    errors: &mut Vec<ValidationError>,
) {
    match signal {
        Signal::Cable {
            module: src_module,
            port: src_port,
            ..
        } => {
            // HiddenAudioIn is created internally by Rust and has no schema.
            // It's the only module of its kind - skip validation for connections to it.
            if src_module == WellKnownModule::HiddenAudioIn.id() {
                return;
            }

            let Some(src_state) = module_by_id.get(src_module.as_str()).copied() else {
                errors.push(ValidationError {
                    field: field.to_string(),
                    message: format!("Module '{}' not found for cable source", src_module),
                    location: Some(location.to_string()),
                    expected_type: None,
                    actual_value: None,
                });
                return;
            };

            let Some(src_schema) = schema_map.get(src_state.module_type.as_str()).copied() else {
                errors.push(ValidationError {
                    field: field.to_string(),
                    message: format!(
                        "Unknown module type '{}' for cable source module '{}'",
                        src_state.module_type, src_module
                    ),
                    location: Some(location.to_string()),
                    expected_type: None,
                    actual_value: None,
                });
                return;
            };

            if !src_schema.outputs.iter().any(|o| o.name == *src_port) {
                errors.push(ValidationError {
                    field: field.to_string(),
                    message: format!(
                        "Output port '{}' not found on module '{}'",
                        src_port, src_module
                    ),
                    location: Some(location.to_string()),
                    expected_type: None,
                    actual_value: None,
                });
            }
        }
        Signal::Volts(..) => {}
    }
}

pub(super) fn validate_signals_in_json_value(
    value: &serde_json::Value,
    field: &str,
    location: &str,
    module_by_id: &HashMap<&str, &ModuleSpec>,
    schema_map: &HashMap<&str, &ModuleSchema>,
    errors: &mut Vec<ValidationError>,
) {
    // Only attempt to parse as a Signal when the tagged discriminator looks right.
    // This avoids false positives and reduces cloning.
    if let Some(obj) = value.as_object()
        && let Some(tag) = obj.get("type").and_then(|v| v.as_str())
        && matches!(tag, "cable" | "volts")
        && let Ok(signal) = serde_json::from_value::<Signal>(value.clone())
    {
        validate_signal_reference(&signal, field, location, module_by_id, schema_map, errors);
        return;
    }

    // Validate buffer_ref targets: the referenced module must exist in the
    // patch and the referenced port must be one of its buffer outputs.
    // `Buffer::connect` on the audio thread silently caches nothing for an
    // unresolvable reference, so this is the only place the user gets an error.
    if let Some(obj) = value.as_object()
        && let Some(tag) = obj.get("type").and_then(|v| v.as_str())
        && tag == "buffer_ref"
    {
        if let Some(module_id) = obj.get("module").and_then(|v| v.as_str()) {
            let Some(src_module) = module_by_id.get(module_id).copied() else {
                errors.push(ValidationError {
                    field: field.to_string(),
                    message: format!(
                        "buffer_ref references module '{}' which does not exist in the patch",
                        module_id
                    ),
                    location: Some(location.to_string()),
                    expected_type: None,
                    actual_value: None,
                });
                return;
            };

            let port = obj.get("port").and_then(|v| v.as_str()).unwrap_or("");
            let is_buffer_port = schema_map
                .get(src_module.module_type.as_str())
                .is_some_and(|s| s.buffer_outputs.iter().any(|p| p == port));
            if !is_buffer_port {
                errors.push(ValidationError {
                    field: field.to_string(),
                    message: format!(
                        "buffer_ref references port '{}' on module '{}', which is not a buffer output",
                        port, module_id
                    ),
                    location: Some(location.to_string()),
                    expected_type: Some("a buffer output port (e.g. $buffer's 'buffer')".to_string()),
                    actual_value: None,
                });
            }
        }
        return;
    }

    match value {
        serde_json::Value::Array(arr) => {
            for v in arr {
                validate_signals_in_json_value(
                    v,
                    field,
                    location,
                    module_by_id,
                    schema_map,
                    errors,
                );
            }
        }
        serde_json::Value::Object(map) => {
            for (_, v) in map {
                validate_signals_in_json_value(
                    v,
                    field,
                    location,
                    module_by_id,
                    schema_map,
                    errors,
                );
            }
        }
        _ => {}
    }
}
