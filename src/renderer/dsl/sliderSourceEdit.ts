/**
 * Utility for finding slider value literal positions in DSL source code.
 *
 * Uses lightweight string parsing (no ts-morph) to locate `$slider(label, value, ...)`
 * calls by matching the label string literal. Returns character offsets of the value
 * argument — a numeric literal or a quoted hz/note string — so the UI can replace it
 * via Monaco edits.
 *
 * This runs in the renderer process, so it must not depend on Node.js-only modules.
 */

export interface SourceSpanResult {
    /** Inclusive start character offset */
    start: number;
    /** Exclusive end character offset */
    end: number;
}

/**
 * Scan the source for the character ranges occupied by line comments, block
 * comments, and string/template literals. A `$slider(...)` occurrence starting
 * inside any of these is not a live call and must be skipped.
 *
 * The scan is string-aware so a `//` or `/*` inside a string literal (e.g. a
 * URL) does not start a spurious comment, and quote characters inside comments
 * do not start a spurious string.
 *
 * @returns Sorted, non-overlapping `[start, end)` ranges to ignore.
 */
function findIgnoredRanges(source: string): Array<[number, number]> {
    const ranges: Array<[number, number]> = [];
    const n = source.length;
    let i = 0;
    while (i < n) {
        const c = source[i];
        const next = source[i + 1];

        // String / template literal — consumed whole so its contents can't
        // start a comment, and so a `$slider(` spelled inside it is ignored.
        if (c === '"' || c === "'" || c === '`') {
            const start = i;
            i++;
            while (i < n) {
                if (source[i] === '\\') {
                    i += 2;
                    continue;
                }
                if (source[i] === c) {
                    i++;
                    break;
                }
                i++;
            }
            ranges.push([start, i]);
            continue;
        }

        // Line comment — to end of line.
        if (c === '/' && next === '/') {
            const start = i;
            i += 2;
            while (i < n && source[i] !== '\n') {
                i++;
            }
            ranges.push([start, i]);
            continue;
        }

        // Block comment — to closing `*/` (or end of source if unterminated).
        if (c === '/' && next === '*') {
            const start = i;
            i += 2;
            while (i < n && !(source[i] === '*' && source[i + 1] === '/')) {
                i++;
            }
            i = Math.min(n, i + 2);
            ranges.push([start, i]);
            continue;
        }

        i++;
    }
    return ranges;
}

/** True if `offset` falls within any ignored range. */
function isIgnored(
    offset: number,
    ranges: Array<[number, number]>,
): boolean {
    for (const [start, end] of ranges) {
        if (offset >= start && offset < end) {
            return true;
        }
    }
    return false;
}

/** Advance past whitespace and comments, returning the next code offset. */
function skipTrivia(source: string, i: number): number {
    const n = source.length;
    for (;;) {
        while (i < n && /\s/.test(source[i])) {
            i++;
        }
        if (source[i] === '/' && source[i + 1] === '/') {
            i += 2;
            while (i < n && source[i] !== '\n') {
                i++;
            }
            continue;
        }
        if (source[i] === '/' && source[i + 1] === '*') {
            i += 2;
            while (i < n && !(source[i] === '*' && source[i + 1] === '/')) {
                i++;
            }
            i = Math.min(n, i + 2);
            continue;
        }
        return i;
    }
}

/**
 * Decode the escape sequences of a single- or double-quoted JS string literal
 * body, matching JS semantics so a decoded label compares equal to the value
 * the executed source produced at runtime.
 */
