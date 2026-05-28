/*
 * Mini notation grammar for the Operator DSL.
 *
 * Adapted structurally from strudel's packages/mini/krill.pegjs:
 *   Copyright (C) 2022 Strudel contributors
 *   https://codeberg.org/uzu/strudel/src/branch/main/packages/mini/krill.pegjs
 *   SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This file inherits the AGPL-3.0-or-later license from krill.pegjs. The rest
 * of the repository is licensed separately.
 *
 * Differences from krill.pegjs:
 *   - Emits the shared `MiniAST` shape directly (no {type_, source_, options_}
 *     intermediate wrappers).
 *   - Atom kinds reduced to Number, Hz, Note, Rest. No sample-name identifiers,
 *     MIDI shorthand, module-reference atoms, string atoms, or voltage atoms.
 *   - Modifier operands reuse the same AST typing as the Rust parser
 *     (MiniASTF64 for fast/slow factors and degrade probabilities,
 *     MiniASTU32 for euclidean pulses/steps, MiniASTI32 for rotation).
 *   - No polymeter, $-operator chain, struct/target/scale/etc. high-level
 *     statements, no `cat(...)` / `slow(...)` / `fast(...)` commands.
 *
 * Seed assignment for RandomChoice / Degrade nodes is left-to-right,
 * depth-first: matches Rust's `assign_seeds` pass so equivalent pattern
 * strings produce identical ASTs across languages.
 */
{{
  function located(node, loc) {
    return { node: node, span: { start: loc.start.offset, end: loc.end.offset } };
  }

  function spanOf(loc) {
    return { start: loc.start.offset, end: loc.end.offset };
  }

  // Apply a chain of modifier builders to a base AST, threading a seed counter.
  // Each modifier is a function `(ast, ctx) => ast` where `ctx` exposes `nextSeed()`.
  function applyMods(base, mods, ctx) {
    let ast = base;
    for (const m of mods) {
      ast = m(ast, ctx);
    }
    return ast;
  }

  // Collapse elongation markers (`_`) into the preceding sequence entry's
  // weight. Entries are tagged `{kind:'element', entry:[ast, weight]}` or
  // `{kind:'elongation'}`. Returns a plain `[ast, weight]` array suitable for
  // a `Sequence` / `FastCat` / `SlowCat` node.
  function applyElongation(entries) {
    const out = [];
    for (const entry of entries) {
      if (entry.kind === 'elongation') {
        if (out.length === 0) {
          throw new Error("leading `_` has no element to elongate");
        }
        const last = out[out.length - 1];
        last[1] = (last[1] ?? 1) + 1;
        continue;
      }
      out.push(entry.entry);
    }
    return out;
  }
}}

{
  // Seed counter: incremented in source order for `?` (Degrade) and `|`
  // (RandomChoice) nodes. Matches Rust's assign_seeds behaviour.
  const seedState = { next: 0 };
  const ctx = {
    nextSeed() {
      const s = seedState.next;
      seedState.next += 1;
      return s;
    },
  };
}

// ---------------------------------------------------------------------------
// Top-level
// ---------------------------------------------------------------------------

Start
  = _ s:StackExpr _ { return s; }

// Stack: comma-separated patterns play simultaneously.
// Each element may itself be a feet-separated chain (`a b . c d`) which
// fastcats its dot-separated sub-sequences as nested elements.
StackExpr
  = head:FeetExpr tail:(_ "," _ @FeetExpr)* {
      if (tail.length === 0) return head;
      return { Stack: [head, ...tail] };
    }

// Feet separator: `a b . c d` = sequence of [a b] then [c d], each taking
// half the cycle. Matches strudel's `dot_tail` → `feet` alignment.
FeetExpr
  = head:SequenceExpr tail:(_ "." _ @SequenceExpr)* {
      if (tail.length === 0) return head;
      return { Sequence: [head, ...tail].map(s => [s, null]) };
    }

// Sequence: space-separated elements. Bare `_` tokens after an element act
// as elongation markers, each bumping the preceding entry's weight by one
// (so `a _ _` is weight 3, same as `a@3`).
SequenceExpr
  = elems:(_ @SequenceEntry)+ {
      const out = applyElongation(elems);
      if (out.length === 1 && out[0][1] === null) {
        return out[0][0];
      }
      return { Sequence: out };
    }

SequenceEntry
  = "_" !IdentCont { return { kind: 'elongation' }; }
  / e:ElementWithWeight { return { kind: 'element', entry: e }; }

