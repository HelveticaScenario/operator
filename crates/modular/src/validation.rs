use modular_core::params::ARGUMENT_SPANS_KEY;
use modular_core::types::{ModuleSchema, ModuleState, PatchGraph, Signal, WellKnownModule};
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
    && matches!(tag, "cable" | "volts")
    && let Ok(signal) = serde_json::from_value::<Signal>(value.clone())
  {
    validate_signal_reference(&signal, field, location, module_by_id, schema_map, errors);
    return;
  }

  // Validate buffer_ref targets: the referenced module must exist in the patch.
  if let Some(obj) = value.as_object()
    && let Some(tag) = obj.get("type").and_then(|v| v.as_str())
    && tag == "buffer_ref"
  {
    if let Some(module_id) = obj.get("module").and_then(|v| v.as_str()) {
      if !module_by_id.contains_key(module_id) {
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
      }
    }
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
/// - Signal params with Cable references point to existing modules/ports
/// - Scopes reference existing module outputs
///
/// Note: Param-level validation (unknown fields, type checking) is handled by
/// deserr during deserialization. This validator focuses on graph-level concerns.
pub fn validate_patch(
  patch: &PatchGraph,
  schemas: &[ModuleSchema],
) -> Result<(), Vec<ValidationError>> {
  // === Overview ===
  // This validator is intentionally "best effort": it walks the whole patch and
  // accumulates *all* issues it can find, returning them together.
  //
  // High-level flow:
  // 1) Build fast lookup tables (schemas by name, modules by id).
  // 2) Validate each module:
  //    - module type exists
  //    - for params whose schema indicates a `Signal`, validate any Cable references
  //    (param-level validation is now handled by deserr)
  // 3) Validate scopes:
  //    - referenced module exists
  //    - referenced output port exists on the module type
  let mut errors = Vec::new();

  // === Indexing ===
  // Build a map from module type name -> schema.
  let schema_map: HashMap<&str, &ModuleSchema> =
    schemas.iter().map(|s| (s.name.as_str(), s)).collect();

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

    // 4) Validate cable references in Signal-typed params.
    //
    // Note: Param-level validation (unknown fields, type checking) is handled
    // by deserr. This loop only validates graph-level concerns: that Cable
    // references point to existing modules and valid output ports.
    for (param_name, param_value) in param_obj {
      // Skip internal metadata fields used for editor features (argument spans tracking).
      if param_name == ARGUMENT_SPANS_KEY {
        continue;
      }

      let field = format!("params.{}", param_name);

      // Skip unknown param names — deserr now handles this via deny_unknown_fields.
      let Some(param_schema_node) = param_schemas.get(param_name) else {
        continue;
      };

      // 4b) Only params whose schema indicates they *contain* Signals can reference entities.
      if !schema_refers_to_signal(param_schema_node) {
        continue;
      }
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
  fn test_unknown_param_via_deserr() {
    // Unknown params are now rejected by deserr (deny_unknown_fields) rather
    // than by validate_patch. Verify that deserr catches them.
    // Use $noise because all its params are optional — we only want
    // the "unknown parameter" error, not an extra "missing required param" error.
    let params = json!({
        "unknown_param": {"type": "volts", "value": 1.0}
    });
    let result = crate::params_cache::deserialize_params("$noise", params, false);
    assert!(result.is_err());
    let errors = result.err().unwrap().into_errors();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("unknown parameter"));
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
  fn test_multiple_unknown_params_via_deserr() {
    // Multiple unknown params are now caught by deserr (deny_unknown_fields).
    // deserr accumulates all errors via ControlFlow::Continue.
    // Use $noise because all its params are optional — we only want
    // "unknown parameter" errors, not extra "missing required param" errors.
    let params = json!({
        "unknown1": 1.0,
        "unknown2": 2.0
    });
    let result = crate::params_cache::deserialize_params("$noise", params, false);
    assert!(result.is_err());
    let errors = result.err().unwrap().into_errors();
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
  fn test_null_params_is_tolerated() {
    // validate_patch treats `params: null` as "no params" — it skips
    // further param validation for that module.
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
