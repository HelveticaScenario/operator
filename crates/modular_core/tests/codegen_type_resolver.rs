//! Golden tests for the JSON Schema → TypeScript type expression resolver.
//!
//! Each test feeds a small JSON Schema fragment through `schema_to_type_expr`
//! and asserts the produced TS type expression matches the expected string.
//! Mirrors the branches in `src/shared/dsl/schemaTypeResolver.ts`.

use modular_core::codegen::type_resolver::{
    get_enum_variants, schema_to_type_expr, ResolvedRef,
};
use serde_json::json;

fn root_with_defs(defs: serde_json::Value) -> serde_json::Value {
    json!({ "$defs": defs })
}

#[test]
fn primitive_number() {
    let schema = json!({ "type": "number" });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        "number"
    );
}

#[test]
fn primitive_integer_maps_to_number() {
    let schema = json!({ "type": "integer" });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        "number"
    );
}

#[test]
fn primitive_string() {
    let schema = json!({ "type": "string" });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        "string"
    );
}

#[test]
fn primitive_boolean() {
    let schema = json!({ "type": "boolean" });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        "boolean"
    );
}

#[test]
fn nullable_type_array_collapses_to_inner() {
    let schema = json!({ "type": ["string", "null"] });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        "string"
    );
}

#[test]
fn ref_signal_sentinel() {
    let root = root_with_defs(json!({}));
    let schema = json!({ "$ref": "#/$defs/Signal" });
    assert_eq!(schema_to_type_expr(&schema, &root).unwrap(), "Signal");
}

#[test]
fn ref_polysignal_sentinel() {
    let root = root_with_defs(json!({}));
    let schema = json!({ "$ref": "#/$defs/PolySignal" });
    assert_eq!(
        schema_to_type_expr(&schema, &root).unwrap(),
        "Poly<Signal>"
    );
}

#[test]
fn ref_monosignal_sentinel() {
    let root = root_with_defs(json!({}));
    let schema = json!({ "$ref": "#/$defs/MonoSignal" });
    assert_eq!(
        schema_to_type_expr(&schema, &root).unwrap(),
        "Mono<Signal>"
    );
}

#[test]
fn ref_buffer_to_buffer_output_ref() {
    let root = root_with_defs(json!({}));
    let schema = json!({ "$ref": "#/$defs/Buffer" });
    assert_eq!(
        schema_to_type_expr(&schema, &root).unwrap(),
        "BufferOutputRef"
    );
}

#[test]
fn ref_table_sentinel() {
    let root = root_with_defs(json!({}));
    let schema = json!({ "$ref": "#/$defs/Table" });
    assert_eq!(schema_to_type_expr(&schema, &root).unwrap(), "Table");
}

#[test]
fn ref_resolves_via_title() {
    let root = root_with_defs(json!({
        "MyAlias": { "title": "Signal", "type": "object" }
    }));
    let schema = json!({ "$ref": "#/$defs/MyAlias" });
    assert_eq!(schema_to_type_expr(&schema, &root).unwrap(), "Signal");
}

#[test]
fn ref_resolves_to_inline_def() {
    let root = root_with_defs(json!({
        "Mode": {
            "oneOf": [
                { "const": "sum" },
                { "const": "average" }
            ]
        }
    }));
    let schema = json!({ "$ref": "#/$defs/Mode" });
    assert_eq!(
        schema_to_type_expr(&schema, &root).unwrap(),
        r#""sum" | "average""#
    );
}

#[test]
fn enum_array() {
    let schema = json!({ "enum": ["red", "green", "blue"] });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        r#""red" | "green" | "blue""#
    );
}

#[test]
fn one_of_const_variants_form_enum() {
    let schema = json!({
        "oneOf": [
            { "const": "lpf" },
            { "const": "hpf" }
        ]
    });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        r#""lpf" | "hpf""#
    );
}

#[test]
fn one_of_filters_null_for_optional() {
    let schema = json!({
        "anyOf": [
            { "type": "string" },
            { "type": "null" }
        ]
    });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        "string"
    );
}

