use super::signal_refs::schema_refers_to_signal;
use super::*;
use modular_core::types::ModuleSpec;
use schemars::Schema;
use serde_json::json;

fn schemas() -> Vec<ModuleSchema> {
    modular_core::dsp::schema()
}

#[test]
fn test_valid_patch() {
    let schemas = schemas();
    let patch = PatchGraph {
        modules: vec![ModuleSpec {
            id: "sine-1".to_string(),
            module_type: "$sine".to_string(),
            id_is_explicit: None,
            params: json!({
                "freq": 4.0
            }),
        }],
        module_id_remaps: None,

        scopes: vec![],
        scope_xy: None,
    };

    assert!(validate_patch(&patch, &schemas).is_ok());
}

fn patch_with_scope_xy(x_port: &str, x_module: &str) -> PatchGraph {
    use modular_core::types::{ScopeChannel, ScopeXy, ScopeXyPair};
    PatchGraph {
        modules: vec![ModuleSpec {
            id: "sine-1".to_string(),
            module_type: "$sine".to_string(),
            id_is_explicit: None,
            params: json!({ "freq": 4.0 }),
        }],
        module_id_remaps: None,
        scopes: vec![],
        scope_xy: Some(ScopeXy {
            pairs: vec![ScopeXyPair {
                x: ScopeChannel {
                    module_id: x_module.to_string(),
                    port_name: x_port.to_string(),
                    channel: 0,
                },
                y: ScopeChannel {
                    module_id: "sine-1".to_string(),
                    port_name: "output".to_string(),
                    channel: 0,
                },
            }],
            x_range: (-5.0, 5.0),
            y_range: (-5.0, 5.0),
        }),
    }
}

#[test]
fn test_scope_xy_valid() {
    let schemas = schemas();
    let patch = patch_with_scope_xy("output", "sine-1");
    assert!(validate_patch(&patch, &schemas).is_ok());
}

#[test]
fn test_scope_xy_missing_module() {
    let schemas = schemas();
    let patch = patch_with_scope_xy("output", "ghost");
    let errors = validate_patch(&patch, &schemas).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.field == "scopeXY" && e.message.contains("missing module")),
        "expected a missing-module scopeXY error, got {errors:?}"
    );
}

#[test]
fn test_scope_xy_missing_port() {
    let schemas = schemas();
    let patch = patch_with_scope_xy("nope", "sine-1");
    let errors = validate_patch(&patch, &schemas).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.field == "scopeXY" && e.message.contains("missing output port")),
        "expected a missing-output-port scopeXY error, got {errors:?}"
    );
}

#[test]
fn test_unknown_module_type() {
    let schemas = schemas();
    let patch = PatchGraph {
        modules: vec![ModuleSpec {
            id: "foo-1".to_string(),
            module_type: "unknown-module".to_string(),
            id_is_explicit: None,
            params: json!({}),
        }],
        module_id_remaps: None,

        scopes: vec![],
        scope_xy: None,
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
        modules: vec![ModuleSpec {
            id: "root".to_string(),
            module_type: "$signal".to_string(),
            id_is_explicit: None,
            params: json!({
                "source": {"type": "cable", "module": "nonexistent", "port": "output"}
            }),
        }],
        module_id_remaps: None,

        scopes: vec![],
        scope_xy: None,
    };

    let result = validate_patch(&patch, &schemas);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("not found for cable source"));
}

#[test]
fn test_schema_refers_to_signal_descends_prefix_items() {
    let schema: Schema = json!({
        "type": "array",
        "prefixItems": [
            { "type": "array", "items": { "type": "number" } },
            { "$ref": "#/$defs/MonoSignal" }
        ]
    })
    .try_into()
    .unwrap();
    assert!(schema_refers_to_signal(&schema));
}

#[test]
fn test_cable_in_slice_tuple_to_nonexistent_module() {
    // The sampler's slice param nests a signal inside a tuple
    // (prefixItems) inside an anyOf — a dangling cable there must still be
    // reference-checked.
    let schemas = schemas();
    let patch = PatchGraph {
        modules: vec![ModuleSpec {
            id: "s1".to_string(),
            module_type: "$sampler".to_string(),
            id_is_explicit: None,
            params: json!({
                "wav": { "type": "wav_ref", "path": "test", "channels": 1 },
                "gate": 0.0,
                "slice": [[0.0, 0.5], {"type": "cable", "module": "nonexistent", "port": "output"}]
            }),
        }],
        module_id_remaps: None,

        scopes: vec![],
        scope_xy: None,
    };

    let result = validate_patch(&patch, &schemas);
    assert!(
        result.is_err(),
        "dangling cable in slice tuple must be caught"
    );
    let errors = result.unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("not found for cable source")),
        "expected a cable-source error, got {errors:?}"
    );
}