// Element with optional @weight and modifier chain. Weight may appear
// anywhere in the modifier list (matching Rust's grammar.pest), but we
// treat it purely as positional metadata on the sequence entry; non-weight
// modifiers wrap the AST in order.
ElementWithWeight
  = base:ElementBase parts:ElementPart* {
      let ast = base;
      let weight = null;
      for (const part of parts) {
        if (part.kind === 'weight') {
          // Last-wins if duplicated (matches Rust's single-slot behaviour).
          weight = part.value;
        } else {
          ast = part.apply(ast, ctx);
        }
      }
      return [ast, weight];
    }

ElementPart
  = w:Weight { return { kind: 'weight', value: w }; }
  / m:Modifier { return { kind: 'mod', apply: m }; }

ElementBase
  = FastSub
  / SlowSub
  / Polymeter
  / Atom

// Polymeter: `{a b, c d e}` with optional `%n` steps-per-cycle override.
// Each comma-separated child sequence gets stacked, scaled so its step
// count maps to `steps_per_cycle`. Matches strudel's `polymeter`.
Polymeter
  = "{" _ head:FeetExpr tail:(_ "," _ @FeetExpr)* _ "}" steps:PolymeterSteps? {
      const children = [head, ...tail];
      return { Polymeter: { children, steps_per_cycle: steps } };
    }

PolymeterSteps
  = "%" op:ModOperandF64 { return op; }

FastSub
  = "[" _ s:StackExpr _ "]" {
      // Keep `[...]` as FastCat (explicit fast subsequence) rather than
      // letting StackExpr flatten. When `s` is a Stack, wrap it as a
      // single-element FastCat so nesting semantics match the Rust parser.
      if (s && s.Stack) {
        return { FastCat: [[s, null]] };
      }
      if (s && s.Sequence) {
        return { FastCat: s.Sequence };
      }
      // Single element: wrap in FastCat with one entry.
      return { FastCat: [[s, null]] };
    }

SlowSub
  = "<" _ s:StackExpr _ ">" {
      if (s && s.Stack) {
        return { SlowCat: [[s, null]] };
      }
      if (s && s.Sequence) {
        return { SlowCat: s.Sequence };
      }
      return { SlowCat: [[s, null]] };
    }

// Atoms: random-choice group, a rest, or a concrete value.
Atom
  = RandomChoice
  / Rest
  / Value

// Random choice: a|b|c (at least one `|` present).
RandomChoice
  = first:ChoiceElement rest:(_ "|" _ @ChoiceElement)+ {
      const seed = ctx.nextSeed();
      return { RandomChoice: [[first, ...rest], seed] };
    }

ChoiceElement
  = Rest
  / Value

Rest
  = ("~" / "-" ![0-9]) { return { Rest: spanOf(location()) }; }

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

Value
  = HzValue
  / NoteValue
  / NumberValue

HzValue
  = n:Number "hz"i {
      return { Pure: located({ Hz: n }, location()) };
    }

NumberValue
  = n:Number {
      return { Pure: located({ Number: n }, location()) };
    }

// Notes like c4, c#4, cb4, c (no octave).
// Accidental is one of: # s (sharp) or b f (flat). We only store '#' / 'b'
// in the AST. The note letter must NOT be followed by another letter or
// digit that would make it part of an identifier — guarded by negative
// lookahead on alphanumerics after the note.
NoteValue
  = letter:$[a-gA-G] acc:Accidental? oct:Octave? !IdentCont {
      return {
        Pure: located(
          {
            Note: {
              letter: letter.toLowerCase(),
              accidental: acc,
              octave: oct,
            },
          },
          location()
        ),
      };
    }

Accidental
  = "#" { return "#"; }
  / "s" { return "#"; }
  / "b" &IntOrDigit { return "b"; }
  / "f" &IntOrDigit { return "b"; }

Octave
  = sign:"-"? digits:$[0-9]+ {
      return parseInt((sign || "") + digits, 10);
    }

// Lookahead used to disambiguate a trailing flat accidental from a bare note
// followed by whitespace. "cb4" -> c flat 4; "cb" alone -> identifier (not a
// valid atom in the reduced grammar) so falls through to parse failure.
IntOrDigit
  = "-"? [0-9]

// Identifier continuation — any letter/digit/underscore.
IdentCont
  = [a-zA-Z0-9_]

// ---------------------------------------------------------------------------
// Modifiers
// ---------------------------------------------------------------------------

Modifier
  = FastMod
  / SlowMod
  / Replicate
  / Degrade
  / Euclidean

// Weight is captured at ElementWithWeight level as positional metadata
// on the sequence entry — never mixed into a modifier chain.
Weight
  = "@" n:Number? { return n === null ? 1 : n; }

FastMod
  = "*" op:ModOperandF64 {
      return (ast, _c) => ({ Fast: [ast, op] });
    }

SlowMod
  = "/" op:ModOperand {
      return (ast, _c) => ({ Slow: [ast, op] });
    }

Replicate
  = "!" n:Integer? {
      const count = n === null ? 2 : n;
      return (ast, _c) => ({ Replicate: [ast, count] });
    }

