/**
 * TypeScript mirror of the Rust `MiniAST` type family in
 * `crates/modular_core/src/pattern_system/mini/ast.rs`.
 *
 * The JSON shape matches serde's default (externally-tagged) representation
 * so the Rust side can `Deserialize` these objects directly from the
 * `$cycle` / `$p.s` params carried in the patch graph.
 *
 * Tuple variants like `Pure(Located<AtomValue>)` serialize as
 * `{ "Pure": { "node": ..., "span": ... } }`. Struct variants like
 * `Euclidean { pattern, pulses, steps, rotation }` serialize as
 * `{ "Euclidean": { "pattern": ..., "pulses": ..., ... } }`. Enum variants
 * with multiple positional fields serialize as a tuple (array) payload, so
 * `Fast(Box<MiniAST>, Box<MiniASTF64>)` becomes `{ "Fast": [ast, factor] }`.
 *
 * `Sequence(Vec<(MiniAST, Option<Weight>)>)` serializes as
 * `{ "Sequence": [[child, weight], ...] }`, where the weight slot is
 * untagged on the Rust side: a static weight is a bare number (or null),
 * a patterned weight (`a@<1 2>`) is a `MiniASTF64` object. Replicate counts
 * follow the same untagged scheme with `MiniASTU32`.
 */

/** Source location in the original pattern string, half-open `[start, end)`. */
export interface SourceSpan {
    start: number;
    end: number;
}

/** A value with an associated source location. */
export interface Located<T> {
    node: T;
    span: SourceSpan;
}

/** Atomic value types that survive `$p()` parsing. */
export type AtomValue =
    | { Number: number }
    | { Hz: number }
    | {
          Note: {
              /** Single lowercase letter 'a'..'g'. */
              letter: string;
              /** '#' for sharp, 'b' for flat, or null. */
              accidental: string | null;
              /** Octave integer or null when not specified. */
              octave: number | null;
          };
      }
    /**
     * The `x` structure marker (serde unit variant, so a bare string). Only
     * meaningful in `.struct(...)` boolean patterns.
     */
    | 'Truthy';

/** `@` weight on a sequence entry: static number or pattern operand. */
export type Weight = number | MiniASTF64;

/** `!` replicate count: static number or pattern operand. */
export type ReplicateCount = number | MiniASTU32;

/** Top-level AST node. */
export type MiniAST =
    | { Pure: Located<AtomValue> }
    | { Rest: SourceSpan }
    | { List: Located<MiniAST[]> }
    | { Sequence: Array<[MiniAST, Weight | null]> }
    | { FastCat: Array<[MiniAST, Weight | null]> }
    | { SlowCat: Array<[MiniAST, Weight | null]> }
    | { Stack: MiniAST[] }
    | { RandomChoice: [MiniAST[], number] }
    | { Fast: [MiniAST, MiniASTF64] }
    | { Slow: [MiniAST, MiniASTF64] }
    | { Replicate: [MiniAST, ReplicateCount] }
    | { Degrade: [MiniAST, number | null, number] }
    | {
          Euclidean: {
              pattern: MiniAST;
              pulses: MiniASTU32;
              steps: MiniASTU32;
              rotation: MiniASTI32 | null;
          };
      }
    | {
          Polymeter: {
              children: MiniAST[];
              steps_per_cycle: MiniASTF64 | null;
          };
      };

/** AST specialized for `f64`-valued modifier arguments (fast/slow factors). */
export type MiniASTF64 =
    | { Pure: Located<number> }
    | { Rest: SourceSpan }
    | { List: Located<MiniASTF64[]> }
    | { Sequence: Array<[MiniASTF64, Weight | null]> }
    | { FastCat: Array<[MiniASTF64, Weight | null]> }
    | { SlowCat: Array<[MiniASTF64, Weight | null]> }
    | { Stack: MiniASTF64[] }
    | { RandomChoice: [MiniASTF64[], number] }
    | { Fast: [MiniASTF64, MiniASTF64] }
    | { Slow: [MiniASTF64, MiniASTF64] }
    | { Replicate: [MiniASTF64, ReplicateCount] }
    | { Degrade: [MiniASTF64, number | null, number] }
    | {
          Euclidean: {
              pattern: MiniASTF64;
              pulses: MiniASTU32;
              steps: MiniASTU32;
              rotation: MiniASTI32 | null;
          };
      }
    | {
          Polymeter: {
              children: MiniASTF64[];
              steps_per_cycle: MiniASTF64 | null;
          };
      };

/** AST specialized for `u32`-valued modifier arguments (euclidean pulses/steps). */
export type MiniASTU32 =
    | { Pure: Located<number> }
    | { Rest: SourceSpan }
    | { List: Located<MiniASTU32[]> }
    | { Sequence: Array<[MiniASTU32, Weight | null]> }
    | { FastCat: Array<[MiniASTU32, Weight | null]> }
    | { SlowCat: Array<[MiniASTU32, Weight | null]> }
    | { Stack: MiniASTU32[] }
    | { RandomChoice: [MiniASTU32[], number] }
    | { Fast: [MiniASTU32, MiniASTF64] }
    | { Slow: [MiniASTU32, MiniASTF64] }
    | { Replicate: [MiniASTU32, ReplicateCount] }
    | { Degrade: [MiniASTU32, number | null, number] }
    | {
          Euclidean: {
              pattern: MiniASTU32;
              pulses: MiniASTU32;
              steps: MiniASTU32;
              rotation: MiniASTI32 | null;
          };
      }
    | {
          Polymeter: {
              children: MiniASTU32[];
              steps_per_cycle: MiniASTF64 | null;
          };
      };

/** AST specialized for `i32`-valued modifier arguments (euclidean rotation). */
export type MiniASTI32 =
    | { Pure: Located<number> }
    | { Rest: SourceSpan }
    | { List: Located<MiniASTI32[]> }
    | { Sequence: Array<[MiniASTI32, Weight | null]> }
    | { FastCat: Array<[MiniASTI32, Weight | null]> }
    | { SlowCat: Array<[MiniASTI32, Weight | null]> }
    | { Stack: MiniASTI32[] }
    | { RandomChoice: [MiniASTI32[], number] }
    | { Fast: [MiniASTI32, MiniASTF64] }
    | { Slow: [MiniASTI32, MiniASTF64] }
    | { Replicate: [MiniASTI32, ReplicateCount] }
    | { Degrade: [MiniASTI32, number | null, number] }
    | {
          Euclidean: {
              pattern: MiniASTI32;
              pulses: MiniASTU32;
              steps: MiniASTU32;
              rotation: MiniASTI32 | null;
          };
      }
    | {
          Polymeter: {
              children: MiniASTI32[];
              steps_per_cycle: MiniASTF64 | null;
          };
      };

/** `$p()` output — a parsed pattern with source + pre-computed leaf spans. */
export interface ParsedPattern {
    __kind: 'ParsedPattern';
    ast: MiniAST;
    source: string;
    all_spans: [number, number][];
    /**
     * Document span of the literal `$p()` parsed (outer quotes included).
     * Module factories receiving this `ParsedPattern` use this span as the
     * `argument_spans` entry for whatever param the pattern was passed as,
     * so pattern highlighting stays attached to the original `$p(...)` call
     * site regardless of how many `const` indirections sit between the two.
     * `undefined` when `$p()` was called outside a tracked DSL source file.
     */
    argument_span?: { start: number; end: number };
}
