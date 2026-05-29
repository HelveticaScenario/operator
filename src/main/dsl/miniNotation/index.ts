/**
 * Public entry point for the TypeScript mini-notation implementation.
 *
 * `$p(source)` parses a mini-notation string into a serializable
 * `ParsedPattern`. `$p.s(source, scale)` returns an `SpPattern` chainable
 * with `.add(...)` / `.sub(...)` and seven alignment modes (`.add.in`,
 * `.add.out`, `.add.mix`, `.add.squeeze`, `.add.squeezeout`,
 * `.add.reset`, `.add.restart`, same for `.sub`). Both are consumed by
 * `$cycle`'s `pattern` param: the Rust side dispatches on the wire shape
 * (`SeqPatternSource::Single | Sp`) and lowers either to a runtime
 * `Pattern<SeqValue>`.
 */

import type {
    AtomValue,
    MiniAST,
    ParsedPattern,
    SourceSpan,
    Located,
} from './ast';
import { collectLeafSpans } from './collectLeafSpans';
import { MiniParseError, parseMini } from './parser';
import { captureSourceLocation } from '../captureSourceLocation';
import { lookupArgumentSpan } from '../factories';

export type { MiniAST, ParsedPattern } from './ast';
export { MiniParseError } from './parser';

/**
 * Parse a mini-notation string into a `ParsedPattern`.
 *
 * Entry point for all mini-notation usage in the DSL: `$cycle` accepts
 * a `ParsedPattern`, so every mini-notation literal flows through
 * `$p()`. Examples:
 *
 * ```js
 * $cycle($p("c4 e4 g4"))
 * const bass = $p("c2 [c2 g2] c2 e2");
 * $cycle(bass)
 * ```
 *
 * The returned object is JSON-serializable and structurally compatible
 * with the Rust `{ ast, source, all_spans }` shape expected during
 * patch-graph deserialization. It also embeds an `argument_span`
 * captured from the call site so that editor highlighting follows the
 * pattern through `const` indirections.
 *
 * Throws `MiniParseError` if `source` is not a string or fails to parse.
 */
