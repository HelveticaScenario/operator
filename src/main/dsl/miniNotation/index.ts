/**
 * Public entry point for the TypeScript mini-notation implementation.
 *
 * `$p(source)` parses a mini-notation string into a serializable
 * `ParsedPattern` object that can be passed as an argument to `$cycle` or
 * `$iCycle`. The object is consumed by the Rust side during patch-graph
 * deserialization: its `ast`, `source`, and `all_spans` fields map
 * directly onto the Rust `SeqPatternParam` / `IntervalPatternParam`
 * deserialization payload.
 */

import type { AtomValue, MiniAST, ParsedPattern, SourceSpan } from './ast';
import { collectLeafSpans } from './collectLeafSpans';
import { MiniParseError, parseMini } from './parser';
import { captureSourceLocation } from '../captureSourceLocation';
import { lookupArgumentSpan } from '../factories';
import { degreesToVoltages } from '@modular/core';

export type { MiniAST, ParsedPattern } from './ast';
export { MiniParseError } from './parser';

/**
 * Parse a mini-notation string into a `ParsedPattern`.
 *
 * Entry point for all mini-notation usage in the DSL: `$cycle` and
 * `$iCycle` accept a `ParsedPattern`, so every mini-notation literal
 * flows through `$p()`. Examples:
 *
 * ```js
 * $cycle($p("c4 e4 g4"))
 * $iCycle([$p("0 2 4"), $p("0,4")], "c4(major)")
 * const bass = $p("c2 [c2 g2] c2 e2");
 * $cycle(bass)
 * ```
 *
 * The returned object is JSON-serializable and structurally compatible
 * with the Rust `{ ast, source, all_spans }` shape expected by
 * `SeqPatternParam` / `IntervalPatternParam` during patch-graph
 * deserialization. It also embeds an `argument_span` captured from the
 * call site so that editor highlighting follows the pattern through
 * `const` indirections (`const p = $p(...); $cycle(p)`).
 *
 * Throws `MiniParseError` if `source` is not a string or fails to
 * parse. See the `$cycle` doc comment for the full mini-notation
 * grammar.
 */
export function $p(source: string): ParsedPattern {
    if (typeof source !== 'string') {
        throw new MiniParseError(
            `$p() expects a string argument, got ${typeof source}`,
        );
    }
    const ast: MiniAST = parseMini(source);
    const all_spans = collectLeafSpans(ast);
    const sourceLocation = captureSourceLocation();
    const argument_span = lookupArgumentSpan(sourceLocation, 'source');
    const pattern: ParsedPattern = {
        __kind: 'ParsedPattern',
        ast,
        source,
        all_spans,
    };
    if (argument_span) {
        pattern.argument_span = argument_span;
    }
    return pattern;
}

/** Type guard for runtime `ParsedPattern` checks. */
export function isParsedPattern(value: unknown): value is ParsedPattern {
    return (
        typeof value === 'object' &&
        value !== null &&
        (value as { __kind?: unknown }).__kind === 'ParsedPattern'
    );
}

/**
 * Parse a scale-degree mini-notation source and resolve each integer
 * degree to its V/Oct voltage against `scale`, returning a
 * `ParsedPattern` suitable for `$cycle`.
 *
 * Atoms are 0-indexed scale degrees: `0` is the scale's root, `1` the
 * second tone, `2` the third, and so on. Negative values move downward.
 * Values beyond the scale length wrap into higher/lower octaves
 * automatically. Hz and note atoms are rejected — use `$p` for those.
 *
 * Mini-notation grammar (groups, stacks, modifiers, euclidean, etc.) is
 * the same as `$p`; only the atom vocabulary differs.
 *
 * Scale string accepts `"c(major)"`, `"D#3(min)"`, custom intervals
 * `"c(0 2 4 5 7 9 11)"`, just-intonation tunings `"c(just)"` /
 * `"c(pythagorean)"`, and the bare `"chromatic"` ladder.
 */
export function $sp(source: string, scale: string): ParsedPattern {
    if (typeof source !== 'string') {
        throw new MiniParseError(
            `$sp() expects a string source, got ${typeof source}`,
        );
    }
    if (typeof scale !== 'string') {
        throw new MiniParseError(
            `$sp() expects a string scale, got ${typeof scale}`,
        );
    }

    const ast = parseMini(source);
    const all_spans = collectLeafSpans(ast);

    // Walk in declaration order, collecting every Pure Number atom while
    // rejecting Hz / Note atoms. Order is preserved between collect + map
    // passes so voltages line up with their atoms.
    const degrees: number[] = [];
    visitPureAtoms(ast, (atom, span) => {
        if ('Hz' in atom) {
            throw new MiniParseError(
                '$sp() rejects Hz atoms — use integer scale degrees',
                span.start,
                span.end,
            );
        }
        if ('Note' in atom) {
            throw new MiniParseError(
                '$sp() rejects note atoms — use integer scale degrees',
                span.start,
                span.end,
            );
        }
        const n = atom.Number;
        if (!Number.isFinite(n) || !Number.isInteger(n)) {
            throw new MiniParseError(
                `$sp() requires integer scale degrees, got ${n}`,
                span.start,
                span.end,
            );
        }
        degrees.push(n);
    });

    const voltages: number[] =
        degrees.length === 0 ? [] : degreesToVoltages(degrees, scale);

    let voltageIdx = 0;
    const resolved = mapPureAtoms(ast, () => {
        const v = voltages[voltageIdx++];
        return { Number: v };
    });

    const sourceLocation = captureSourceLocation();
    const argument_span = lookupArgumentSpan(sourceLocation, 'source');
    const pattern: ParsedPattern = {
        __kind: 'ParsedPattern',
        ast: resolved,
        source,
        all_spans,
    };
    if (argument_span) {
        pattern.argument_span = argument_span;
    }
    return pattern;
}

