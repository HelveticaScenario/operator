//! Peggy ↔ Rust parser parity gate.
//!
//! The production mini-notation parser lives TS-side (Peggy). The Rust
//! crate keeps a thin descent parser in `mini::test_parser` to support
//! existing `#[cfg(test)]` and integration-test fixtures. Drift between
//! the two — even on the small overlapping grammar surface the descent
//! parser claims to handle — is silent failure: TS-built patches
//! continue to work in production while Rust unit tests pass against
//! divergent AST shapes.
//!
//! This test loads `peggy_parser_parity.json` (emitted by the vitest
//! file `peggy_parser_parity_fixture.test.ts` via the
//! `gen:parser-parity-fixture` yarn script) and, for each row, parses
//! the input string through Rust's descent parser, serializes the
//! resulting `MiniAST` through serde, applies the same normalization
//! the TS fixture-emitter applies, and asserts the JSON shapes match.
//!
//! **Normalization** is minimal: zero every `RandomChoice` and
//! `Degrade` seed before comparison. Both parsers assign these
//! depth-first from a monotonic counter, but the order can diverge for
//! deeply-nested constructs (Peggy actions fire top-down, the descent
//! parser increments bottom-up). The seed value is determinism
//! metadata for downstream `Pattern` construction — not part of the
//! syntactic shape we want to compare here.

use std::fs;
use std::path::PathBuf;

use modular_core::pattern_system::mini::test_parser;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct ParityRow {
    label: String,
    input: String,
    peggy_ast: Value,
}

fn fixture_path() -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "src",
        "pattern_system",
        "__fixtures__",
        "peggy_parser_parity.json",
    ]
    .iter()
    .collect()
}

/// Walk a JSON `Value` and apply two canonicalizations:
///
/// 1. Zero every `RandomChoice` / `Degrade` seed. Both parsers assign
///    these depth-first from a monotonic counter, but the order can
///    diverge for deeply-nested constructs. The seed value is
///    determinism metadata for downstream `Pattern` construction, not
///    part of the syntactic shape.
/// 2. Collapse integral-valued floats to integers. `serde_json`
///    serializes `0.0_f64` as `0.0`, while `JSON.stringify` in V8
///    serializes the same value as `0`. Both round-trip to identical
///    `f64`, so we canonicalize by dropping the trivial fractional
///    part. This matches the TS fixture's wire shape.
///
/// Mirrors the TS-side `normalizeSeeds` in
/// `peggy_parser_parity_fixture.test.ts` (TS skips step 2 because
/// JS-side numbers don't carry a representational distinction).
fn normalize(node: &Value) -> Value {
    match node {
        Value::Array(arr) => Value::Array(arr.iter().map(normalize).collect()),
        Value::Object(obj) => {
            // Externally-tagged enum variants serde produces look like
            // single-key objects keyed by the variant name. Special-case
            // the two seed-bearing variants by name; everything else
            // recurses generically.
            if obj.len() == 1 {
                if let Some(payload) = obj.get("RandomChoice") {
                    if let Value::Array(arr) = payload {
                        if arr.len() == 2 {
                            let children = normalize(&arr[0]);
                            let mut out = serde_json::Map::new();
                            out.insert(
                                "RandomChoice".to_string(),
                                Value::Array(vec![children, Value::from(0u64)]),
                            );
                            return Value::Object(out);
                        }
                    }
                }
                if let Some(payload) = obj.get("Degrade") {
                    if let Value::Array(arr) = payload {
                        if arr.len() == 3 {
                            let child = normalize(&arr[0]);
                            let prob = normalize(&arr[1]);
                            let mut out = serde_json::Map::new();
                            out.insert(
                                "Degrade".to_string(),
                                Value::Array(vec![child, prob, Value::from(0u64)]),
                            );
                            return Value::Object(out);
                        }
                    }
                }
            }
            let mut out = serde_json::Map::new();
            for (k, v) in obj {
                out.insert(k.clone(), normalize(v));
            }
            Value::Object(out)
        }
        Value::Number(n) => {
            // Collapse 0.0 → 0, 2.0 → 2. `serde_json::Number` retains
            // the original parse form, but `as_f64` + integer check
            // gives us a unified representation.
            if let Some(f) = n.as_f64() {
                if f.is_finite() && f.fract() == 0.0 && f.abs() < (i64::MAX as f64) {
                    return Value::from(f as i64);
                }
            }
            node.clone()
        }
        _ => node.clone(),
    }
}

#[test]
fn rust_descent_parser_matches_peggy_for_overlapping_grammar() {
    let path = fixture_path();
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing peggy parity fixture at {path:?}: {e}\n\
             regenerate with: yarn gen:parser-parity-fixture",
        )
    });
    let rows: Vec<ParityRow> =
        serde_json::from_str(&text).expect("peggy_parser_parity.json must parse as ParityRow[]");

    assert!(
        rows.len() >= 20,
        "expected at least 20 parity rows in fixture, got {}",
        rows.len(),
    );

    let mut failures: Vec<String> = Vec::new();

    for row in &rows {
        let rust_ast = match test_parser::parse(&row.input) {
            Ok(ast) => ast,
            Err(e) => {
                failures.push(format!(
                    "PARSE ERROR: [{label}] input={input:?} → {e}",
                    label = row.label,
                    input = row.input,
                ));
                continue;
            }
        };
        let rust_json = serde_json::to_value(&rust_ast).expect("MiniAST must serialize to JSON");
        let rust_canonical = normalize(&rust_json);
        let peggy_canonical = normalize(&row.peggy_ast);

        if rust_canonical != peggy_canonical {
            failures.push(format!(
                "AST DIVERGENCE: [{label}] input={input:?}\n  peggy: {peggy}\n  rust:  {rust}",
                label = row.label,
                input = row.input,
                peggy = serde_json::to_string(&peggy_canonical).unwrap_or_default(),
                rust = serde_json::to_string(&rust_canonical).unwrap_or_default(),
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{n} of {total} parity rows diverged between Peggy and Rust parsers:\n\n{body}",
        n = failures.len(),
        total = rows.len(),
        body = failures.join("\n\n"),
    );
}
