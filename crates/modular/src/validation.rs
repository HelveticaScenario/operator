use modular_core::dsp::get_param_validators;
use modular_core::params::ARGUMENT_SPANS_KEY;
use modular_core::types::{
  ModuleSchema, ModuleState, PatchGraph, Signal, WellKnownModule,
};
use napi_derive::napi;
use schemars::Schema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Detailed validation error for patch validation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[napi(object)]
pub struct ValidationError {
  pub field: String,
  pub message: String,
  pub location: Option<String>,
  /// Human-readable description of expected input type
  pub expected_type: Option<String>,
  /// JSON snippet of the actual value that failed
  pub actual_value: Option<String>,
}

/// Translate serde deserialization errors into user-friendly messages.
///
/// Maps cryptic Rust/serde error messages to DSL-oriented explanations.
fn translate_serde_error(raw: &str, module_type: &str) -> (String, Option<String>) {
  // Pattern: "data did not match any variant of untagged enum PolySignalDe"
  if raw.contains("untagged enum PolySignalDe") {
    return (
      format!("invalid input for '{}' parameter", module_type),
      Some("a signal: number, string (e.g. '440hz', 'c4'), or module output".to_string()),
    );
  }

  // Pattern: "data did not match any variant of untagged enum SignalDe"
  if raw.contains("untagged enum SignalDe") {
    return (
      format!("invalid input for '{}' parameter", module_type),
      Some("a signal: number, string (e.g. '440hz', 'c4'), or module output".to_string()),
    );
  }

  // Pattern: "invalid type: map, expected a sequence"
  if raw.contains("invalid type: map, expected a sequence") {
    return (
      "expected an array of signals, got a single object".to_string(),
      Some("an array like [signal1, signal2, ...]".to_string()),
    );
  }

  // Pattern: "invalid type: sequence, expected a map"
  if raw.contains("invalid type: sequence, expected a map") {
    return (
      "expected a single signal, got an array".to_string(),
      Some("a single signal (number, string, or module output)".to_string()),
    );
  }

  // Pattern: "invalid type: X, expected Y"
  if let Some(caps) = extract_type_mismatch(raw) {
    return (
      format!("expected {}, got {}", caps.1, caps.0),
      Some(caps.1.to_string()),
    );
  }

  // Pattern: "missing field `fieldName`"
  if raw.contains("missing field")
    && let Some(field) = extract_field_name(raw, "missing field")
  {
    return (format!("missing required parameter: {}", field), None);
  }

  // Pattern: "unknown field `fieldName`"
  if raw.contains("unknown field")
    && let Some(field) = extract_field_name(raw, "unknown field")
  {
    return (format!("unknown parameter: {}", field), None);
  }

  // Pattern: "invalid value: X, expected Y"
  if raw.contains("invalid value:")
    && let Some(caps) = extract_invalid_value(raw)
  {
    return (
      format!("invalid value: {}, expected {}", caps.0, caps.1),
      Some(caps.1),
    );
  }

  // Pattern: "expected X at line Y column Z" (JSON path errors)
  if raw.contains("expected") && (raw.contains("at line") || raw.contains("at column")) {
    // Strip the position info which isn't useful to DSL users
    let cleaned = raw.split(" at line").next().unwrap_or(raw).to_string();
    return (cleaned, None);
  }

  // Fallback: return original message cleaned up
  (raw.to_string(), None)
}

/// Extract "invalid type: X, expected Y" components
fn extract_type_mismatch(raw: &str) -> Option<(String, String)> {
  let prefix = "invalid type: ";
  if let Some(start) = raw.find(prefix) {
    let rest = &raw[start + prefix.len()..];
    if let Some(comma_pos) = rest.find(", expected ") {
      let actual = rest[..comma_pos].trim().to_string();
      let expected = rest[comma_pos + ", expected ".len()..].trim().to_string();
      return Some((actual, expected));
    }
  }
  None
}