#[test]
fn one_of_all_signal_collapses_to_poly_signal() {
    let root = root_with_defs(json!({}));
    let schema = json!({
        "oneOf": [
            { "$ref": "#/$defs/Signal" },
            { "$ref": "#/$defs/Signal" }
        ]
    });
    assert_eq!(
        schema_to_type_expr(&schema, &root).unwrap(),
        "Poly<Signal>"
    );
}

#[test]
fn object_with_required_and_optional_props() {
    let schema = json!({
        "type": "object",
        "properties": {
            "freq": { "type": "number" },
            "label": { "type": "string" }
        },
        "required": ["freq"]
    });
    let result = schema_to_type_expr(&schema, &schema).unwrap();
    // Property iteration order matches the JSON object key order
    assert!(result.contains("freq: number"));
    assert!(result.contains("label?: string"));
}

#[test]
fn object_property_with_non_identifier_name_is_quoted() {
    let schema = json!({
        "type": "object",
        "properties": {
            "kebab-case-name": { "type": "number" }
        },
        "required": ["kebab-case-name"]
    });
    let result = schema_to_type_expr(&schema, &schema).unwrap();
    assert!(result.contains(r#""kebab-case-name": number"#));
}

#[test]
fn array_with_items() {
    let schema = json!({
        "type": "array",
        "items": { "type": "number" }
    });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        "number[]"
    );
}

#[test]
fn array_with_prefix_items_is_tuple() {
    let schema = json!({
        "type": "array",
        "prefixItems": [
            { "type": "number" },
            { "type": "string" }
        ]
    });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        "[number, string]"
    );
}

#[test]
fn const_node_serializes_value() {
    let schema = json!({ "const": "foo" });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        r#""foo""#
    );
}

#[test]
fn all_of_falls_back_to_any() {
    let schema = json!({
        "allOf": [{ "type": "string" }]
    });
    assert_eq!(schema_to_type_expr(&schema, &schema).unwrap(), "any");
}

#[test]
fn empty_object_renders_as_empty_braces() {
    let schema = json!({ "type": "object" });
    assert_eq!(
        schema_to_type_expr(&schema, &schema).unwrap(),
        "{}"
    );
}

#[test]
fn enum_variants_extracts_const_descriptions() {
    let schema = json!({
        "oneOf": [
            { "const": "a", "description": "first" },
            { "const": "b" }
        ]
    });
    let variants = get_enum_variants(&schema, &schema).unwrap().unwrap();
    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0].value, r#""a""#);
    assert_eq!(variants[0].description.as_deref(), Some("first"));
    assert_eq!(variants[1].value, r#""b""#);
    assert!(variants[1].description.is_none());
}

#[test]
fn enum_variants_returns_none_for_signal_ref() {
    let root = root_with_defs(json!({}));
    let schema = json!({ "$ref": "#/$defs/Signal" });
    assert!(get_enum_variants(&schema, &root).unwrap().is_none());
}

#[test]
fn enum_variants_handles_bare_enum_array() {
    let schema = json!({ "enum": [1, 2, 3] });
    let variants = get_enum_variants(&schema, &schema).unwrap().unwrap();
    assert_eq!(variants.len(), 3);
    assert_eq!(variants[0].value, "1");
}

#[test]
fn resolved_ref_unsupported_returns_error() {
    use modular_core::codegen::type_resolver::resolve_ref;
    let root = root_with_defs(json!({}));
    assert!(resolve_ref("not-a-defs-ref", &root).is_err());
}

#[test]
fn resolved_ref_missing_def_returns_error() {
    use modular_core::codegen::type_resolver::resolve_ref;
    let root = root_with_defs(json!({}));
    assert!(resolve_ref("#/$defs/NotPresent", &root).is_err());
}

#[test]
fn resolved_ref_returns_schema_for_arbitrary_def() {
    use modular_core::codegen::type_resolver::resolve_ref;
    let root = root_with_defs(json!({
        "Foo": { "type": "object" }
    }));
    let resolved = resolve_ref("#/$defs/Foo", &root).unwrap();
    assert!(matches!(resolved, ResolvedRef::Schema(_)));
}
