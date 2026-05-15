# Module Ref Pattern Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove `module(...)` mini-notation syntax parser-wide, delete broken seq pattern support for signal-backed values, and make existing uses fail early with a clear error.

**Architecture:** The change is a parser-wide language removal rather than a runtime repair. Mini-notation stops producing `AtomValue::ModuleRef`, seq pattern values collapse to voltage/rest only, and the DSL string form becomes explicitly non-mini so accidental stringification no longer generates invalid-but-plausible seq syntax.

**Tech Stack:** Rust, Pest parser, Cargo tests, TypeScript DSL stringification

---

## File Map

- Modify: `crates/modular_core/src/pattern_system/mini/ast.rs`
  - Remove the `AtomValue::ModuleRef` variant and associated comments/helpers
- Modify: `crates/modular_core/src/pattern_system/mini/parser.rs`
  - Remove parser success for `module(...)` and replace it with a direct unsupported-syntax parse error
- Modify: `crates/modular_core/src/dsp/seq/seq_value.rs`
  - Remove `SeqValue::Signal`, `sample_and_hold`, module-ref parsing helpers, and dead signal-collection fields
- Modify: `crates/modular_core/src/dsp/seq/seq.rs`
  - Remove signal-backed pattern-value handling from cached seq haps
- Modify: `crates/modular_core/tests/dsp_fresh_tests.rs`
  - Flip the new seq regression from success to explicit failure
- Modify: `src/main/dsl/GraphBuilder.ts`
  - Change `ModuleOutput::toString()` to a non-mini debug string
- Modify: mini parser / seq tests that currently expect `module(...)` success
  - Update them to expect failure or delete them if they only existed for the removed feature

### Task 1: Remove parser-level `module(...)` syntax

**Files:**
- Modify: `crates/modular_core/src/pattern_system/mini/parser.rs:1870-1911`
- Test: `cargo test -p modular_core test_parse_module_ref -- --nocapture`
- Test: `cargo test -p modular_core test_parse_module_ref_sample_and_hold -- --nocapture`
- Test: `cargo test -p modular_core test_parse_module_ref_in_sequence -- --nocapture`

- [ ] **Step 1: Write the failing parser tests first**

Change the parser tests in `crates/modular_core/src/pattern_system/mini/parser.rs` so they expect `parse(...)` to fail for these inputs instead of succeed:

```rust
#[test]
fn test_parse_module_ref() {
    let err = parse("module(sine-1:sample:0)").expect_err("module refs should be rejected");
    assert!(
        err.to_string().contains("module(...) syntax is no longer supported"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn test_parse_module_ref_sample_and_hold() {
    let err = parse("module(lfo-1:output:0)=").expect_err("sample-and-hold module refs should be rejected");
    assert!(
        err.to_string().contains("module(...) syntax is no longer supported"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn test_parse_module_ref_in_sequence() {
    let err = parse("c4 module(osc:out:0) e4").expect_err("module refs in sequences should be rejected");
    assert!(
        err.to_string().contains("module(...) syntax is no longer supported"),
        "unexpected error: {err:?}"
    );
}
```

- [ ] **Step 2: Run the parser tests to verify RED**

Run: `cargo test -p modular_core test_parse_module_ref -- --nocapture`
Expected: FAIL because parser still accepts `module(...)`

Run: `cargo test -p modular_core test_parse_module_ref_sample_and_hold -- --nocapture`
Expected: FAIL because parser still accepts `module(...)=`

Run: `cargo test -p modular_core test_parse_module_ref_in_sequence -- --nocapture`
Expected: FAIL because parser still accepts module refs inside sequences

- [ ] **Step 3: Remove the parser success path only**

Update `crates/modular_core/src/pattern_system/mini/parser.rs` so `Rule::module_ref` returns a direct parse error instead of `AtomValue::ModuleRef`:

```rust
Rule::module_ref => Err(ParseError {
    message: "module(...) syntax is no longer supported".to_string(),
    span: Some(SourceSpan::new(
        inner.as_span().start(),
        inner.as_span().end(),
    )),
}),
```

