//! Render `factoryMetadata.json` — per-module metadata consumed by ts-morph
//! span analyzers (PR 7) and by the runtime as a debug introspection aid.

use serde_json::Value;

use crate::types::ModuleSchema;

/// Build the JSON document. Emits a stable array sorted by module name so
/// regenerations don't churn the diff when DSP definitions move around.
pub fn render(schemas: &[ModuleSchema]) -> Value {
    let mut entries: Vec<Value> = schemas
        .iter()
        .map(|s| {
            serde_json::json!({
                "moduleName": s.name,
                "factoryName": sanitize_identifier(&s.name),
                "namespacePath": namespace_path(&s.name),
                "positionalArgNames": s.positional_args.iter().map(|p| &p.name).collect::<Vec<_>>(),
                "outputs": s.outputs.iter().map(|o| serde_json::json!({
                    "name": o.name,
                    "polyphonic": o.polyphonic,
                    "default": o.default,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        a["moduleName"]
            .as_str()
            .unwrap_or("")
            .cmp(b["moduleName"].as_str().unwrap_or(""))
    });

    Value::Array(entries)
}

/// Mirror of `factory/identifiers.ts::sanitizeIdentifier`.
fn sanitize_identifier(name: &str) -> String {
    let re = regex::Regex::new(r"[^a-zA-Z0-9_$]+(.)?").unwrap();
    let mut id = re
        .replace_all(name, |caps: &regex::Captures| {
            caps.get(1)
                .map(|m| m.as_str().to_uppercase())
                .unwrap_or_default()
        })
        .into_owned();
    if id.is_empty() {
        return "_".into();
    }
    let first = id.chars().next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        id = format!("_{id}");
    }
    id
}

/// Split a dotted module name into its namespace path (excluding the leaf).
fn namespace_path(name: &str) -> Vec<String> {
    let parts: Vec<&str> = name.trim().split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 1 {
        return Vec::new();
    }
    parts[..parts.len() - 1]
        .iter()
        .map(|s| s.to_string())
        .collect()
}