#[test]
fn test_cable_to_invalid_port() {
    let schemas = schemas();
    let patch = PatchGraph {
        modules: vec![
            ModuleSpec {
                id: "sine-1".to_string(),
                module_type: "$sine".to_string(),
                id_is_explicit: None,
                params: json!({
                    "freq": 4.0
                }),
            },
            ModuleSpec {
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
        scope_xy: None,
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
        modules: vec![ModuleSpec {
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
        scope_xy: None,
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
            ModuleSpec {
                id: "sine-1".to_string(),
                module_type: "$sine".to_string(),
                id_is_explicit: None,
                params: json!({
                    "freq": 4.0
                }),
            },
            ModuleSpec {
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
        scope_xy: None,
    };

    assert!(validate_patch(&patch, &schemas).is_ok());
}

#[test]
fn test_valid_cable_connection() {
    let schemas = schemas();
    let patch = PatchGraph {
        modules: vec![
            ModuleSpec {
                id: "sine-1".to_string(),
                module_type: "$sine".to_string(),
                id_is_explicit: None,
                params: json!({
                    "freq": 4.0
                }),
            },
            ModuleSpec {
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
        scope_xy: None,
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
fn test_truncate_json_multibyte_content() {
    // Byte 97 of the serialized value falls inside a multibyte codepoint;
    // truncation must land on a char boundary.
    let value = json!(format!("a{}", "あ".repeat(40)));
    let truncated = truncate_json(&value);
    assert!(truncated.ends_with("..."));
    assert!(truncated.len() <= 100);
}

#[test]
fn test_non_object_multibyte_params_reported_not_panicking() {
    let schemas = schemas();
    let patch = PatchGraph {
        modules: vec![ModuleSpec {
            id: "sine-1".to_string(),
            module_type: "$sine".to_string(),
            id_is_explicit: None,
            params: json!(format!("a{}", "あ".repeat(40))),
        }],
        module_id_remaps: None,

        scopes: vec![],
        scope_xy: None,
    };

    let errors = validate_patch(&patch, &schemas).unwrap_err();
    assert!(errors.iter().any(|e| {
        e.message.contains("must be a JSON object")
            && e.actual_value
                .as_deref()
                .is_some_and(|v| v.ends_with("..."))
    }));
}

#[test]
fn test_duplicate_module_ids_rejected() {
    let schemas = schemas();
    let module = ModuleSpec {
        id: "dup".to_string(),
        module_type: "$sine".to_string(),
        id_is_explicit: Some(true),
        params: json!({ "freq": 4.0 }),
    };
    let patch = PatchGraph {
        modules: vec![module.clone(), module],
        module_id_remaps: None,

        scopes: vec![],
        scope_xy: None,
    };

    let errors = validate_patch(&patch, &schemas).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.field == "id" && e.message.contains("Duplicate module id 'dup'")),
        "expected a duplicate-id error, got {errors:?}"
    );
}

#[test]
fn test_hidden_audio_in_id_rejected() {
    let schemas = schemas();
    let patch = PatchGraph {
        modules: vec![ModuleSpec {
            id: "HIDDEN_AUDIO_IN".to_string(),
            module_type: "$sine".to_string(),
            id_is_explicit: Some(true),
            params: json!({ "freq": 4.0 }),
        }],
        module_id_remaps: None,

        scopes: vec![],
        scope_xy: None,
    };

    let errors = validate_patch(&patch, &schemas).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.field == "id" && e.message.contains("reserved for the engine")),
        "expected a reserved-id error, got {errors:?}"
    );
}

#[test]
fn test_root_clock_id_requires_clock_type() {
    let schemas = schemas();
    let mk_patch = |module_type: &str| PatchGraph {
        modules: vec![ModuleSpec {
            id: "ROOT_CLOCK".to_string(),
            module_type: module_type.to_string(),
            id_is_explicit: Some(true),
            params: json!({}),
        }],
        module_id_remaps: None,

        scopes: vec![],
        scope_xy: None,
    };

    let errors = validate_patch(&mk_patch("$noise"), &schemas).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.field == "id" && e.message.contains("reserved for a _clock module")),
        "expected a reserved-id error, got {errors:?}"
    );

    assert!(validate_patch(&mk_patch("_clock"), &schemas).is_ok());
}

#[test]
fn test_empty_patch_is_valid() {
    let schemas = schemas();
    let patch = PatchGraph {
        modules: Vec::new(),
        module_id_remaps: None,

        scopes: vec![],
        scope_xy: None,
    };

    assert!(validate_patch(&patch, &schemas).is_ok());
}

#[test]
fn test_null_params_is_tolerated() {
    // validate_patch treats `params: null` as "no params" — it skips
    // further param validation for that module.
    let schemas = modular_core::dsp::schema();

    let patch = PatchGraph {
        modules: vec![ModuleSpec {
            id: "noise-1".to_string(),
            module_type: "$noise".to_string(),
            id_is_explicit: None,
            params: serde_json::Value::Null,
        }],
        module_id_remaps: None,

        scopes: vec![],
        scope_xy: None,
    };

    assert!(validate_patch(&patch, &schemas).is_ok());
}