Degrade
  = "?" p:Number? {
      return (ast, c) => ({ Degrade: [ast, p, c.nextSeed()] });
    }

Euclidean
  = "(" _ pulses:ModOperandU32 _ "," _ steps:ModOperandU32 _ rot:("," _ @ModOperandI32)? _ ")" {
      return (ast, _c) => ({
        Euclidean: {
          pattern: ast,
          pulses: pulses,
          steps: steps,
          rotation: rot,
        },
      });
    }

// Modifier operand is a number, a fast subsequence, or a slow subsequence,
// each projected into the modifier's expected AST type.

ModOperand
  = r:OperandRandomChoice { return r; }
  / n:Number {
      return { Pure: located({ Number: n }, location()) };
    }
  / "[" _ s:OperandStackExpr _ "]" { return s.fast; }
  / "<" _ s:OperandStackExpr _ ">" { return s.slow; }

ModOperandF64
  = r:OperandRandomChoiceF64 { return r; }
  / n:Number {
      return { Pure: located(n, location()) };
    }
  / "[" _ s:OperandStackExprF64 _ "]" { return s.fast; }
  / "<" _ s:OperandStackExprF64 _ ">" { return s.slow; }

ModOperandU32
  = r:OperandRandomChoiceU32 { return r; }
  / n:Integer {
      if (n < 0) error("Euclidean pulses/steps must be non-negative integers");
      return { Pure: located(n, location()) };
    }
  / "[" _ s:OperandStackExprU32 _ "]" { return s.fast; }
  / "<" _ s:OperandStackExprU32 _ ">" { return s.slow; }

ModOperandI32
  = r:OperandRandomChoiceI32 { return r; }
  / n:Integer {
      return { Pure: located(n, location()) };
    }
  / "[" _ s:OperandStackExprI32 _ "]" { return s.fast; }
  / "<" _ s:OperandStackExprI32 _ ">" { return s.slow; }

// Operand subsequence parsing — much simpler than full StackExpr since
// modifier operands only carry numeric values. Each operand type has its
// own thin expansion.

OperandStackExpr
  = first:OperandSequence tail:(_ "," _ @OperandSequence)* {
      if (tail.length === 0) {
        return { fast: { FastCat: first.asCat }, slow: { SlowCat: first.asCat } };
      }
      const all = [first, ...tail];
      return {
        fast: { Stack: all.map(s => ({ FastCat: s.asCat })) },
        slow: { Stack: all.map(s => ({ SlowCat: s.asCat })) },
      };
    }

OperandSequence
  = elems:(_ @OperandSeqEntry)+ {
      return { asCat: applyElongation(elems) };
    }

OperandSeqEntry
  = "_" !IdentCont { return { kind: 'elongation' }; }
  / e:OperandElement { return { kind: 'element', entry: e }; }

OperandElement
  = r:OperandRandomChoice weight:Weight? { return [r, weight]; }
  / base:OperandBase weight:Weight? { return [base, weight]; }

OperandRandomChoice
  = first:OperandChoice rest:(_ "|" _ @OperandChoice)+ {
      const seed = ctx.nextSeed();
      return { RandomChoice: [[first, ...rest], seed] };
    }

OperandChoice
  = ("~" / "-" ![0-9]) { return { Rest: spanOf(location()) }; }
  / n:Number { return { Pure: located({ Number: n }, location()) }; }

OperandBase
  = ("~" / "-" ![0-9]) { return { Rest: spanOf(location()) }; }
  / n:Number { return { Pure: located({ Number: n }, location()) }; }
  / "[" _ s:OperandStackExpr _ "]" { return s.fast; }
  / "<" _ s:OperandStackExpr _ ">" { return s.slow; }

// ------ f64 operand subtree ------

OperandStackExprF64
  = first:OperandSequenceF64 tail:(_ "," _ @OperandSequenceF64)* {
      if (tail.length === 0) {
        return { fast: { FastCat: first.asCat }, slow: { SlowCat: first.asCat } };
      }
      const all = [first, ...tail];
      return {
        fast: { Stack: all.map(s => ({ FastCat: s.asCat })) },
        slow: { Stack: all.map(s => ({ SlowCat: s.asCat })) },
      };
    }

OperandSequenceF64
  = elems:(_ @OperandSeqEntryF64)+ { return { asCat: applyElongation(elems) }; }

OperandSeqEntryF64
  = "_" !IdentCont { return { kind: 'elongation' }; }
  / e:OperandElementF64 { return { kind: 'element', entry: e }; }

OperandElementF64
  = r:OperandRandomChoiceF64 weight:Weight? { return [r, weight]; }
  / base:OperandBaseF64 weight:Weight? { return [base, weight]; }