/// Extract "invalid value: X, expected Y" components  
fn extract_invalid_value(raw: &str) -> Option<(String, String)> {
  let prefix = "invalid value: ";
  if let Some(start) = raw.find(prefix) {
    let rest = &raw[start + prefix.len()..];
    if let Some(comma_pos) = rest.find(", expected ") {
      let actual = rest[..comma_pos].trim().to_string();
      let expected = rest[comma_pos + ", expected ".len()..].trim().to_string();
      return Some((actual, expected));
    }
  }
  None
}

/// Extract field name from "missing field `X`" or "unknown field `X`"
fn extract_field_name(raw: &str, prefix: &str) -> Option<String> {
  if let Some(start) = raw.find(prefix) {
    let rest = &raw[start + prefix.len()..];
    // Look for backtick-quoted field name
    if let Some(tick_start) = rest.find('`') {
      let after_tick = &rest[tick_start + 1..];
      if let Some(tick_end) = after_tick.find('`') {
        return Some(after_tick[..tick_end].to_string());
      }
    }
  }
  None
}

/// Format module location for error messages.
///
/// For explicitly named modules, returns the user's ID (e.g., "myOscillator").
/// For auto-generated IDs, returns None so the error can be tied to source line instead.
fn format_module_location(module: &ModuleState) -> String {
  if module.id_is_explicit == Some(true) {
    // User explicitly set this ID, show it
    format!("'{}'", module.id)
  } else {
    // Auto-generated ID - this will be replaced by source line in TypeScript
    // For now, show module type as a hint
    format!("{}(...)", module.module_type)
  }
}

/// Truncate JSON value for error display (max ~100 chars)
fn truncate_json(value: &serde_json::Value) -> String {
  let s = value.to_string();
  if s.len() > 100 {
    format!("{}...", &s[..97])
  } else {
    s
  }
}

impl std::fmt::Display for ValidationError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    if let Some(ref location) = self.location {
      write!(f, "{}: {} (at {})", self.field, self.message, location)
    } else {
      write!(f, "{}: {}", self.field, self.message)
    }
  }
}