function $pImpl(source: string): ParsedPattern {
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

/**
 * The mini-notation factory. Callable as `$p(source)` for a plain
 * `ParsedPattern`, and carries `$p.s(source, scale)` for scale-degree
 * patterns (see `$spImpl`). `$spImpl` is hoisted, so the assignment can
 * reference it here.
 */
export const $p: typeof $pImpl & { s: typeof $spImpl } = Object.assign(
    $pImpl,
    { s: $spImpl },
);

/** Type guard for runtime `ParsedPattern` checks. */
export function isParsedPattern(value: unknown): value is ParsedPattern {
    return (
        typeof value === 'object' &&
        value !== null &&
        (value as { __kind?: unknown }).__kind === 'ParsedPattern'
    );
}

// ─── $p.s chainable + alignment ─────────────────────────────────────────

/** Strudel-style alignment modes for `$p.s` chain ops. */
export type SpAlignmentMode =
    | 'in'
    | 'out'
    | 'mix'
    | 'squeeze'
    | 'squeezeout'
    | 'reset'
    | 'restart';

const ALIGNMENT_MODES: ReadonlyArray<SpAlignmentMode> = [
    'in',
    'out',
    'mix',
    'squeeze',
    'squeezeout',
    'reset',
    'restart',
];

/** Wire-shape op carried in `SpPattern.ops`. */
export type SpOpKind = 'add' | 'sub';

export interface SpOp {
    op: SpOpKind;
    mode: SpAlignmentMode;
}

/** Discriminator string the Rust side matches against. */
const SP_KIND = 'SpPattern' as const;

/**
 * Mini-notation payload shape — same as `ParsedPattern` minus chain
 * methods. Used as elements of `SpPattern.sources`.
 *
 * Mutable arrays (rather than readonly) so the shape lines up with the
 * factory's schema-generated param types. Treat as immutable by
 * convention.
 */
export interface ParsedPatternPayload {
    ast: MiniAST;
    source: string;
    all_spans: Array<[number, number]>;
}

/** Callable + method bag for chain methods. */
export type SpCombineBuilder = ((rhs: string) => SpPattern) & {
    [M in SpAlignmentMode]: (rhs: string) => SpPattern;
};

/**
 * Wire shape for chained scale-degree patterns. `$cycle`'s Rust-side
 * deserializer recognises `__kind === 'SpPattern'` via the
 * `SeqPatternSource` untagged enum and lowers the chain to a
 * `Pattern<SeqValue>` at param ingestion time.
 *
 * Arrays are mutable (rather than readonly) so the shape lines up with
 * the schema-generated `$cycle.pattern` param type. Treat as immutable
 * by convention.
 */
export interface SpPattern {
    __kind: typeof SP_KIND;
    sources: Array<ParsedPatternPayload>;
    scale: string;
    ops: Array<SpOp>;
    argument_spans: Array<SourceSpan>;
    add: SpCombineBuilder;
    sub: SpCombineBuilder;
}

/**
 * Parse a scale-degree mini-notation source against `scale`, returning
 * an `SpPattern`. Exposed to the DSL as `$p.s(source, scale)`. Chain via
 * `.add(...)` / `.sub(...)` (defaults to `.in` alignment) or any of the
 * seven explicit modes (`.add.in`, `.add.out`, `.add.mix`, `.add.squeeze`,
 * `.add.squeezeout`, `.add.reset`, `.add.restart`). Each chained RHS is
 * itself a mini-notation source of integer scale degrees.
 *
 * Atoms are 0-indexed scale degrees: `0` is the scale's root, `1` the
 * second tone, `2` the third, etc. Negative degrees move downward;
 * degrees beyond the scale length wrap into higher/lower octaves
 * automatically. Hz / note atoms are rejected at `$cycle` ingestion.
 *
 * Mini-notation grammar (groups, stacks, modifiers, euclidean, etc.) is
 * the same as `$p`; only the atom vocabulary differs.
 *
 * Scale string accepts `"c(major)"`, `"D#3(min)"`, custom intervals
 * `"c(0 2 4 5 7 9 11)"`, just-intonation tunings `"c(just)"` /
 * `"c(pythagorean)"`, and the bare `"chromatic"` ladder.
 */
function $spImpl(source: string, scale: string): SpPattern {
    if (typeof source !== 'string') {
        throw new MiniParseError(
            `$p.s() expects a string source, got ${typeof source}`,
        );
    }
    if (typeof scale !== 'string') {
        throw new MiniParseError(
            `$p.s() expects a string scale, got ${typeof scale}`,
        );
    }
    const payload = parsePayload(source);
    const loc = captureSourceLocation();
    const argSpan = lookupArgumentSpan(loc, 'source');
    return buildSpPattern(
        [payload],
        scale,
        [],
        argSpan ? [argSpan] : [{ start: 0, end: 0 }],
    );
}

/** Type guard for runtime `SpPattern` checks. */
export function isSpPattern(value: unknown): value is SpPattern {
    return (
        typeof value === 'object' &&
        value !== null &&
        (value as { __kind?: unknown }).__kind === SP_KIND
    );
}

function parsePayload(source: string): ParsedPatternPayload {
    const ast = parseMini(source);
    const all_spans = collectLeafSpans(ast);
    return { ast, source, all_spans };
}

function buildSpPattern(
    sources: ParsedPatternPayload[],
    scale: string,
    ops: SpOp[],
    argument_spans: SourceSpan[],
): SpPattern {
    const pat: Record<string, unknown> = {
        __kind: SP_KIND,
        sources,
        scale,
        ops,
        argument_spans,
    };
    const handle = pat as unknown as SpPattern;

    // add / sub are non-enumerable so JSON.stringify omits them and only
    // the wire-shape fields cross the IPC boundary.
    Object.defineProperty(pat, 'add', {
        enumerable: false,
        configurable: true,
        get: () => makeCombineBuilder(handle, 'add'),
    });
    Object.defineProperty(pat, 'sub', {
        enumerable: false,
        configurable: true,
        get: () => makeCombineBuilder(handle, 'sub'),
    });
    return handle;
}

function makeCombineBuilder(
    pat: SpPattern,
    op: SpOpKind,
): SpCombineBuilder {
    const apply = (mode: SpAlignmentMode, rhs: string): SpPattern => {
        if (typeof rhs !== 'string') {
            throw new MiniParseError(
                `$p.s().${op}.${mode}() expects a string RHS, got ${typeof rhs}`,
            );
        }
        const payload = parsePayload(rhs);
        // Capture argument span at the chain method's call site. The
        // argument-span analyzer registers each chain RHS under the
        // method's source location, keyed by the param name `rhs`.
        const loc = captureSourceLocation();
        const argSpan =
            lookupArgumentSpan(loc, 'rhs') ?? ({ start: 0, end: 0 } as SourceSpan);
        return buildSpPattern(
            [...pat.sources, payload],
            pat.scale,
            [...pat.ops, { op, mode }],
            [...pat.argument_spans, argSpan],
        );
    };

    // Bare invocation defaults to `.in` (strudel's default alignment).
    const builder = ((rhs: string) => apply('in', rhs)) as SpCombineBuilder;
    for (const mode of ALIGNMENT_MODES) {
        // Use defineProperty to keep these non-enumerable on the builder
        // for cleaner JSON serialization snapshots if it ever leaks out.
        Object.defineProperty(builder, mode, {
            enumerable: false,
            configurable: true,
            value: (rhs: string) => apply(mode, rhs),
        });
    }
    return builder;
}

// `Located` is re-exported for tests that hand-construct ASTs.
export type { AtomValue, Located, SourceSpan };
