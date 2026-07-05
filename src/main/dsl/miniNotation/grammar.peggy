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
 *   - No $-operator chain, struct/target/scale/etc. high-level statements,
 *     no `cat(...)` / `slow(...)` / `fast(...)` commands.
 *   - Replicate (`!`) accumulates: `x!!` / `x!2!3` sum into a single
 *     Replicate count (1 + Σ(value - 1)), and bare `@` weight defaults to 2 —
 *     both matching Tidal/krill (and `@` is then consistent with `_`).
 *   - Separator model matches Tidal/krill: top-level `,` and `|` are mutually
 *     exclusive (commit to one, never mix), feet (`.`) live inside each
 *     operand, and `<...>` / `{...}` are comma-only voices (no `|`).
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
  = _ s:Group _ { return s; }

// Top-level separators `,` (stack) and `|` (random choice) are mutually
// exclusive: a group commits to one tail and never mixes them, so `a , b , c`
// stacks, `a | b | c` chooses, and `a , b | c` is a parse error. Matches
// Tidal's stackTail/chooseTail and krill's single stack_or_choose tail. Feet
// (`.`) live one level down, inside each `,`/`|` operand.
Group
  = head:FeetSeq tail:(CommaTail / PipeTail)? {
      if (!tail) return head;
      if (tail.kind === 'stack') return { Stack: [head, ...tail.items] };
      return { RandomChoice: [[head, ...tail.items], ctx.nextSeed()] };
    }

// Stack tail: `, b , c` — comma-separated voices that play simultaneously.
CommaTail
  = items:(_ "," _ @FeetSeq)+ { return { kind: 'stack', items }; }

// Choice tail: `| b | c` — picks one whole operand at random each cycle.
// Mirrors strudel krill's `choose_tail`; the seed is taken in the Group
// action (after the operands) so seeding stays left-to-right depth-first,
// matching Rust's assign_seeds.
PipeTail
  = items:(_ "|" _ @FeetSeq)+ { return { kind: 'rand', items }; }

// Feet separator: `a b . c d` wraps the dot-separated sub-sequences [a b]
// and [c d] in a Sequence node (which convert.rs lowers identically to a
// fastcat), each taking an equal slice of the cycle. Feet bind tighter than
// the `,`/`|` tails but looser than a space-separated sequence. Mirrors
// strudel's `dot_tail` → `feet` alignment.
FeetSeq
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

// Leading `_` mirrors pest's implicit WHITESPACE between an element_base and
// its modifier chain (and between successive modifiers). The repetition that
// drives this rule backtracks a failed attempt, so whitespace is only
// committed when a real modifier sigil follows — `a b` still parses as two
// elements, `a / 2` as one slowed element.
ElementPart
  = _ w:Weight { return { kind: 'weight', value: w }; }
  / _ m:Modifier { return { kind: 'mod', apply: m }; }

ElementBase
  = FastSub
  / SlowSub
  / Polymeter
  / Atom

// Polymeter: `{a b, c d e}` with optional `%n` steps-per-cycle override.
// Each comma-separated child sequence gets stacked, scaled so its step
// count maps to `steps_per_cycle`. Matches strudel's `polymeter`.
Polymeter
  = "{" _ head:FeetSeq tail:(_ "," _ @FeetSeq)* _ "}" steps:PolymeterSteps? {
      const children = [head, ...tail];
      return { Polymeter: { children, steps_per_cycle: steps } };
    }

PolymeterSteps
  = "%" op:ModOperandF64 { return op; }

FastSub
  = "[" _ s:Group _ "]" {
      // Keep `[...]` as FastCat (explicit fast subsequence) rather than
      // letting the inner Group flatten. The Group body allows `,` XOR `|`
      // with feet inside; a Stack/RandomChoice is wrapped as a single-element
      // FastCat so nesting semantics match the Rust parser.
      if (s && s.Stack) {
        return { FastCat: [[s, null]] };
      }
      if (s && s.Sequence) {
        return { FastCat: s.Sequence };
      }
      // Single element (atom, or a `|` choice voice): wrap in one entry.
      return { FastCat: [[s, null]] };
    }

// `<...>` is comma-only (voices), feet allowed, no `|` — matches Tidal/krill
// (`<0|1>` is a parse error). Build the same Stack/Sequence intermediate a
// comma group would, then wrap as SlowCat exactly as before.
SlowSub
  = "<" _ head:FeetSeq tail:(_ "," _ @FeetSeq)* _ ">" {
      // Each comma voice becomes its own slowcat (one entry per cycle, each
      // voice keeping its own period), then the voices are stacked:
      // `<a b, c d e>` is `stack(slowcat(a,b), slowcat(c,d,e))` — matching
      // strudel and the operand `<...>` subtrees below. A voice that is a
      // Sequence contributes its entries as slowcat steps; any other voice
      // (single atom, `[...]` group, …) is a one-step slowcat.
      const toSlowCat = (v) =>
        v && v.Sequence ? { SlowCat: v.Sequence } : { SlowCat: [[v, null]] };
      if (tail.length === 0) {
        return toSlowCat(head);
      }
      return { Stack: [head, ...tail].map(toSlowCat) };
    }