OperandRandomChoiceF64
  = first:OperandChoiceF64 rest:(_ "|" _ @OperandChoiceF64)+ {
      const seed = ctx.nextSeed();
      return { RandomChoice: [[first, ...rest], seed] };
    }

OperandChoiceF64
  = ("~" / "-" ![0-9]) { return { Rest: spanOf(location()) }; }
  / n:Number { return { Pure: located(n, location()) }; }

OperandBaseF64
  = ("~" / "-" ![0-9]) { return { Rest: spanOf(location()) }; }
  / n:Number { return { Pure: located(n, location()) }; }
  / "[" _ s:OperandStackExprF64 _ "]" { return s.fast; }
  / "<" _ s:OperandStackExprF64 _ ">" { return s.slow; }

// ------ u32 operand subtree ------

OperandStackExprU32
  = first:OperandSequenceU32 tail:(_ "," _ @OperandSequenceU32)* {
      if (tail.length === 0) {
        return { fast: { FastCat: first.asCat }, slow: { SlowCat: first.asCat } };
      }
      const all = [first, ...tail];
      return {
        fast: { Stack: all.map(s => ({ FastCat: s.asCat })) },
        slow: { Stack: all.map(s => ({ SlowCat: s.asCat })) },
      };
    }

OperandSequenceU32
  = elems:(_ @OperandSeqEntryU32)+ { return { asCat: applyElongation(elems) }; }

OperandSeqEntryU32
  = "_" !IdentCont { return { kind: 'elongation' }; }
  / e:OperandElementU32 { return { kind: 'element', entry: e }; }

OperandElementU32
  = r:OperandRandomChoiceU32 weight:Weight? { return [r, weight]; }
  / base:OperandBaseU32 weight:Weight? { return [base, weight]; }

OperandRandomChoiceU32
  = first:OperandChoiceU32 rest:(_ "|" _ @OperandChoiceU32)+ {
      const seed = ctx.nextSeed();
      return { RandomChoice: [[first, ...rest], seed] };
    }

OperandChoiceU32
  = ("~" / "-" ![0-9]) { return { Rest: spanOf(location()) }; }
  / n:Integer {
      if (n < 0) error("Expected non-negative integer");
      return { Pure: located(n, location()) };
    }

OperandBaseU32
  = ("~" / "-" ![0-9]) { return { Rest: spanOf(location()) }; }
  / n:Integer {
      if (n < 0) error("Expected non-negative integer");
      return { Pure: located(n, location()) };
    }
  / "[" _ s:OperandStackExprU32 _ "]" { return s.fast; }
  / "<" _ s:OperandStackExprU32 _ ">" { return s.slow; }

// ------ i32 operand subtree ------

OperandStackExprI32
  = first:OperandSequenceI32 tail:(_ "," _ @OperandSequenceI32)* {
      if (tail.length === 0) {
        return { fast: { FastCat: first.asCat }, slow: { SlowCat: first.asCat } };
      }
      const all = [first, ...tail];
      return {
        fast: { Stack: all.map(s => ({ FastCat: s.asCat })) },
        slow: { Stack: all.map(s => ({ SlowCat: s.asCat })) },
      };
    }

OperandSequenceI32
  = elems:(_ @OperandSeqEntryI32)+ { return { asCat: applyElongation(elems) }; }

OperandSeqEntryI32
  = "_" !IdentCont { return { kind: 'elongation' }; }
  / e:OperandElementI32 { return { kind: 'element', entry: e }; }

OperandElementI32
  = r:OperandRandomChoiceI32 weight:Weight? { return [r, weight]; }
  / base:OperandBaseI32 weight:Weight? { return [base, weight]; }

OperandRandomChoiceI32
  = first:OperandChoiceI32 rest:(_ "|" _ @OperandChoiceI32)+ {
      const seed = ctx.nextSeed();
      return { RandomChoice: [[first, ...rest], seed] };
    }

OperandChoiceI32
  = ("~" / "-" ![0-9]) { return { Rest: spanOf(location()) }; }
  / n:Integer { return { Pure: located(n, location()) }; }

OperandBaseI32
  = ("~" / "-" ![0-9]) { return { Rest: spanOf(location()) }; }
  / n:Integer { return { Pure: located(n, location()) }; }
  / "[" _ s:OperandStackExprI32 _ "]" { return s.fast; }
  / "<" _ s:OperandStackExprI32 _ ">" { return s.slow; }

// ---------------------------------------------------------------------------
// Numbers & whitespace
// ---------------------------------------------------------------------------

Number "number"
  = s:$("-"? [0-9]+ ("." [0-9]+)?) { return parseFloat(s); }

Integer "integer"
  = s:$("-"? [0-9]+) { return parseInt(s, 10); }

_ "whitespace"
  = [ \t\r\n]*
