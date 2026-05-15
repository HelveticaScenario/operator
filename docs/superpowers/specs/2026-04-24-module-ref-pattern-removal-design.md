# Module Ref Pattern Removal Design

## Overview

Remove `module(id:port:channel)` and `module(id:port:channel)=` from mini-notation entirely. The feature is only used by seq-pattern code in the live codebase, is already broken at runtime, and keeps unnecessary signal-specific complexity inside generic pattern values.

## Goals

- Remove `module(...)` syntax from the mini-notation language
- Remove seq runtime support for signal-backed pattern values
- Make existing `module(...)` patterns fail early with a clear parse or construction error
- Simplify the path toward making runtime `Signal` values send-only

## Non-Goals

- Redesigning the full pattern system in this slice
- Adding a replacement feature for signal-driven values inside mini notation
- Changing non-pattern cable routing elsewhere in the patch graph
- Preserving backward compatibility for existing `module(...)` patterns

## Selected Approach

Use parser-wide hard removal.

This means:

- Delete `AtomValue::ModuleRef` from `crates/modular_core/src/pattern_system/mini/ast.rs`
- Delete parser support for `module(...)` forms from `crates/modular_core/src/pattern_system/mini/parser.rs`
- Delete seq-side conversion/runtime support in `crates/modular_core/src/dsp/seq/seq_value.rs` and `crates/modular_core/src/dsp/seq/seq.rs`
- Update `src/main/dsl/GraphBuilder.ts` so `ModuleOutput::toString()` no longer emits mini-notation `module(...)`
- Update tests to assert failure instead of success for `module(...)` patterns

## Why This Approach

- The feature is already not working correctly in runtime seq playback
- Parser-wide removal is simpler than keeping dead language syntax around
- Seq is the only live consumer, so removing the syntax does not orphan working generic pattern behavior
- Removing signal-backed pattern values shrinks the remaining reason `Signal` is entangled with pattern-system `Sync` bounds

## Files In Scope

- `crates/modular_core/src/pattern_system/mini/ast.rs`
- `crates/modular_core/src/pattern_system/mini/parser.rs`
- `crates/modular_core/src/dsp/seq/seq_value.rs`
- `crates/modular_core/src/dsp/seq/seq.rs`
- `crates/modular_core/tests/dsp_fresh_tests.rs`
- `src/main/dsl/GraphBuilder.ts`
- Any mini-notation tests that currently expect `module(...)` parsing to succeed

## Current Failure Being Removed

Today `module(...)` parses into `SeqValue::Signal`, but seq pattern caches store cloned unresolved signal values and `SeqPatternParam::connect()` has no collected signals to reconnect. As a result, a pattern like `module(src:output:0)` inside `$cycle` currently reads `0` instead of the source module signal.

Because the feature already fails in runtime behavior, removing it is a cleanup of broken functionality rather than a regression in working behavior.

## Design

### Language Surface

`module(...)` and `module(...)=` stop being valid mini-notation atoms.

Mini-notation continues to support:

- numeric values
- note names
- MIDI / Hz / volts atoms
- rest syntax
- existing sequencing and combinator operators

It no longer supports embedding patch-graph signal references inside pattern strings.

### Parser Behavior

The parser should reject `module(...)` explicitly rather than silently treating it as a generic identifier. Users should see a targeted error that the syntax is no longer supported.

That keeps the failure mode obvious and avoids confusing downstream conversion errors.

### Seq Runtime Simplification

`SeqValue` should only represent values that are self-contained in the parsed pattern.

After this change, seq patterns only need:

- `Voltage(f64)`
- `Rest`

Removing `SeqValue::Signal` also removes the need for:

- `sample_and_hold` handling in seq pattern values
- module-ref parsing helpers in `seq_value.rs`
- signal reads from cached seq haps in `seq.rs`

### DSL Output Strings

`ModuleOutput::toString()` in `src/main/dsl/GraphBuilder.ts` can no longer emit `module(...)`, because that string would be invalid mini notation after this change.

The replacement should be an explicitly non-mini debug form such as `<ModuleOutput id:port:channel>`, so accidental string interpolation no longer produces something that looks like valid seq syntax.

## Error Handling

- Parsing `module(...)` should return a direct unsupported-syntax error
- Seq module construction with patterns containing `module(...)` should fail at parse/conversion time
- Existing non-pattern cable references remain unchanged and keep their current validation behavior

## Testing Strategy

Use TDD for the removal:

1. Flip parser and seq tests so `module(...)` is expected to fail
2. Remove parser/runtime support until those failures become the new GREEN behavior
3. Re-run the fresh verification slice:
   - `cargo test -p modular_core`
   - `cargo test -p modular --no-run`

Required coverage:

- former parser-success tests for `module(...)` become parser-failure tests
- the new seq integration regression should change from “reads connected signal” to “patch construction rejects `module(...)` pattern”
- any TypeScript tests touching `ModuleOutput::toString()` should be updated if present

## Risks

- `ModuleOutput::toString()` may be used in places that implicitly relied on valid mini-notation strings
- Removing parser support may affect docs or examples outside the Rust tests
- If parser rejection is implemented too late in the pipeline, users may get vague conversion errors instead of a clear unsupported-syntax message

## Notes

- This slice is intentionally narrower than a full pattern-system `Sync` cleanup, but it removes one broken feature that currently forces pattern values to carry runtime signal semantics.
- Per prior instruction in this session, this spec is written without creating a git commit.
