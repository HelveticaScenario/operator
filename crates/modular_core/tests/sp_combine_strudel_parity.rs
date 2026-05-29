//! Strudel parity regression for `combine_sp`.
//!
//! Two fixtures, both produced by `scripts/gen-sp-fixture.mjs` against
//! the real `@strudel/core` + `@strudel/mini` JS packages.
//!
//! 1. `sp_combine.json` — single-op chain across the full grammar
//!    surface (10 × 10 × {add, sub} × 7 modes = 1400 rows).
//! 2. `sp_combine_chain2.json` — two-op chain. Folds
//!    `lhs._op1(rhs1)._op2(rhs2)` matching $sp(...).add(rhs1).add(rhs2)
//!    style chains (4 × 4 × 4 × 2 × 7 × 2 × 7 = 12 544 rows).
//!
//! For every row: parse sources via `mini::convert::<IntervalValue>`,
//! fold the chain through `combine_sp`, query the first cycle, and
//! assert (whole, part, value) parity hap-for-hap as a multiset.
//! `IntervalValue::Rest` haps are filtered before compare — strudel's
//! mini-notation emits no gap haps, so the structural mismatch is by
//! design.

use std::fs;
use std::path::PathBuf;

use modular_core::dsp::seq::IntervalValue;
use modular_core::pattern_system::sp_combine::{SpAlignmentMode, combine_sp};
use modular_core::pattern_system::{Fraction, Pattern, mini};
use serde::Deserialize;

fn add_iv(a: &IntervalValue, b: &IntervalValue) -> IntervalValue {
    match (a.degree(), b.degree()) {
        (Some(da), Some(db)) => IntervalValue::Degree(da + db),
        _ => IntervalValue::Rest,
    }
}

fn sub_iv(a: &IntervalValue, b: &IntervalValue) -> IntervalValue {
    match (a.degree(), b.degree()) {
        (Some(da), Some(db)) => IntervalValue::Degree(da - db),
        _ => IntervalValue::Rest,
    }
}

#[derive(Debug, Deserialize)]
struct FixtureHap {
    whole: Option<[[i64; 2]; 2]>,
    part: [[i64; 2]; 2],
    value: f64,
}

/// Normalized hap shape for comparison.
type CmpHap = (Option<[[i64; 2]; 2]>, [[i64; 2]; 2], i32);

fn parse_mode(s: &str) -> SpAlignmentMode {
    match s {
        "in" => SpAlignmentMode::In,
        "out" => SpAlignmentMode::Out,
        "mix" => SpAlignmentMode::Mix,
        "squeeze" => SpAlignmentMode::Squeeze,
        "squeezeout" => SpAlignmentMode::SqueezeOut,
        "reset" => SpAlignmentMode::Reset,
        "restart" => SpAlignmentMode::Restart,
        other => panic!("unknown mode: {other}"),
    }
}

fn parse_pattern(source: &str) -> Pattern<IntervalValue> {
    let ast = mini::parse_ast(source)
        .unwrap_or_else(|e| panic!("parse error for `{source}`: {e:?}"));
    mini::convert::<IntervalValue>(&ast)
        .unwrap_or_else(|e| panic!("convert error for `{source}`: {e:?}"))
}

fn apply_op(
    lhs: &Pattern<IntervalValue>,
    rhs: &Pattern<IntervalValue>,
    op: &str,
    mode: SpAlignmentMode,
) -> Pattern<IntervalValue> {
    match op {
        "add" => combine_sp(lhs, rhs, mode, add_iv),
        "sub" => combine_sp(lhs, rhs, mode, sub_iv),
        other => panic!("unknown op: {other}"),
    }
}

fn frac_pair(f: &Fraction) -> [i64; 2] {
    [f.numer(), f.denom()]
}

fn sort_key(h: &CmpHap) -> ([[i64; 2]; 2], i32, Option<[[i64; 2]; 2]>) {
    (h.1, h.2, h.0)
}

fn extract_actual(pat: &Pattern<IntervalValue>) -> Vec<CmpHap> {
    let haps =
        pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
    let mut out: Vec<CmpHap> = haps
        .iter()
        .filter_map(|h| {
            let d = h.value.degree()?;
            let whole = h
                .whole
                .as_ref()
                .map(|w| [frac_pair(&w.begin), frac_pair(&w.end)]);
            let part = [frac_pair(&h.part.begin), frac_pair(&h.part.end)];
            Some((whole, part, d))
        })
        .collect();
    out.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    out
}

fn extract_expected(haps: &[FixtureHap]) -> Vec<CmpHap> {
    let mut out: Vec<CmpHap> =
        haps.iter().map(|h| (h.whole, h.part, h.value as i32)).collect();
    out.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    out
}

fn fixture_path(name: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "src",
        "pattern_system",
        "__fixtures__",
        name,
    ]
    .iter()
    .collect()
}

// ─── 1-op chain (full grammar surface) ─────────────────────────────────