/**
 * Visit every `Pure` atom of a `MiniAST` in declaration (depth-first,
 * left-to-right) order. Modifier-argument sub-ASTs (Fast factor, Slow
 * factor, euclidean pulses/steps/rotation, polymeter steps_per_cycle) are
 * NOT visited — they carry numeric modifier values, not pattern atoms.
 */
function visitPureAtoms(
    ast: MiniAST,
    visit: (atom: AtomValue, span: SourceSpan) => void,
): void {
    if ('Pure' in ast) {
        visit(ast.Pure.node, ast.Pure.span);
        return;
    }
    if ('Rest' in ast) return;
    if ('List' in ast) {
        for (const child of ast.List.node) visitPureAtoms(child, visit);
        return;
    }
    if ('Sequence' in ast) {
        for (const [child] of ast.Sequence) visitPureAtoms(child, visit);
        return;
    }
    if ('FastCat' in ast) {
        for (const [child] of ast.FastCat) visitPureAtoms(child, visit);
        return;
    }
    if ('SlowCat' in ast) {
        for (const [child] of ast.SlowCat) visitPureAtoms(child, visit);
        return;
    }
    if ('Stack' in ast) {
        for (const child of ast.Stack) visitPureAtoms(child, visit);
        return;
    }
    if ('RandomChoice' in ast) {
        for (const child of ast.RandomChoice[0]) visitPureAtoms(child, visit);
        return;
    }
    if ('Fast' in ast) {
        visitPureAtoms(ast.Fast[0], visit);
        return;
    }
    if ('Slow' in ast) {
        // Slow's factor (second slot) is a separate numeric value, not a
        // pattern atom — convert.rs reads it via to_f64. Skip.
        visitPureAtoms(ast.Slow[0], visit);
        return;
    }
    if ('Replicate' in ast) {
        visitPureAtoms(ast.Replicate[0], visit);
        return;
    }
    if ('Degrade' in ast) {
        visitPureAtoms(ast.Degrade[0], visit);
        return;
    }
    if ('Euclidean' in ast) {
        visitPureAtoms(ast.Euclidean.pattern, visit);
        return;
    }
    if ('Polymeter' in ast) {
        for (const child of ast.Polymeter.children) visitPureAtoms(child, visit);
        return;
    }
}

/**
 * Produce a new `MiniAST` with every `Pure` atom replaced by `transform`'s
 * return value, preserving structure and spans. Visits in the same order
 * as `visitPureAtoms`.
 */
function mapPureAtoms(
    ast: MiniAST,
    transform: (atom: AtomValue, span: SourceSpan) => AtomValue,
): MiniAST {
    if ('Pure' in ast) {
        return {
            Pure: {
                node: transform(ast.Pure.node, ast.Pure.span),
                span: ast.Pure.span,
            },
        };
    }
    if ('Rest' in ast) return ast;
    if ('List' in ast) {
        return {
            List: {
                node: ast.List.node.map((c) => mapPureAtoms(c, transform)),
                span: ast.List.span,
            },
        };
    }
    if ('Sequence' in ast) {
        return {
            Sequence: ast.Sequence.map(
                ([c, w]) => [mapPureAtoms(c, transform), w] as [MiniAST, number | null],
            ),
        };
    }
    if ('FastCat' in ast) {
        return {
            FastCat: ast.FastCat.map(
                ([c, w]) => [mapPureAtoms(c, transform), w] as [MiniAST, number | null],
            ),
        };
    }
    if ('SlowCat' in ast) {
        return {
            SlowCat: ast.SlowCat.map(
                ([c, w]) => [mapPureAtoms(c, transform), w] as [MiniAST, number | null],
            ),
        };
    }
    if ('Stack' in ast) {
        return { Stack: ast.Stack.map((c) => mapPureAtoms(c, transform)) };
    }
    if ('RandomChoice' in ast) {
        return {
            RandomChoice: [
                ast.RandomChoice[0].map((c) => mapPureAtoms(c, transform)),
                ast.RandomChoice[1],
            ],
        };
    }
    if ('Fast' in ast) {
        return { Fast: [mapPureAtoms(ast.Fast[0], transform), ast.Fast[1]] };
    }
    if ('Slow' in ast) {
        return { Slow: [mapPureAtoms(ast.Slow[0], transform), ast.Slow[1]] };
    }
    if ('Replicate' in ast) {
        return {
            Replicate: [
                mapPureAtoms(ast.Replicate[0], transform),
                ast.Replicate[1],
            ],
        };
    }
    if ('Degrade' in ast) {
        return {
            Degrade: [
                mapPureAtoms(ast.Degrade[0], transform),
                ast.Degrade[1],
                ast.Degrade[2],
            ],
        };
    }
    if ('Euclidean' in ast) {
        return {
            Euclidean: {
                pattern: mapPureAtoms(ast.Euclidean.pattern, transform),
                pulses: ast.Euclidean.pulses,
                steps: ast.Euclidean.steps,
                rotation: ast.Euclidean.rotation,
            },
        };
    }
    if ('Polymeter' in ast) {
        return {
            Polymeter: {
                children: ast.Polymeter.children.map((c) =>
                    mapPureAtoms(c, transform),
                ),
                steps_per_cycle: ast.Polymeter.steps_per_cycle,
            },
        };
    }
    // Exhaustive — every variant handled above.
    return ast;
}
