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
function $pImpl(source: string): ParsedPattern & TimeModifiable {
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
    return attachTimeModifiers(pattern);
}

/**
 * The mini-notation factory. Callable as `$p(source)` for a plain
 * `ParsedPattern`, carries `$p.s(source, scale)` for scale-degree patterns
 * (see `$spImpl`) and `$p.arrange([cycles, pattern], …)` for multi-cycle
 * arrangements (see `$arrangeImpl`). All three are hoisted, so the assignment
 * can reference them here.
 */
export const $p: typeof $pImpl & {
    s: typeof $spImpl;
    arrange: typeof $arrangeImpl;
} = Object.assign($pImpl, { s: $spImpl, arrange: $arrangeImpl });

/** Type guard for runtime `ParsedPattern` checks. */
export function isParsedPattern(value: unknown): value is ParsedPattern {
    return (
        typeof value === 'object' &&
        value !== null &&
        '__kind' in value &&
        value.__kind === 'ParsedPattern'
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
export type SpCombineBuilder = ((rhs: string) => SpPattern & TimeModifiable) & {
    [M in SpAlignmentMode]: (rhs: string) => SpPattern & TimeModifiable;
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
function $spImpl(source: string, scale: string): SpPattern & TimeModifiable {
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
        '__kind' in value &&
        value.__kind === SP_KIND
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
): SpPattern & TimeModifiable {
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
    // fast / slow are likewise non-enumerable; the wire shape stays clean.
    return attachTimeModifiers(handle);
}

function makeCombineBuilder(pat: SpPattern, op: SpOpKind): SpCombineBuilder {
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
        const argSpan = lookupArgumentSpan(loc, 'rhs') ?? { start: 0, end: 0 };
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

// ─── $p.arrange ─────────────────────────────────────────────────────────

/** Discriminator string the Rust `SeqPatternSource` untagged enum matches. */
const ARRANGE_KIND = 'ArrangePattern' as const;

/** A pattern usable as an `$p.arrange` section (and as a `$cycle` argument). */
export type SectionPattern =
    | ParsedPattern
    | SpPattern
    | ArrangePattern
    | FastPattern
    | SlowPattern;

/** One `[cycles, pattern]` tuple accepted by `$p.arrange`. */
export type ArrangeSectionArg = readonly [number, SectionPattern];

/**
 * Wire shape for one arrange section. `cycles` is a positive integer, or the
 * string `'Infinity'` for an infinite tail (JSON can't carry numeric infinity).
 * `pattern` is the embedded section payload — its enumerable fields already
 * match the Rust `SeqPatternSource` shape, so the object is embedded verbatim.
 */
export interface ArrangeSectionWire {
    cycles: number | 'Infinity';
    pattern: SectionPattern;
}

/**
 * Wire shape for an arrangement. Recognised by the Rust `SeqPatternSource`
 * untagged enum via `__kind === 'ArrangePattern'`. `argument_spans` is the flat
 * per-source span list (sections in order, sources within each section in
 * order) that `$cycle`'s factory maps to `pattern.0`, `pattern.1`, … for editor
 * highlighting — matching the flat `per_source` order the Rust side builds.
 *
 * An `ArrangePattern` is itself a valid `$p.arrange` section and `$cycle`
 * argument, so arrangements nest.
 */
export interface ArrangePattern {
    __kind: typeof ARRANGE_KIND;
    sections: Array<ArrangeSectionWire>;
    argument_spans: Array<SourceSpan>;
}

/** Type guard for runtime `ArrangePattern` checks. */
export function isArrangePattern(value: unknown): value is ArrangePattern {
    return (
        typeof value === 'object' &&
        value !== null &&
        '__kind' in value &&
        value.__kind === ARRANGE_KIND
    );
}

/**
 * Arrange multiple patterns over multiple cycles, mirroring Strudel's
 * `arrange`. Each argument is a `[cycles, pattern]` tuple: `pattern` (a
 * `$p(...)`, `$p.s(...)`, or nested `$p.arrange(...)`) plays for `cycles`
 * cycles, and the sections play back-to-back, looping with period `Σ cycles`.
 *
 * Because each `$p.s` section is resolved through its own scale, an arrangement
 * can switch scales between sections. Cycle counts must be positive integers,
 * except a single trailing section may use `Infinity` to loop forever once
 * reached (later sections would never play and are rejected).
 *
 * Returns an `ArrangePattern`, itself usable as a section of another
 * `$p.arrange(...)` or as the argument to `$cycle(...)`.
 */
function $arrangeImpl(
    ...sections: ArrangeSectionArg[]
): ArrangePattern & TimeModifiable {
    if (sections.length === 0) {
        throw new MiniParseError(
            '$p.arrange() requires at least one [cycles, pattern] section',
        );
    }

    const wireSections: ArrangeSectionWire[] = [];
    const argument_spans: SourceSpan[] = [];

    sections.forEach((sectionArg, i) => {
        if (!Array.isArray(sectionArg) || sectionArg.length !== 2) {
            throw new MiniParseError(
                `$p.arrange() section ${i} must be a [cycles, pattern] tuple`,
            );
        }
        const [cyclesRaw, pattern] = sectionArg;

        if (typeof cyclesRaw !== 'number' || Number.isNaN(cyclesRaw)) {
            throw new MiniParseError(
                `$p.arrange() section ${i}: cycles must be a number, got ${typeof cyclesRaw}`,
            );
        }
        let cycles: number | 'Infinity';
        if (cyclesRaw === Infinity) {
            if (i !== sections.length - 1) {
                throw new MiniParseError(
                    '$p.arrange(): an Infinity section must be the last section ' +
                        '(sections after it can never play)',
                );
            }
            cycles = 'Infinity';
        } else if (!Number.isInteger(cyclesRaw) || cyclesRaw <= 0) {
            throw new MiniParseError(
                `$p.arrange() section ${i}: cycles must be a positive integer or Infinity, got ${cyclesRaw}`,
            );
        } else {
            cycles = cyclesRaw;
        }

        // Collect each section's flat argument spans in order so the factory can
        // map them to `pattern.0`, `pattern.1`, … (mirroring the Rust per_source).
        argument_spans.push(
            ...collectPatternSpans(pattern, `$p.arrange() section ${i}`),
        );
        wireSections.push({ cycles, pattern });
    });

    return attachTimeModifiers({
        __kind: ARRANGE_KIND,
        sections: wireSections,
        argument_spans,
    });
}

// ─── .fast / .slow (time modifiers on every pattern) ────────────────────

/** Discriminator strings the Rust `SeqPatternSource` untagged enum matches. */
const FAST_KIND = 'FastPattern' as const;
const SLOW_KIND = 'SlowPattern' as const;

/**
 * The `.fast(...)` / `.slow(...)` methods attached (non-enumerably) to every
 * pattern object, mirroring Strudel's `fast`/`slow`. The factor is a constant
 * (`2`) or a mini-notation number pattern (`"2 4"` → ×2 then ×4 across the
 * cycle). A factor of `0` yields silence and negatives reverse time, matching
 * the in-string `*` / `/` operators.
 */
export interface TimeModifiable {
    fast(factor: number | string): FastPattern & TimeModifiable;
    slow(factor: number | string): SlowPattern & TimeModifiable;
}

/**
 * Wire shape for `pattern.fast(factor)`. Recognised by the Rust
 * `SeqPatternSource` untagged enum via `__kind === 'FastPattern'`. `pattern` is
 * the wrapped pattern (any `SectionPattern`, so wrappers chain and nest);
 * `argument_spans` is the wrapped pattern's flat per-source span list, plus the
 * factor's span when the factor is a pattern string. At runtime the object also
 * carries non-enumerable `.fast`/`.slow` methods (the `& TimeModifiable` the
 * builders return); the bare interface is the JSON wire shape.
 */
export interface FastPattern {
    __kind: typeof FAST_KIND;
    pattern: SectionPattern;
    /**
     * The speed factor. A bare `number` is a constant (`.fast(2)`) and is *not*
     * highlightable. A `ParsedPatternPayload` (`.fast("2 4")`) is its own
     * highlightable source whose active value lights up as it drives the speed.
     */
    factor: number | ParsedPatternPayload;
    argument_spans: Array<SourceSpan>;
}

/** Wire shape for `pattern.slow(factor)` — the time-inverse of `FastPattern`. */
export interface SlowPattern {
    __kind: typeof SLOW_KIND;
    pattern: SectionPattern;
    /** See {@link FastPattern.factor}. */
    factor: number | ParsedPatternPayload;
    argument_spans: Array<SourceSpan>;
}

/** Type guard for runtime `FastPattern` checks. */
export function isFastPattern(value: unknown): value is FastPattern {
    return (
        typeof value === 'object' &&
        value !== null &&
        '__kind' in value &&
        value.__kind === FAST_KIND
    );
}

/** Type guard for runtime `SlowPattern` checks. */
export function isSlowPattern(value: unknown): value is SlowPattern {
    return (
        typeof value === 'object' &&
        value !== null &&
        '__kind' in value &&
        value.__kind === SLOW_KIND
    );
}

/**
 * Collect a pattern's flat per-source argument spans in the same order the Rust
 * side flattens `per_source`. Shared by `$p.arrange` (per section) and the
 * `.fast`/`.slow` wrappers (the wrapped pattern). Throws on a non-pattern value.
 */
function collectPatternSpans(
    pattern: SectionPattern,
    context: string,
): SourceSpan[] {
    if (isParsedPattern(pattern)) {
        return [pattern.argument_span ?? { start: 0, end: 0 }];
    }
    if (
        isSpPattern(pattern) ||
        isArrangePattern(pattern) ||
        isFastPattern(pattern) ||
        isSlowPattern(pattern)
    ) {
        return [...pattern.argument_spans];
    }
    throw new MiniParseError(
        `${context}: pattern must be a $p(...), $p.s(...), $p.arrange(...), ` +
            `or .fast(...)/.slow(...) value`,
    );
}

/**
 * Build the wire payload for a `.fast`/`.slow` factor. A number is carried as a
 * raw constant (not a highlightable source); a string is parsed as full
 * mini-notation (`"2 4"`, `"<1 2>"`, …) into a `ParsedPatternPayload` that the
 * Rust side lowers to a `Pattern<Fraction>` and highlights.
 */
function factorPayload(arg: number | string): number | ParsedPatternPayload {
    if (typeof arg === 'number') {
        if (!Number.isFinite(arg)) {
            throw new MiniParseError(
                `fast()/slow() expects a finite number factor, got ${arg}`,
            );
        }
        return arg;
    }
    return parsePayload(arg);
}

/** Build a `FastPattern` wrapper around `inner`. */
function makeFast(
    inner: SectionPattern,
    factor: number | string,
): FastPattern & TimeModifiable {
    return attachTimeModifiers({
        __kind: FAST_KIND,
        pattern: inner,
        factor: factorPayload(factor),
        argument_spans: factorArgumentSpans(inner, factor, 'fast'),
    });
}

/** Build a `SlowPattern` wrapper around `inner`. */
function makeSlow(
    inner: SectionPattern,
    factor: number | string,
): SlowPattern & TimeModifiable {
    return attachTimeModifiers({
        __kind: SLOW_KIND,
        pattern: inner,
        factor: factorPayload(factor),
        argument_spans: factorArgumentSpans(inner, factor, 'slow'),
    });
}

/**
 * Build a wrapper's flat argument spans: the wrapped pattern's spans, then — for
 * a string (pattern) factor only — the factor's own document span (matching the
 * Rust `per_source` order). A number factor is a constant: no span, no source.
 */
function factorArgumentSpans(
    inner: SectionPattern,
    factor: number | string,
    methodName: 'fast' | 'slow',
): SourceSpan[] {
    const innerSpans = collectPatternSpans(inner, `$p(...).${methodName}()`);
    if (typeof factor !== 'string') {
        return innerSpans;
    }
    const loc = captureSourceLocation();
    const factorSpan = lookupArgumentSpan(loc, 'factor') ?? {
        start: 0,
        end: 0,
    };
    return [...innerSpans, factorSpan];
}

/**
 * Attach `.fast(...)` / `.slow(...)` methods to a pattern object. `Object.assign`
 * types the result; the `defineProperty` calls then flip the two methods to
 * non-enumerable so `JSON.stringify` (the IPC clone) and `Object.keys`/`entries`
 * skip them and only the wire-shape fields cross the boundary — the same trick
 * `$p.s` uses for `.add`/`.sub`. The object identity is preserved.
 */
function attachTimeModifiers<T extends SectionPattern>(
    pattern: T,
): T & TimeModifiable {
    const modifiable = Object.assign(pattern, {
        fast: (factor: number | string) => makeFast(pattern, factor),
        slow: (factor: number | string) => makeSlow(pattern, factor),
    });
    Object.defineProperty(modifiable, 'fast', { enumerable: false });
    Object.defineProperty(modifiable, 'slow', { enumerable: false });
    return modifiable;
}

// `Located` is re-exported for tests that hand-construct ASTs.
export type { AtomValue, Located, SourceSpan };