- [ ] **Step 4: Run the parser tests to verify GREEN**

Run: `cargo test -p modular_core test_parse_module_ref -- --nocapture`
Expected: PASS

Run: `cargo test -p modular_core test_parse_module_ref_sample_and_hold -- --nocapture`
Expected: PASS

Run: `cargo test -p modular_core test_parse_module_ref_in_sequence -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit checkpoint**

Do not commit in this session. Use the passing parser tests as the checkpoint.

### Task 2: Remove seq support for signal-backed pattern values

**Files:**
- Modify: `crates/modular_core/src/pattern_system/mini/ast.rs:224-264`
- Modify: `crates/modular_core/src/pattern_system/mini/convert.rs:941-975`
- Modify: `crates/modular_core/src/dsp/seq/seq_value.rs:27-399`
- Modify: `crates/modular_core/src/dsp/seq/seq.rs:50-101`
- Test: `cargo test -p modular_core from_graph_seq_module_ref_reads_connected_signal -- --nocapture`
- Test: `cargo test -p modular_core test_from_atom -- --nocapture`
- Test: `cargo test -p modular_core test_parse_module_ref -- --nocapture`

- [ ] **Step 1: Flip the seq integration regression to the new failure mode**

Replace the current success test in `crates/modular_core/tests/dsp_fresh_tests.rs` with a construction-failure test:

```rust
#[test]
fn from_graph_seq_module_ref_pattern_is_rejected() {
    let graph = make_graph(vec![
        ("src", "$signal", serde_json::json!({ "source": 2.5 })),
        ("seq", "$cycle", serde_json::json!({ "pattern": "module(src:output:0)" })),
    ]);

    let err = Patch::from_graph(&graph, SAMPLE_RATE).expect_err("module(...) patterns should be rejected");
    assert!(
        err.contains("module(...) syntax is no longer supported"),
        "unexpected error: {err}"
    );
}
```

- [ ] **Step 2: Run the seq regression to verify RED**

Run: `cargo test -p modular_core from_graph_seq_module_ref_pattern_is_rejected -- --nocapture`
Expected: FAIL because the current assertion/expected error text still reflects the old success behavior and has not yet been updated to the new contract end-to-end

- [ ] **Step 3: Delete `SeqValue::Signal` and module-ref-specific helpers**

Update `crates/modular_core/src/pattern_system/mini/ast.rs` to delete the `ModuleRef` atom variant entirely and remove the corresponding `AtomValue::to_f64()` match arm.

Update `crates/modular_core/src/pattern_system/mini/convert.rs` to delete the `AtomValue::ModuleRef` branch from `atom_to_string(...)`.

Then update `crates/modular_core/src/dsp/seq/seq_value.rs` so `SeqValue` becomes:

```rust
#[derive(Clone, Debug)]
pub enum SeqValue {
    Voltage(f64),
    Rest,
}
```

Then remove the following from the file:

```rust
- sample_and_hold handling
- AtomValue::ModuleRef conversion
- parse_module_ref(...)
- signals: Vec<*mut Signal>
- the `Connect for SeqPatternParam` body that walks signal pointers
```

Replace the `SeqPatternParam` shape with only the fields still used:

```rust
pub struct SeqPatternParam {
    source: String,
    pub(crate) pattern: Option<Pattern<SeqValue>>,
    pub(crate) all_spans: Vec<(usize, usize)>,
    pub(crate) cached_haps: Vec<Arc<Vec<DspHap<SeqValue>>>>,
}
```

and simplify its `unsafe impl Send` comment or remove the impl if the remaining fields derive `Send` cleanly.

- [ ] **Step 4: Remove signal-specific seq runtime branches**

Update `crates/modular_core/src/dsp/seq/seq.rs` so cached hap evaluation only handles voltage/rest:

```rust
fn new(cycle_haps: Arc<Vec<DspHap<SeqValue>>>, hap_index: usize, cached_cycle: i64) -> Self {
    Self {
        cycle_haps,
        hap_index,
        sampled_voltage: None,
        cached_cycle,
    }
}

