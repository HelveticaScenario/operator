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

    let match: RegExpExecArray | null;
    while ((match = pattern.exec(source)) !== null) {
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
