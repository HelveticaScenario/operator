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

import type { MiniAST, ParsedPattern } from './ast';
import { collectLeafSpans } from './collectLeafSpans';
import { MiniParseError, parseMini } from './parser';
import { captureSourceLocation } from '../captureSourceLocation';
import { lookupArgumentSpan } from '../factories';

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