fn get_cv(&self) -> Option<f64> {
    match &self.hap().value {
        SeqValue::Voltage(v) => Some(*v),
        SeqValue::Rest => None,
    }
}
```

Delete any remaining `SeqValue::Signal` match arms.

- [ ] **Step 5: Update seq-value unit tests to the removed feature set**

In `crates/modular_core/src/dsp/seq/seq_value.rs`, delete or replace tests that explicitly exercise module refs. Keep tests like `test_from_atom()` focused on supported atom types only:

```rust
#[test]
fn test_from_atom() {
    let n = SeqValue::from_atom(&AtomValue::Number(60.0)).unwrap();
    let expected_voltage = midi_to_voct_f64(60.0);
    assert!(matches!(n, SeqValue::Voltage(v) if (v - expected_voltage).abs() < 0.001));

    let note = SeqValue::from_atom(&AtomValue::Note {
        letter: 'a',
        accidental: None,
        octave: Some(4),
    })
    .unwrap();
    let expected_a4_voltage = midi_to_voct_f64(69.0);
    assert!(matches!(note, SeqValue::Voltage(v) if (v - expected_a4_voltage).abs() < 0.001));
}
```

- [ ] **Step 6: Run focused seq tests to verify GREEN**

Run: `cargo test -p modular_core from_graph_seq_module_ref_pattern_is_rejected -- --nocapture`
Expected: PASS

Run: `cargo test -p modular_core test_from_atom -- --nocapture`
Expected: PASS

Run: `cargo test -p modular_core test_parse_module_ref -- --nocapture`
Expected: PASS, because the parser rejection path is now the contract

- [ ] **Step 7: Commit checkpoint**

Do not commit in this session. Use the passing focused seq tests as the checkpoint.

### Task 3: Make DSL stringification explicitly non-mini

**Files:**
- Modify: `src/main/dsl/GraphBuilder.ts:1272-1274`
- Test: `yarn typecheck`

- [ ] **Step 1: Change the failing behavior contract in code**

Update `ModuleOutput::toString()` in `src/main/dsl/GraphBuilder.ts` from:

```ts
toString(): string {
    return `module(${this.moduleId}:${this.portName}:${this.channel})`;
}
```

to:

```ts
toString(): string {
    return `<ModuleOutput ${this.moduleId}:${this.portName}:${this.channel}>`;
}
```

- [ ] **Step 2: Run typecheck to verify no TypeScript callers break**

Run: `yarn typecheck`
Expected: PASS

- [ ] **Step 3: Commit checkpoint**

Do not commit in this session. Use the passing typecheck as the checkpoint.

### Task 4: Run full verification and remove stale assumptions

**Files:**
- Modify: any now-stale parser or seq tests that still mention `module(...)` success
- Test: `cargo test -p modular_core`
- Test: `cargo test -p modular --no-run`
- Test: `yarn typecheck`

- [ ] **Step 1: Remove any remaining stale success assertions for `module(...)`**

Search and update any remaining tests in:

```text
crates/modular_core/src/pattern_system/mini/parser.rs
crates/modular_core/src/dsp/seq/seq_value.rs
crates/modular_core/tests/dsp_fresh_tests.rs
```

The final contract should be:

```text
- parser rejects module(...) directly
- seq pattern construction rejects module(...) patterns
- no live code still converts module refs into SeqValue
```

- [ ] **Step 2: Run the full core suite**

Run: `cargo test -p modular_core`
Expected: PASS with 0 failures

- [ ] **Step 3: Build modular test targets**

Run: `cargo test -p modular --no-run`
Expected: PASS

- [ ] **Step 4: Re-run TypeScript verification**

Run: `yarn typecheck`
Expected: PASS

- [ ] **Step 5: Final checkpoint**

Do not commit in this session. Final checkpoint is the passing verification slice.
