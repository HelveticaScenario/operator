//! JSON Schema → TypeScript type expression resolver.
//!
//! Direct port of `src/shared/dsl/schemaTypeResolver.ts`. Walks a `serde_json::Value`
//! representing a JSON Schema (as produced by `schemars`) and emits a TS type
//! expression string suitable for embedding in factory signatures and Monaco's
//! lib.d.ts.
//!
//! Sentinel rule: `Signal`, `PolySignal`, `MonoSignal`, `Buffer`, `Table` from
//! `$defs` map to `Signal` / `Poly<Signal>` / `Mono<Signal>` / `BufferOutputRef` /
//! `Table` respectively (these are runtime types defined in `@modular/dsl`).

use serde_json::Value;
use std::fmt;

/// Result of resolving a `$ref`. Either a sentinel signal type, a `Buffer`/`Table`
/// reference, or the raw resolved sub-schema.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedRef<'a> {
    Signal,
    PolySignal,
    MonoSignal,
    Buffer,
    Table,
    Schema(&'a Value),
}

#[derive(Debug)]
pub enum ResolveError {
    UnsupportedRef(String),
    MissingRef(String),
    UnsupportedSchema(String),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::UnsupportedRef(r) => write!(f, "Unsupported $ref: {r}"),
            ResolveError::MissingRef(r) => write!(f, "Unresolved $ref: {r}"),
            ResolveError::UnsupportedSchema(s) => write!(f, "Unsupported schema: {s}"),
        }
    }
}

impl std::error::Error for ResolveError {}

const DEFS_PREFIX: &str = "#/$defs/";

/// Resolve a `$ref` against the root schema's `$defs`. Returns sentinel for
/// well-known signal/buffer/table names.
pub fn resolve_ref<'a>(
    reference: &str,
    root_schema: &'a Value,
) -> Result<ResolvedRef<'a>, ResolveError> {
    match reference {
        "Signal" => return Ok(ResolvedRef::Signal),
        "Buffer" => return Ok(ResolvedRef::Buffer),
        "Table" => return Ok(ResolvedRef::Table),
        _ => {}
    }

    let def_name = match reference.strip_prefix(DEFS_PREFIX) {
        Some(name) => name,
        None => return Err(ResolveError::UnsupportedRef(reference.to_string())),
    };

    match def_name {
        "Signal" => return Ok(ResolvedRef::Signal),
        "PolySignal" => return Ok(ResolvedRef::PolySignal),
        "MonoSignal" => return Ok(ResolvedRef::MonoSignal),
        "Buffer" => return Ok(ResolvedRef::Buffer),
        "Table" => return Ok(ResolvedRef::Table),
        _ => {}
    }

    let defs = root_schema
        .get("$defs")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ResolveError::MissingRef(reference.to_string()))?;
    let resolved = defs
        .get(def_name)
        .ok_or_else(|| ResolveError::MissingRef(reference.to_string()))?;

    // If the resolved schema has a sentinel `title`, treat it as such.
    if let Some(title) = resolved.get("title").and_then(|v| v.as_str()) {
        match title {
            "Signal" => return Ok(ResolvedRef::Signal),
            "PolySignal" => return Ok(ResolvedRef::PolySignal),
            "MonoSignal" => return Ok(ResolvedRef::MonoSignal),
            "Buffer" => return Ok(ResolvedRef::Buffer),
            "Table" => return Ok(ResolvedRef::Table),
            _ => {}
        }
    }

    Ok(ResolvedRef::Schema(resolved))
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariantInfo {
    /// JSON-serialized const value (e.g. `"vaVcf"` with quotes).
    pub value: String,
    /// Raw const value (re-serializable).
    pub raw_value: Value,
    /// Description from `///` doc comments on the variant, if any.
    pub description: Option<String>,
}