// Atoms: a rest or a concrete value. Random choice (`|`) is handled higher
// up at the ChoiceExpr level so it can alternate whole sequences.
Atom
  = Rest
  / Value

Rest
  = ("~" / "-" ![0-9]) { return { Rest: spanOf(location()) }; }

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

Value
  = HzValue
  / TruthyValue
  / NoteValue
  / NumberValue

// `x` is the truthy structure marker used by `.struct(...)` boolean patterns
// (an onset with no pitch of its own). The !IdentCont guard stops it from
// matching the start of a longer alphanumeric run, so `x4` / `xx` are errors.
TruthyValue
  = [xX] !IdentCont {
      return { Pure: located('Truthy', location()) };
    }

HzValue
  = n:Number "hz"i {
      return { Pure: located({ Hz: n }, location()) };
    }

NumberValue
  = n:Number {
      return { Pure: located({ Number: n }, location()) };
    }

// A note is a letter (a-g), an optional accidental, and an optional octave:
// c, c4, c#, c#4, cb, eb3. Sharp is # or s; flat is b or f; the AST stores
// only '#' or 'b'. The trailing !IdentCont stops a note from matching the
// start of a longer alphanumeric run, so `cb` is c-flat but `cba` is not a note.
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
  / "b" { return "b"; }
  / "f" { return "b"; }

Octave
  = sign:"-"? digits:$[0-9]+ {
      return parseInt((sign || "") + digits, 10);
    }

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
// Sigils whose operand is mandatory (`*`, `/`) — and `@` — permit whitespace
// before the operand, mirroring pest's implicit WHITESPACE (`/ 4`, `* 2`,
// `@ 3` all parse in the Rust grammar). The optional-operand sigils `!` and
// `?` bind their number only when it is adjacent: `1! 2` / `1? 2` keep the
// 2 as its own sequence element — matching Tidal/krill and the Rust descent
// parser. Euclidean already spells its inner whitespace explicitly.
// Bare `@` (no number) is weight 2 — matching Tidal's `pElongate = 1 + sum`
// and krill, and consistent with `_` elongation.
Weight
  = "@" _ n:Number? { return n === null ? 2 : n; }

FastMod
  = "*" _ op:ModOperandF64 {
      return (ast, _c) => ({ Fast: [ast, op] });
    }

SlowMod
  = "/" _ op:ModOperandF64 {
      return (ast, _c) => ({ Slow: [ast, op] });
    }

// Replicate accumulates: consecutive `!`/`!n` sum into a single Replicate, so
// `x!` = 2 copies, `x!3` = 3, `x!!` = 3, `x!2!3` = 4 — matching Tidal's
// `pRepeat = 1 + sum es` and krill. Total copies = 1 + Σ(value - 1).
Replicate
  = parts:(_ "!" n:Integer? { return n === null ? 2 : n; })+ {
      const total = parts.reduce((acc, v) => acc + (v - 1), 1);
      if (total < 0) error("Replicate count must be a non-negative integer");
      return (ast, _c) => ({ Replicate: [ast, total] });
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

// Operand elements carry the same `@weight` + modifier chain as top-level
// elements (`ElementWithWeight`), so `<16!2 12>` etc. work inside euclidean /
// fast / slow operand subsequences. Weight is positional metadata on the
// sequence entry; non-weight modifiers wrap the AST in order.
OperandElementF64
  = base:(OperandRandomChoiceF64 / OperandBaseF64) parts:ElementPart* {
      let ast = base;
      let weight = null;
      for (const part of parts) {
        if (part.kind === 'weight') {
          weight = part.value;
        } else {
          ast = part.apply(ast, ctx);
        }
      }
      return [ast, weight];
    }

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
  = base:(OperandRandomChoiceU32 / OperandBaseU32) parts:ElementPart* {
      let ast = base;
      let weight = null;
      for (const part of parts) {
        if (part.kind === 'weight') {
          weight = part.value;
        } else {
          ast = part.apply(ast, ctx);
        }
      }
      return [ast, weight];
    }

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
  = base:(OperandRandomChoiceI32 / OperandBaseI32) parts:ElementPart* {
      let ast = base;
      let weight = null;
      for (const part of parts) {
        if (part.kind === 'weight') {
          weight = part.value;
        } else {
          ast = part.apply(ast, ctx);
        }
      }
      return [ast, weight];
    }

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
