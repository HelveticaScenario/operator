mod module_ids;
mod signal_refs;

use modular_core::params::ARGUMENT_SPANS_KEY;
use modular_core::types::{ModuleSchema, ModuleSpec, PatchGraph};
use napi_derive::napi;
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
fn format_module_location(module: &ModuleSpec) -> String {
    if module.id_is_explicit == Some(true) {
        // User explicitly set this ID, show it
        format!("'{}'", module.id)
    } else {
        // Auto-generated ID: show the module type as a hint. The TypeScript
        // layer replaces this with the module's source line.
        format!("{}(...)", module.module_type)
    }
}

/// Truncate JSON value for error display (max ~100 chars)
fn truncate_json(value: &serde_json::Value) -> String {
    let s = value.to_string();
    if s.len() > 100 {
        // Back off to a char boundary — a fixed byte offset can land inside a
        // multibyte codepoint, and slicing there panics.
        let mut end = 97;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
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
    // 2) Validate module ids (uniqueness, engine reservations).
    // 3) Validate each module:
    //    - module type exists
    //    - for params whose schema indicates a `Signal`, validate any Cable references
    //    (param-level validation is deserr's job)
    // 4) Validate scopes:
    //    - referenced module exists
    //    - referenced output port exists on the module type
    let mut errors = Vec::new();

    // === Indexing ===
    // Build a map from module type name -> schema.
    let schema_map: HashMap<&str, &ModuleSchema> =
        schemas.iter().map(|s| (s.name.as_str(), s)).collect();

    // Build a map from module id -> module instance (state) from the patch.
    let module_by_id: HashMap<&str, &ModuleSpec> =
        patch.modules.iter().map(|m| (m.id.as_str(), m)).collect();

    module_ids::validate_module_ids(patch, &mut errors);

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
        let param_schemas = signal_refs::schema_properties(&schema.params_schema.schema);

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

            // Skip unknown param names — deserr rejects those via deny_unknown_fields.
            let Some(param_schema_node) = param_schemas.get(param_name) else {
                continue;
            };

            // 4b) Only params whose schema indicates they *contain* Signals or
            //     Buffers can reference entities.
            if !signal_refs::schema_refers_to_module_reference(param_schema_node) {
                continue;
            }
            signal_refs::validate_signals_in_json_value(
                param_value,
                &field,
                &location_str,
                &module_by_id,
                &schema_map,
                &mut errors,
            );
        }
    }

    // === Scope XY validation ===
    if let Some(scope_xy) = patch.scope_xy.as_ref() {
        for (idx, pair) in scope_xy.pairs.iter().enumerate() {
            for (axis, ch) in [("x", &pair.x), ("y", &pair.y)] {
                let Some(module) = module_by_id.get(ch.module_id.as_str()).copied() else {
                    errors.push(ValidationError {
                        field: "scopeXY".to_string(),
                        message: format!(
                            "$scopeXY pair {} ({}) references missing module '{}'",
                            idx, axis, ch.module_id
                        ),
                        location: None,
                        expected_type: None,
                        actual_value: None,
                    });
                    continue;
                };

                let Some(schema) = schema_map.get(module.module_type.as_str()).copied() else {
                    errors.push(ValidationError {
                        field: "scopeXY".to_string(),
                        message: format!(
                            "$scopeXY pair {} ({}) references module '{}' with unknown type '{}'",
                            idx, axis, ch.module_id, module.module_type
                        ),
                        location: None,
                        expected_type: None,
                        actual_value: None,
                    });
                    continue;
                };

                if !schema.outputs.iter().any(|o| o.name == *ch.port_name) {
                    errors.push(ValidationError {
                        field: "scopeXY".to_string(),
                        message: format!(
                            "$scopeXY pair {} ({}) references missing output port '{}' on module '{}'",
                            idx, axis, ch.port_name, ch.module_id
                        ),
                        location: None,
                        expected_type: None,
                        actual_value: None,
                    });
                }
            }
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
mod tests;