/// Extract enum variant info if `schema` represents an enum, else `None`.
/// Follows `$ref` pointers (returns `None` for sentinel signal types).
pub fn get_enum_variants(
    schema: &Value,
    root_schema: &Value,
) -> Result<Option<Vec<EnumVariantInfo>>, ResolveError> {
    let obj = match schema.as_object() {
        Some(o) => o,
        None => return Ok(None),
    };

    if let Some(reference) = obj.get("$ref").and_then(|v| v.as_str()) {
        return match resolve_ref(reference, root_schema)? {
            ResolvedRef::Schema(inner) => get_enum_variants(inner, root_schema),
            _ => Ok(None),
        };
    }

    if let Some(variants) = obj
        .get("oneOf")
        .or_else(|| obj.get("anyOf"))
        .and_then(|v| v.as_array())
    {
        let is_enum = variants.iter().all(|v| v.get("const").is_some());
        if is_enum {
            let infos = variants
                .iter()
                .map(|v| EnumVariantInfo {
                    value: serde_json::to_string(v.get("const").unwrap()).unwrap_or_default(),
                    raw_value: v.get("const").cloned().unwrap_or(Value::Null),
                    description: v
                        .get("description")
                        .and_then(|d| d.as_str())
                        .map(String::from),
                })
                .collect();
            return Ok(Some(infos));
        }
        return Ok(None);
    }

    if let Some(values) = obj.get("enum").and_then(|v| v.as_array()) {
        if !values.is_empty() {
            let infos = values
                .iter()
                .map(|v| EnumVariantInfo {
                    value: serde_json::to_string(v).unwrap_or_default(),
                    raw_value: v.clone(),
                    description: None,
                })
                .collect();
            return Ok(Some(infos));
        }
    }

    Ok(None)
}

