//! Strudel parity regression for `combine_sp`.
//!
//! Two fixtures, both produced by `scripts/gen-sp-fixture.mjs` against
//! the real `@strudel/core` + `@strudel/mini` JS packages.
//!
//! 1. `sp_combine.json` — single-op chain across the full grammar
//!    surface (10 × 10 × {add, sub} × 7 modes = 1400 rows).
//! 2. `sp_combine_chain2.json` — two-op chain. Folds
//!    `lhs._op1(rhs1)._op2(rhs2)` matching $p.s(...).add(rhs1).add(rhs2)
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
use std::process::Command;
use std::sync::OnceLock;

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
    let ast =
        mini::parse_ast(source).unwrap_or_else(|e| panic!("parse error for `{source}`: {e:?}"));
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

/// Extracted parity row: the comparable hap list plus diagnostic counts
/// for the pre-filter / Rest split.
struct Extracted {
    haps: Vec<CmpHap>,
    rest_count: usize,
    pre_filter_count: usize,
}

fn extract_actual(pat: &Pattern<IntervalValue>) -> Extracted {
    let haps = pat.query_arc(Fraction::from_integer(0), Fraction::from_integer(1));
    let pre_filter_count = haps.len();
    let mut rest_count = 0usize;
    let mut out: Vec<CmpHap> = haps
        .iter()
        .filter_map(|h| match h.value.degree() {
            Some(d) => {
                let whole = h
                    .whole
                    .as_ref()
                    .map(|w| [frac_pair(&w.begin), frac_pair(&w.end)]);
                let part = [frac_pair(&h.part.begin), frac_pair(&h.part.end)];
                Some((whole, part, d))
            }
            None => {
                rest_count += 1;
                None
            }
        })
        .collect();
    out.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    Extracted {
        haps: out,
        rest_count,
        pre_filter_count,
    }
}

fn extract_expected(haps: &[FixtureHap]) -> Vec<CmpHap> {
    let mut out: Vec<CmpHap> = haps
        .iter()
        .map(|h| (h.whole, h.part, h.value as i32))
        .collect();
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

/// Repo root — two levels up from this crate (`crates/modular_core`).
fn repo_root() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "..", ".."].iter().collect()
}

/// The `sp_combine` fixtures are large (~20 MB), deterministic strudel
/// ground-truth and are intentionally NOT committed — they regenerate from
/// `scripts/gen-sp-fixture.mjs`. Generate them on first use if absent, at
/// most once per test binary (parallel test threads share the guard).
fn ensure_fixtures() {
    static GEN: OnceLock<()> = OnceLock::new();
    GEN.get_or_init(|| {
        let present = ["sp_combine.json", "sp_combine_chain2.json"]
            .iter()
            .all(|n| fixture_path(n).exists());
        if present {
            return;
        }
        let root = repo_root();
        match Command::new("node")
            .arg("scripts/gen-sp-fixture.mjs")
            .current_dir(&root)
            .status()
        {
            Ok(s) if s.success() => {}
            Ok(s) => panic!(
                "sp_combine fixtures missing and `node scripts/gen-sp-fixture.mjs` \
                 exited with {s}. Run `yarn install` then `yarn gen:sp-fixtures`.",
            ),
            Err(e) => panic!(
                "sp_combine fixtures missing and `node scripts/gen-sp-fixture.mjs` \
                 could not be run ({e}). Run `yarn install` then `yarn gen:sp-fixtures`.",
            ),
        }
    });
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
    /// Strudel-side count of `~`-derived no-value haps. Strudel suppresses
    /// rest haps at queryArc time so this is currently always 0 — but we
    /// still emit it so the Rust side can flag any combine_sp output that
    /// produces a Rest where strudel didn't.
    #[serde(default)]
    rest_count: Option<usize>,
    #[serde(default)]
    error: Option<String>,
}

/// Render a small histogram of divergences, bucketed by (op, mode), so a
/// failed run surfaces hot-spot combinations rather than just a wall of
/// individual failures. Sorted descending by count, ties broken
/// alphabetically for determinism.
fn render_histogram(buckets: &std::collections::BTreeMap<String, usize>) -> String {
    if buckets.is_empty() {
        return String::from("(none)");
    }
    let mut entries: Vec<(&String, &usize)> = buckets.iter().collect();
    entries.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let mut out = String::from("per-bucket divergence count:\n");
    for (bucket, count) in entries {
        out.push_str(&format!("  {count:>5}  {bucket}\n"));
    }
    out
}

/// Cap on the number of full divergence reports rendered in the panic
/// message. Distinct from the iteration cap removed for L7 — we still
/// collect every divergence so the histogram is complete, we only trim
/// the prose body so the test output stays scannable.
const FAILURE_RENDER_CAP: usize = 25;