#[derive(Debug, Deserialize)]
struct Row1 {
    lhs: String,
    lhs_source: String,
    rhs: String,
    rhs_source: String,
    op: String,
    mode: String,
    #[serde(default)]
    haps: Option<Vec<FixtureHap>>,
    #[serde(default)]
    error: Option<String>,
}

#[test]
fn combine_sp_matches_strudel_for_full_grammar_cross() {
    let path = fixture_path("sp_combine.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing fixture at {path:?}: {e}"));
    let rows: Vec<Row1> =
        serde_json::from_str(&text).expect("fixture must parse as Row1[]");

    let mut failures: Vec<String> = Vec::new();
    let mut compared = 0usize;

    for row in &rows {
        if row.error.is_some() {
            continue;
        }
        let Some(expected) = row.haps.as_ref() else {
            continue;
        };
        let mode = parse_mode(&row.mode);
        let lhs = parse_pattern(&row.lhs_source);
        let rhs = parse_pattern(&row.rhs_source);
        let combined = apply_op(&lhs, &rhs, &row.op, mode);
        let actual = extract_actual(&combined);
        let expected_cmp = extract_expected(expected);

        compared += 1;
        if actual != expected_cmp {
            failures.push(format!(
                "DIVERGENCE: {lhs}({lhs_src:?}) op={op} mode={mode_s} rhs={rhs}({rhs_src:?})\n  expected ({el}): {exp:?}\n  actual   ({al}): {act:?}",
                lhs = row.lhs,
                lhs_src = row.lhs_source,
                rhs = row.rhs,
                rhs_src = row.rhs_source,
                op = row.op,
                mode_s = row.mode,
                el = expected_cmp.len(),
                al = actual.len(),
                exp = expected_cmp,
                act = actual,
            ));
            if failures.len() >= 25 {
                break;
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{n} of {total} rows diverged from strudel:\n{joined}",
            n = failures.len(),
            total = compared,
            joined = failures.join("\n\n"),
        );
    }
    assert!(
        compared >= 1400,
        "expected at least 1400 strudel parity rows, only ran {compared}"
    );
}

// ─── 2-op chain (reduced grammar surface) ──────────────────────────────

#[derive(Debug, Deserialize)]
struct Row2 {
    lhs: String,
    lhs_source: String,
    rhs1: String,
    rhs1_source: String,
    rhs2: String,
    rhs2_source: String,
    op1: String,
    mode1: String,
    op2: Option<String>,
    mode2: Option<String>,
    #[serde(default)]
    haps: Option<Vec<FixtureHap>>,
    #[serde(default)]
    error: Option<String>,
}

#[test]
fn combine_sp_chain2_matches_strudel() {
    let path = fixture_path("sp_combine_chain2.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing fixture at {path:?}: {e}"));
    let rows: Vec<Row2> =
        serde_json::from_str(&text).expect("fixture must parse as Row2[]");

    let mut failures: Vec<String> = Vec::new();
    let mut compared = 0usize;

    for row in &rows {
        if row.error.is_some() {
            continue;
        }
        let Some(expected) = row.haps.as_ref() else {
            continue;
        };
        let mode1 = parse_mode(&row.mode1);
        let mode2 = parse_mode(row.mode2.as_deref().unwrap_or_else(|| {
            panic!("non-error row missing mode2: {row:?}")
        }));
        let op2 = row.op2.as_deref().unwrap_or_else(|| {
            panic!("non-error row missing op2: {row:?}")
        });

        let lhs = parse_pattern(&row.lhs_source);
        let rhs1 = parse_pattern(&row.rhs1_source);
        let rhs2 = parse_pattern(&row.rhs2_source);

        let step1 = apply_op(&lhs, &rhs1, &row.op1, mode1);
        let step2 = apply_op(&step1, &rhs2, op2, mode2);
        let actual = extract_actual(&step2);
        let expected_cmp = extract_expected(expected);

        compared += 1;
        if actual != expected_cmp {
            failures.push(format!(
                "DIVERGENCE: {lhs}.{op1}.{mode1}({rhs1}).{op2}.{mode2}({rhs2})\n  expected ({el}): {exp:?}\n  actual   ({al}): {act:?}",
                lhs = row.lhs,
                op1 = row.op1,
                mode1 = row.mode1,
                rhs1 = row.rhs1,
                op2 = op2,
                mode2 = row.mode2.as_deref().unwrap(),
                rhs2 = row.rhs2,
                el = expected_cmp.len(),
                al = actual.len(),
                exp = expected_cmp,
                act = actual,
            ));
            if failures.len() >= 25 {
                break;
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{n} of {total} chain-2 rows diverged from strudel:\n{joined}",
            n = failures.len(),
            total = compared,
            joined = failures.join("\n\n"),
        );
    }
    assert!(
        compared >= 12_544,
        "expected at least 12544 chain-2 parity rows, only ran {compared}"
    );
}