/// Convert a JSON Schema node into a TypeScript type expression.
///
/// Handles `$ref` (Signal/PolySignal/MonoSignal + arbitrary defs), `oneOf`/`anyOf`
/// enum patterns, `allOf`, nullable type arrays, primitive types, object types,
/// array/tuple types, and `enum` schemas.
pub fn schema_to_type_expr(
    schema: &Value,
    root_schema: &Value,
) -> Result<String, ResolveError> {
    let obj = match schema.as_object() {
        Some(o) => o,
        None => return Err(ResolveError::UnsupportedSchema("non-object node".into())),
    };

    // oneOf / anyOf
    if let Some(variants) = obj
        .get("oneOf")
        .or_else(|| obj.get("anyOf"))
        .and_then(|v| v.as_array())
    {
        // Filter out null variants (Rust `Option<T>` adds `null`); optionality
        // is handled at the param level via `?:` in TypeScript.
        let non_null: Vec<&Value> = variants
            .iter()
            .filter(|v| {
                v.get("type")
                    .and_then(|t| t.as_str())
                    .map(|t| t != "null")
                    .unwrap_or(true)
            })
            .collect();

        if non_null.is_empty() {
            return Ok("unknown".into());
        }
        if non_null.len() == 1 {
            return schema_to_type_expr(non_null[0], root_schema);
        }

        // All-`const` → enum literal union
        let is_enum = non_null.iter().all(|v| v.get("const").is_some());
        if is_enum {
            let parts: Vec<String> = non_null
                .iter()
                .map(|v| serde_json::to_string(v.get("const").unwrap()).unwrap_or_default())
                .collect();
            return Ok(parts.join(" | "));
        }

        let types: Vec<String> = non_null
            .iter()
            .map(|v| schema_to_type_expr(v, root_schema))
            .collect::<Result<_, _>>()?;

        if types.iter().all(|t| t == "Signal") {
            return Ok("Poly<Signal>".into());
        }
        if types.iter().any(|t| t == "Signal") && types.iter().any(|t| t == "Signal[]") {
            return Ok("Poly<Signal>".into());
        }

        return Ok(types.join(" | "));
    }

    if obj.contains_key("allOf") {
        return Ok("any".into());
    }

    // type as array (e.g. ["string", "null"])
    if let Some(types_arr) = obj.get("type").and_then(|v| v.as_array()) {
        let non_null: Vec<&str> = types_arr
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|t| *t != "null")
            .collect();

        if non_null.len() == 1 {
            return Ok(map_primitive(non_null[0]).to_string());
        }
        if non_null.is_empty() {
            return Ok("any".into());
        }
        let mapped: Vec<&str> = non_null.iter().map(|t| map_primitive(t)).collect();
        return Ok(mapped.join(" | "));
    }

    // $ref
    if let Some(reference) = obj.get("$ref").and_then(|v| v.as_str()) {
        return Ok(match resolve_ref(reference, root_schema)? {
            ResolvedRef::Signal => "Signal".into(),
            ResolvedRef::PolySignal => "Poly<Signal>".into(),
            ResolvedRef::MonoSignal => "Mono<Signal>".into(),
            ResolvedRef::Buffer => "BufferOutputRef".into(),
            ResolvedRef::Table => "Table".into(),
            ResolvedRef::Schema(inner) => schema_to_type_expr(inner, root_schema)?,
        });
    }

    // enum
    if let Some(values) = obj.get("enum").and_then(|v| v.as_array()) {
        if values.is_empty() {
            return Err(ResolveError::UnsupportedSchema("empty enum".into()));
        }
        let parts: Vec<String> = values
            .iter()
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .collect();
        return Ok(parts.join(" | "));
    }

    let ty = obj.get("type").and_then(|v| v.as_str());

    if let Some(t) = ty {
        match t {
            "integer" | "number" => return Ok("number".into()),
            "string" => return Ok("string".into()),
            "boolean" => return Ok("boolean".into()),
            _ => {}
        }
    }

    let looks_like_object = matches!(ty, Some("object")) || obj.get("properties").is_some();
    if looks_like_object {
        let props = obj
            .get("properties")
            .and_then(|v| v.as_object());
        let props = match props {
            Some(p) if !p.is_empty() => p,
            _ => return Ok("{}".into()),
        };

        let required: std::collections::HashSet<&str> = obj
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let mut parts: Vec<String> = Vec::new();
        for (prop_name, prop_schema) in props.iter() {
            let optional_marker = if required.contains(prop_name.as_str()) {
                ""
            } else {
                "?"
            };
            let type_expr = schema_to_type_expr(prop_schema, root_schema)?;
            parts.push(format!(
                "{}{}: {}",
                render_property_key(prop_name),
                optional_marker,
                type_expr
            ));
        }
        return Ok(format!("{{ {} }}", parts.join("; ")));
    }

    if matches!(ty, Some("array")) {
        if let Some(prefix_items) = obj.get("prefixItems").and_then(|v| v.as_array()) {
            let parts: Vec<String> = prefix_items
                .iter()
                .map(|s| schema_to_type_expr(s, root_schema))
                .collect::<Result<_, _>>()?;
            return Ok(format!("[{}]", parts.join(", ")));
        }
        if let Some(items) = obj.get("items") {
            let inner = schema_to_type_expr(items, root_schema)?;
            return Ok(format!("{inner}[]"));
        }
        return Err(ResolveError::UnsupportedSchema(
            "array missing items/prefixItems".into(),
        ));
    }

    if ty.is_none() {
        if let Some(c) = obj.get("const") {
            return Ok(serde_json::to_string(c).unwrap_or_default());
        }
        return Err(ResolveError::UnsupportedSchema("missing type".into()));
    }

    Err(ResolveError::UnsupportedSchema(format!(
        "unsupported scalar type: {}",
        ty.unwrap_or("?")
    )))
}

fn map_primitive(t: &str) -> &'static str {
    match t {
        "integer" | "number" => "number",
        "string" => "string",
        "boolean" => "boolean",
        _ => "any",
    }
}

fn render_property_key(name: &str) -> String {
    if is_valid_identifier(name) {
        name.to_string()
    } else {
        serde_json::to_string(name).unwrap_or_else(|_| format!("\"{name}\""))
    }
}

fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !(first.is_ascii_alphabetic() || first == '$' || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '$' || c == '_')
}
