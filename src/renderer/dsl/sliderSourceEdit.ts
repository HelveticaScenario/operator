/**
 * Utility for finding slider value literal positions in DSL source code.
 *
 * Uses lightweight string parsing (no ts-morph) to locate `$slider(label, value, ...)`
 * calls by matching the label string literal. Returns character offsets of the value
 * argument so the UI can replace it via Monaco edits.
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
function isIgnored(offset: number, ranges: Array<[number, number]>): boolean {
    for (const [start, end] of ranges) {
        if (offset >= start && offset < end) {
            return true;
        }
    }
    return false;
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
    // Build regex to find $slider( with the exact label string.
    // The label is a validated string literal, so we escape it for regex safety.
    const escapedLabel = label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    // Match: $slider( optional-whitespace, "label" or 'label', optional-whitespace, comma
    const pattern = new RegExp(
        `\\$slider\\s*\\(\\s*(?:"${escapedLabel}"|'${escapedLabel}')\\s*,`,
        'g',
    );

    const ignored = findIgnoredRanges(source);

    let match: RegExpExecArray | null;
    while ((match = pattern.exec(source)) !== null) {
        // Skip occurrences inside comments or string literals — only a live
        // `$slider(...)` call edits the audio engine, so only it may be edited.
        if (isIgnored(match.index, ignored)) {
            continue;
        }

        // Match[0] ends right after the comma following the label
        const afterComma = match.index + match[0].length;

        // Skip whitespace after the comma
        let start = afterComma;
        while (start < source.length && /\s/.test(source[start])) {
            start++;
        }

        if (start >= source.length) {
            continue;
        }

        // Parse the numeric literal: optional minus, digits, optional decimal + digits
        const numMatch = source
            .slice(start)
            .match(/^-?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?/);
        if (!numMatch) {
            continue;
        }

        return {
            end: start + numMatch[0].length,
            start,
        };
    }

    return null;
}