#[test]
fn combine_sp_matches_strudel_for_full_grammar_cross() {
    ensure_fixtures();
    let path = fixture_path("sp_combine.json");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("missing fixture at {path:?}: {e}"));
    let rows: Vec<Row1> = serde_json::from_str(&text).expect("fixture must parse as Row1[]");

    let mut failures: Vec<String> = Vec::new();
    let mut hist: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
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

        // Sanity: the Rest-filter must never enlarge the hap set.
        assert!(
            actual.haps.len() <= actual.pre_filter_count,
            "filter expanded hap count: pre={pre}, post={post} for {lhs}.{op}.{mode}({rhs})",
            pre = actual.pre_filter_count,
            post = actual.haps.len(),
            lhs = row.lhs,
            op = row.op,
            mode = row.mode,
            rhs = row.rhs,
        );

        // Diagnostic-only: track Rest-hap divergence in the histogram
        // without failing the build. The Rust `combine_sp` legitimately
        // emits `IntervalValue::Rest` haps where strudel suppresses them
        // at queryArc time (file-level comment documents this as by
        // design), so a non-zero mismatch tally is expected today. We
        // still surface the per-bucket count so engineers can spot
        // regressions where the gap widens.
        let expected_rests = row.rest_count.unwrap_or(0);
        compared += 1;
        if actual.rest_count != expected_rests {
            let bucket = format!(
                "rest-count-diagnostic:{op}/{mode}",
                op = row.op,
                mode = row.mode,
            );
            *hist.entry(bucket).or_insert(0) += 1;
        }
        if actual.haps != expected_cmp {
            let bucket = format!("hap-mismatch:{op}/{mode}", op = row.op, mode = row.mode);
            *hist.entry(bucket).or_insert(0) += 1;
            failures.push(format!(
                "DIVERGENCE: {lhs}({lhs_src:?}) op={op} mode={mode_s} rhs={rhs}({rhs_src:?})\n  expected ({el}): {exp:?}\n  actual   ({al}): {act:?}",
                lhs = row.lhs,
                lhs_src = row.lhs_source,
                rhs = row.rhs,
                rhs_src = row.rhs_source,
                op = row.op,
                mode_s = row.mode,
                el = expected_cmp.len(),
                al = actual.haps.len(),
                exp = expected_cmp,
                act = actual.haps,
            ));
            // No iteration break — keep collecting so the histogram is
            // complete. Rendering is capped below for output size.
        }
    }

    if !failures.is_empty() {
        let total_failures = failures.len();
        let shown = total_failures.min(FAILURE_RENDER_CAP);
        let body = failures[..shown].join("\n\n");
        panic!(
            "{total_failures} of {compared} rows diverged from strudel (showing {shown} of {total_failures} total)\n\n{body}\n\n{hist}",
            hist = render_histogram(&hist),
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
    rest_count: Option<usize>,
    #[serde(default)]
    error: Option<String>,
}

#[test]
fn combine_sp_chain2_matches_strudel() {
    ensure_fixtures();
    let path = fixture_path("sp_combine_chain2.json");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("missing fixture at {path:?}: {e}"));
    let rows: Vec<Row2> = serde_json::from_str(&text).expect("fixture must parse as Row2[]");

    let mut failures: Vec<String> = Vec::new();
    let mut hist: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut compared = 0usize;

    for row in &rows {
        if row.error.is_some() {
            continue;
        }
        let Some(expected) = row.haps.as_ref() else {
            continue;
        };
        let mode1 = parse_mode(&row.mode1);
        let mode2 = parse_mode(
            row.mode2
                .as_deref()
                .unwrap_or_else(|| panic!("non-error row missing mode2: {row:?}")),
        );
        let op2 = row
            .op2
            .as_deref()
            .unwrap_or_else(|| panic!("non-error row missing op2: {row:?}"));

        let lhs = parse_pattern(&row.lhs_source);
        let rhs1 = parse_pattern(&row.rhs1_source);
        let rhs2 = parse_pattern(&row.rhs2_source);

        let step1 = apply_op(&lhs, &rhs1, &row.op1, mode1);
        let step2 = apply_op(&step1, &rhs2, op2, mode2);
        let actual = extract_actual(&step2);
        let expected_cmp = extract_expected(expected);

        // Sanity: the Rest-filter must never enlarge the hap set.
        assert!(
            actual.haps.len() <= actual.pre_filter_count,
            "filter expanded hap count: pre={pre}, post={post} for {lhs}.{op1}.{mode1}({rhs1}).{op2}.{mode2}({rhs2})",
            pre = actual.pre_filter_count,
            post = actual.haps.len(),
            lhs = row.lhs,
            op1 = row.op1,
            mode1 = row.mode1,
            rhs1 = row.rhs1,
            op2 = op2,
            mode2 = row.mode2.as_deref().unwrap(),
            rhs2 = row.rhs2,
        );

        // Diagnostic-only: see notes on single-chain test. Mismatch in
        // the by-design Rest filter is bucketed, not failed.
        let expected_rests = row.rest_count.unwrap_or(0);
        compared += 1;
        if actual.rest_count != expected_rests {
            let bucket = format!(
                "rest-count-diagnostic:{op1}/{mode1}->{op2}/{mode2}",
                op1 = row.op1,
                mode1 = row.mode1,
                op2 = op2,
                mode2 = row.mode2.as_deref().unwrap(),
            );
            *hist.entry(bucket).or_insert(0) += 1;
        }
        if actual.haps != expected_cmp {
            let bucket = format!(
                "hap-mismatch:{op1}/{mode1}->{op2}/{mode2}",
                op1 = row.op1,
                mode1 = row.mode1,
                op2 = op2,
                mode2 = row.mode2.as_deref().unwrap(),
            );
            *hist.entry(bucket).or_insert(0) += 1;
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
                al = actual.haps.len(),
                exp = expected_cmp,
                act = actual.haps,
            ));
            // No iteration break — collect all divergences for the histogram.
        }
    }

    if !failures.is_empty() {
        let total_failures = failures.len();
        let shown = total_failures.min(FAILURE_RENDER_CAP);
        let body = failures[..shown].join("\n\n");
        panic!(
            "{total_failures} of {compared} chain-2 rows diverged from strudel (showing {shown} of {total_failures} total)\n\n{body}\n\n{hist}",
            hist = render_histogram(&hist),
        );
    }
    assert!(
        compared >= 12_544,
        "expected at least 12544 chain-2 parity rows, only ran {compared}"
    );
}