/// Extract the `properties` object from a schema node.
///
/// Returns a mapping from param name -> schema for that param.
/// If the schema doesn't look like an object schema with properties, returns empty.
fn schema_properties(schema: &Schema) -> HashMap<String, Schema> {
  // schemars::Schema is a thin wrapper around a serde_json::Value (object/bool).
  // Properties live under "properties" in the common case; we also tolerate
  // older "schema.properties" shapes.
  let props = schema.as_object().and_then(|obj| {
    obj
      .get("properties")
      .and_then(|v| v.as_object())
      .or_else(|| {
        obj
          .get("schema")
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

/// Returns true if `schema_node` describes (or contains) a `Signal`.
///
/// Why we need this:
/// - Most params are plain numbers/structs and don't reference other patch entities.
/// - Params typed as `Signal` can contain `Cable { module, port }`.
///   Those require existence checks against `patch.modules`.
///
/// Implementation strategy:
/// - Look for `$ref` containing "Signal".
/// - Recurse through combinators (`anyOf/oneOf/allOf`) and `items` for arrays.
fn schema_refers_to_signal(schema_node: &Schema) -> bool {
  if let Some(obj) = schema_node.as_object() {
    if let Some(r) = obj.get("$ref").and_then(|v| v.as_str()) {
      return r.ends_with("/Signal") || r.ends_with("definitions/Signal") || r.contains("Signal");
    }

    for key in ["anyOf", "oneOf", "allOf"] {
      if let Some(items) = obj.get(key).and_then(|v| v.as_array())
        && items.iter().any(|item| {
          let schema: Result<Schema, _> = item.clone().try_into();
          schema.ok().is_some_and(|s| schema_refers_to_signal(&s))
        })
      {
        return true;
      }
    }

    if let Some(items) = obj.get("items") {
      let schema: Result<Schema, _> = items.clone().try_into();
      if let Ok(schema) = schema
        && schema_refers_to_signal(&schema)
      {
        return true;
      }
    }

    // Object schemas can nest Signal references inside `properties`.
    // This is common for complex params (struct-like objects).
    for key in ["properties", "additionalProperties"] {
      if let Some(props) = obj.get(key) {
        // `properties` is a map; `additionalProperties` can be a schema.
        if let Some(map) = props.as_object() {
          if map.iter().any(|(_, v)| {
            let schema: Result<Schema, _> = v.clone().try_into();
            schema.ok().is_some_and(|s| schema_refers_to_signal(&s))
          }) {
            return true;
          }
        } else {
          let schema: Result<Schema, _> = props.clone().try_into();
          if schema.ok().is_some_and(|s| schema_refers_to_signal(&s)) {
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
        schema.ok().is_some_and(|s| schema_refers_to_signal(&s))
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
  module_by_id: &HashMap<&str, &ModuleState>,
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

fn validate_signals_in_json_value(
  value: &serde_json::Value,
  field: &str,
  location: &str,
  module_by_id: &HashMap<&str, &ModuleState>,
  schema_map: &HashMap<&str, &ModuleSchema>,
  errors: &mut Vec<ValidationError>,
) {
  // Only attempt to parse as a Signal when the tagged discriminator looks right.
  // This avoids false positives and reduces cloning.
  if let Some(obj) = value.as_object()
    && let Some(tag) = obj.get("type").and_then(|v| v.as_str())
    && matches!(tag, "cable" | "track" | "volts")
    && let Ok(signal) = serde_json::from_value::<Signal>(value.clone())
  {
    validate_signal_reference(&signal, field, location, module_by_id, schema_map, errors);
    return;
  }

  match value {
    serde_json::Value::Array(arr) => {
      for v in arr {
        validate_signals_in_json_value(v, field, location, module_by_id, schema_map, errors);
      }
    }
    serde_json::Value::Object(map) => {
      for (_, v) in map {
        validate_signals_in_json_value(v, field, location, module_by_id, schema_map, errors);
      }
    }
    _ => {}
  }
}

/// Validate a patch against the module schemas.
///
/// Returns all validation errors found (not just the first).
///
/// Validates:
/// - All module types exist in the schema
/// - Params in `ModuleState.params` are known for the module type
/// - Signal params with Cable/Track references point to existing modules/ports
/// - Scopes reference existing module outputs
pub fn validate_patch(
  patch: &PatchGraph,
  schemas: &[ModuleSchema],
) -> Result<(), Vec<ValidationError>> {
  // === Overview ===
  // This validator is intentionally "best effort": it walks the whole patch and
  // accumulates *all* issues it can find, returning them together.
  //
  // High-level flow:
  // 1) Build fast lookup tables (schemas by name, modules by id, track ids).
  // 2) Validate each module:
  //    - module type exists
  //    - module params only use known param names
  //    - for params whose schema indicates a `Signal`, validate any Cable/Track references
  // 3) Validate scopes:
  //    - referenced module exists
  //    - referenced output port exists on the module type
  //    - referenced track exists
  let mut errors = Vec::new();

  // === Indexing ===
  // Build a map from module type name -> schema.
  let schema_map: HashMap<&str, &ModuleSchema> =
    schemas.iter().map(|s| (s.name.as_str(), s)).collect();

  // Build a map from module type name -> typed params validator.
  //
  // This map is generated from the Rust module param structs via `#[module]`.
  // If a module type isn't present here (e.g. schemas were provided from a custom source),
  // we simply skip the typed-parse validation step for that module.
  let param_validators = get_param_validators();

  // Build a map from module id -> module instance (state) from the patch.
  let module_by_id: HashMap<&str, &ModuleState> =
    patch.modules.iter().map(|m| (m.id.as_str(), m)).collect();

  // === Schema helpers ===
  // The runtime patch stores parameter values as JSON (`ModuleState.params`), but
  // the authoritative set of valid parameter names/types lives in the module schema.

  // === Module validation ===
  // Validate each module instance in the patch.
  for module in &patch.modules {
    // Format location: show module ID only if explicitly set by user
    let location_str = format_module_location(module);

    // 1) Module type must exist in our schema registry.
    let Some(schema) = schema_map.get(module.module_type.as_str()).copied() else {
      errors.push(ValidationError {
        field: "moduleType".to_string(),
        message: format!("Unknown module type '{}'", module.module_type),
        location: Some(location_str.clone()),
        expected_type: None,
        actual_value: None,
      });
      continue;
    };

    // 2) Gather declared params for this module type (name -> schema node).
    //    This is what we compare the incoming JSON keys against.
    let param_schemas = schema_properties(&schema.params_schema.schema);

    // 3) Params must be a JSON object (map from param name -> JSON value).
    //    `null` is tolerated as "no params" because some senders may omit params.
    let Some(param_obj) = module.params.as_object() else {
      // params is defaulted; tolerate null/empty but flag other shapes.
      if !module.params.is_null() {
        errors.push(ValidationError {
          field: "params".to_string(),
          message: "Module params must be a JSON object".to_string(),
          location: Some(location_str.clone()),
          expected_type: Some("an object with parameter values".to_string()),
          actual_value: Some(truncate_json(&module.params)),
        });
      }
      continue;
    };

    // 3b) If available, validate that `module.params` can be deserialized into the
    // module's concrete `*Params` Rust type.
    //
    // Important: we only attempt this once we know params is an object. We explicitly
    // tolerate `null` elsewhere, and we don't want a redundant parse failure in that case.
    if let Some(validate) = param_validators.get(module.module_type.as_str())
      && let Err(err) = validate(&module.params)
    {
      let raw_error = err.to_string();
      let (translated_message, expected_type) =
        translate_serde_error(&raw_error, &module.module_type);
      errors.push(ValidationError {
        field: "params".to_string(),
        message: translated_message,
        location: Some(location_str.clone()),
        expected_type,
        actual_value: Some(truncate_json(&module.params)),
      });
    }

    // 4) Validate each param key/value pair.
    //
    // Notes:
    // - The generated typed validators (step 3b) ensure params have the correct shape/type.
    // - Here we *only* validate that any referenced targets (Cable/Track) actually exist.
    // - Params may contain Signals nested inside arbitrary serializable structures.
    for (param_name, param_value) in param_obj {
      // Skip internal metadata fields used for editor features (argument spans tracking).
      if param_name == ARGUMENT_SPANS_KEY {
        continue;
      }

      // 4a) Unknown param names are always an error.
      let Some(param_schema_node) = param_schemas.get(param_name) else {
        errors.push(ValidationError {
          field: format!("params.{}", param_name),
          message: format!(
            "Unknown parameter '{}' for module type '{}'",
            param_name, module.module_type
          ),
          location: Some(location_str.clone()),
          expected_type: None,
          actual_value: None,
        });
        continue;
      };

      // 4b) Only params whose schema indicates they *contain* Signals can reference entities.
      if !schema_refers_to_signal(param_schema_node) {
        continue;
      }

      let field = format!("params.{}", param_name);
      validate_signals_in_json_value(
        param_value,
        &field,
        &location_str,
        &module_by_id,
        &schema_map,
        &mut errors,
      );
    }
  }

    // === Scope validation ===
    for scope in &patch.scopes {
        if scope.channels.is_empty() {
            errors.push(ValidationError {
                field: "scopes".to_string(),
                message: "Scope has no channels".to_string(),
                location: None,
                expected_type: None,
                actual_value: None,
            });
            continue;
        }

        for channel in &scope.channels {
            // Scope target module must exist
            let Some(module) = module_by_id.get(channel.module_id.as_str()).copied() else {
                errors.push(ValidationError {
                    field: "scopes".to_string(),
                    message: format!("Scope references missing module '{}'", channel.module_id),
                    location: None,
                    expected_type: None,
                    actual_value: None,
                });
                continue;
            };

            // Target module type must be known
            let Some(schema) = schema_map.get(module.module_type.as_str()).copied() else {
                errors.push(ValidationError {
                    field: "scopes".to_string(),
                    message: format!(
                        "Scope references module '{}' with unknown type '{}'",
                        channel.module_id, module.module_type
                    ),
                    location: None,
                    expected_type: None,
                    actual_value: None,
                });
                continue;
            };

            // Output port must exist in module schema
            if !schema.outputs.iter().any(|o| o.name == *channel.port_name) {
                errors.push(ValidationError {
                    field: "scopes".to_string(),
                    message: format!(
                        "Scope references missing output port '{}' on module '{}'",
                        channel.port_name, channel.module_id
                    ),
                    location: None,
                    expected_type: None,
                    actual_value: None,
                });
            }
        }
    }

  // === Result ===
  // Return Ok for a clean patch; otherwise return all collected errors.
  if errors.is_empty() {
    Ok(())
  } else {
    Err(errors)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use modular_core::types::ModuleState;
  use serde_json::json;

  fn schemas() -> Vec<ModuleSchema> {
    modular_core::dsp::schema()
  }

  #[test]
  fn test_valid_patch() {
    let schemas = schemas();
    let patch = PatchGraph {
      modules: vec![ModuleState {
        id: "sine-1".to_string(),
        module_type: "$sine".to_string(),
        id_is_explicit: None,
        params: json!({
            "freq": 4.0
        }),
      }],
      module_id_remaps: None,

      scopes: vec![],
    };

    assert!(validate_patch(&patch, &schemas).is_ok());
  }

  #[test]
  fn test_unknown_module_type() {
    let schemas = schemas();
    let patch = PatchGraph {
      modules: vec![ModuleState {
        id: "foo-1".to_string(),
        module_type: "unknown-module".to_string(),
        id_is_explicit: None,
        params: json!({}),
      }],
      module_id_remaps: None,

      scopes: vec![],
    };

    let result = validate_patch(&patch, &schemas);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("Unknown module type"));
  }

  #[test]
  fn test_unknown_param() {
    let schemas = schemas();
    let patch = PatchGraph {
      modules: vec![ModuleState {
        id: "sine-1".to_string(),
        module_type: "$sine".to_string(),
        id_is_explicit: None,
        params: json!({
            "unknown_param": {"type": "volts", "value": 1.0}
        }),
      }],
      module_id_remaps: None,

      scopes: vec![],
    };

    let result = validate_patch(&patch, &schemas);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("Unknown parameter"));
  }

  #[test]
  fn test_cable_to_nonexistent_module() {
    let schemas = schemas();
    let patch = PatchGraph {
      modules: vec![ModuleState {
        id: "root".to_string(),
        module_type: "$signal".to_string(),
        id_is_explicit: None,
        params: json!({
            "source": {"type": "cable", "module": "nonexistent", "port": "output"}
        }),
      }],
      module_id_remaps: None,

      scopes: vec![],
    };

    let result = validate_patch(&patch, &schemas);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("not found for cable source"));
  }

  #[test]
  fn test_cable_to_invalid_port() {
    let schemas = schemas();
    let patch = PatchGraph {
      modules: vec![
        ModuleState {
          id: "sine-1".to_string(),
          module_type: "$sine".to_string(),
          id_is_explicit: None,
          params: json!({
              "freq": 4.0
          }),
        },
        ModuleState {
          id: "root".to_string(),
          module_type: "$signal".to_string(),
          id_is_explicit: None,
          params: json!({
              "source": {"type": "cable", "module": "sine-1", "port": "invalid_port"}
          }),
        },
      ],
      module_id_remaps: None,

      scopes: vec![],
    };

    let result = validate_patch(&patch, &schemas);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(
      errors[0]
        .message
        .contains("Output port 'invalid_port' not found")
    );
  }

  #[test]
  fn test_nested_signal_cable_to_nonexistent_module() {
    let schemas = schemas();
    let patch = PatchGraph {
      modules: vec![ModuleState {
        id: "nested-1".to_string(),
        module_type: "$mix".to_string(),
        id_is_explicit: None,
        params: json!({
            "inputs": [
              {"type": "cable", "module": "nonexistent", "port": "output"}
            ]
        }),
      }],
      module_id_remaps: None,

      scopes: vec![],
    };

    let result = validate_patch(&patch, &schemas);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| {
      // Location is now formatted as "moduleName(...)" for auto-generated IDs
      e.location.as_deref() == Some("$mix(...)")
        && e.field == "params.inputs"
        && e.message.contains("not found for cable source")
    }));
  }

  #[test]
  fn test_nested_signal_valid_cable_connection() {
    let schemas = schemas();
    let patch = PatchGraph {
      modules: vec![
        ModuleState {
          id: "sine-1".to_string(),
          module_type: "$sine".to_string(),
          id_is_explicit: None,
          params: json!({
              "freq": 4.0
          }),
        },
        ModuleState {
          id: "nested-1".to_string(),
          module_type: "$mix".to_string(),
          id_is_explicit: None,
          params: json!({
              "inputs": [
                {"type": "cable", "module": "sine-1", "port": "output"}
              ]
          }),
        },
      ],
      module_id_remaps: None,

      scopes: vec![],
    };

    assert!(validate_patch(&patch, &schemas).is_ok());
  }

  #[test]
  fn test_valid_cable_connection() {
    let schemas = schemas();
    let patch = PatchGraph {
      modules: vec![
        ModuleState {
          id: "sine-1".to_string(),
          module_type: "$sine".to_string(),
          id_is_explicit: None,
          params: json!({
              "freq": 4.0
          }),
        },
        ModuleState {
          id: "signal-1".to_string(),
          module_type: "$signal".to_string(),
          id_is_explicit: None,
          params: json!({
              "source": {"type": "cable", "module": "sine-1", "port": "output"}
          }),
        },
      ],
      module_id_remaps: None,

      scopes: vec![],
    };

    assert!(validate_patch(&patch, &schemas).is_ok());
  }

  #[test]
  fn test_multiple_errors() {
    let schemas = schemas();
    let patch = PatchGraph {
      modules: vec![ModuleState {
        id: "sine-1".to_string(),
        module_type: "$sine".to_string(),
        id_is_explicit: None,
        params: json!({
            "unknown1": 1.0,
            "unknown2": 2.0
        }),
      }],
      module_id_remaps: None,

      scopes: vec![],
    };

    let result = validate_patch(&patch, &schemas);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 2);
  }

  #[test]
  fn test_empty_patch_is_valid() {
    let schemas = schemas();
    let patch = PatchGraph {
      modules: Vec::new(),
      module_id_remaps: None,

      scopes: vec![],
    };

    assert!(validate_patch(&patch, &schemas).is_ok());
  }

  #[test]
  fn test_typed_params_validation_catches_missing_required_fields() {
    // Use the real schemas (and real typed validators) from modular_core.
    let schemas = modular_core::dsp::schema();

    // Ensure typed params validation fails by providing an invalid enum variant.
    // `color` expects one of white/pink/brown (lowercase).
    let patch = PatchGraph {
      modules: vec![ModuleState {
        id: "noise-1".to_string(),
        module_type: "$noise".to_string(),
        id_is_explicit: None,
        params: json!({
            "color": "invalid_color"
        }),
      }],
      module_id_remaps: None,

      scopes: vec![],
    };

    let result = validate_patch(&patch, &schemas);
    assert!(result.is_err());
    let errors = result.unwrap_err();

    // Location is formatted as "$noise(...)" for auto-generated IDs
    assert!(errors.iter().any(|e| {
      e.field == "params"
        && e.location.as_deref() == Some("$noise(...)")
        && e.message.contains("unknown variant")
    }));
  }

  #[test]
  fn test_null_params_is_tolerated_even_with_typed_validation() {
    // validate_patch treats `params: null` as "no params" and does not require
    // it to be deserializable into the module's concrete params type.
    let schemas = modular_core::dsp::schema();

    let patch = PatchGraph {
      modules: vec![ModuleState {
        id: "noise-1".to_string(),
        module_type: "$noise".to_string(),
        id_is_explicit: None,
        params: serde_json::Value::Null,
      }],
      module_id_remaps: None,

      scopes: vec![],
    };

    assert!(validate_patch(&patch, &schemas).is_ok());
  }
}