function decodeStringLiteral(body: string): string {
    let out = '';
    let i = 0;
    while (i < body.length) {
        const c = body[i];
        if (c !== '\\') {
            out += c;
            i++;
            continue;
        }
        const e = body[i + 1];
        i += 2;
        switch (e) {
            case 'n':
                out += '\n';
                break;
            case 't':
                out += '\t';
                break;
            case 'r':
                out += '\r';
                break;
            case 'b':
                out += '\b';
                break;
            case 'f':
                out += '\f';
                break;
            case 'v':
                out += '\v';
                break;
            case '0':
                out += '\0';
                break;
            case 'x':
                out += String.fromCharCode(
                    parseInt(body.slice(i, i + 2), 16),
                );
                i += 2;
                break;
            case 'u':
                if (body[i] === '{') {
                    const close = body.indexOf('}', i);
                    out += String.fromCodePoint(
                        parseInt(body.slice(i + 1, close), 16),
                    );
                    i = close + 1;
                } else {
                    out += String.fromCharCode(
                        parseInt(body.slice(i, i + 4), 16),
                    );
                    i += 4;
                }
                break;
            // Escaped line terminators are line continuations: no output.
            case '\n':
                break;
            case '\r':
                if (body[i] === '\n') {
                    i++;
                }
                break;
            default:
                out += e ?? '';
        }
    }
    return out;
}

/** Parse the quoted string literal starting at `start` (which must be a `"`
 *  or `'`), returning its decoded value and the offset just past the closing
 *  quote, or null if unterminated. */
function parseStringLiteralAt(
    source: string,
    start: number,
): { value: string; end: number } | null {
    const quote = source[start];
    let i = start + 1;
    while (i < source.length) {
        if (source[i] === '\\') {
            i += 2;
            continue;
        }
        if (source[i] === quote) {
            return {
                end: i + 1,
                value: decodeStringLiteral(source.slice(start + 1, i)),
            };
        }
        if (source[i] === '\n') {
            return null;
        }
        i++;
    }
    return null;
}

/**
 * Find the offset of the second argument's first token in a
 * `fnName("label", …)` call whose decoded label equals `label`. The label is
 * compared by decoded value (not source text), so labels whose literals use
 * escape sequences still match; whitespace and comments may appear anywhere
 * between the tokens.
 *
 * @param source - The full DSL source code
 * @param fnName - The callee name including its `$` prefix, e.g. "$slider"
 * @param label  - The runtime label string to match against
 * @returns The offset of the second argument, or null if no live call matches
 */
export function findLabeledCallSecondArgStart(
    source: string,
    fnName: string,
    label: string,
): number | null {
    const pattern = new RegExp(`\\${fnName}\\s*\\(`, 'g');
    const ignored = findIgnoredRanges(source);

    let match: RegExpExecArray | null;
    while ((match = pattern.exec(source)) !== null) {
        // Skip occurrences inside comments or string literals — only a live
        // call edits the audio engine, so only it may be edited.
        if (isIgnored(match.index, ignored)) {
            continue;
        }

        let i = skipTrivia(source, match.index + match[0].length);
        if (source[i] !== '"' && source[i] !== "'") {
            continue;
        }
        const lit = parseStringLiteralAt(source, i);
        if (!lit || lit.value !== label) {
            continue;
        }
        i = skipTrivia(source, lit.end);
        if (source[i] !== ',') {
            continue;
        }
        return skipTrivia(source, i + 1);
    }

    return null;
}

/**
 * Find the character offset range of the `value` argument in a `$slider(label, value, min, max)` call
 * whose label matches the given string.
 *
 * @param source - The full DSL source code
 * @param label  - The label string to match against
 * @returns The start/end offsets of the value argument literal, or null if not found
 */
export function findSliderValueSpan(
    source: string,
    label: string,
): SourceSpanResult | null {
    const start = findLabeledCallSecondArgStart(source, '$slider', label);
    if (start === null || start >= source.length) {
        return null;
    }

    // Numeric literal: optional minus, digits, optional decimal + digits
    const numMatch = source
        .slice(start)
        .match(/^-?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?/);
    if (numMatch) {
        return {
            end: start + numMatch[0].length,
            start,
        };
    }

    // Quoted string literal (hz/note sliders) — the span includes the
    // quotes so writeback replaces the whole literal.
    const strMatch = source.slice(start).match(/^(["'])(?:\\.|[^\\])*?\1/);
    if (strMatch) {
        return {
            end: start + strMatch[0].length,
            start,
        };
    }

    return null;
}
