/**
 * Source-text edit primitives shared by the DSL migration tools. An `Edit`
 * is a half-open `[start, end)` replacement against the original source;
 * `applyEdits` splices a set of them in one pass, refusing overlapping
 * edits rather than producing corrupt output.
 */

export interface Edit {
    start: number;
    end: number;
    replacement: string;
}

/**
 * Apply non-overlapping edits to `source`. Edits are sorted by start offset
 * and spliced left-to-right; if any edit starts before the previous one
 * ended, the whole batch is rejected (`conflict: true`) and the original
 * source is returned unchanged.
 */
export function applyEdits(
    source: string,
    edits: Edit[],
): { source: string; conflict: boolean } {
    if (edits.length === 0) return { source, conflict: false };
    const sorted = [...edits].sort((a, b) => a.start - b.start);
    let out = '';
    let cursor = 0;
    for (const edit of sorted) {
        if (edit.start < cursor) {
            return { source, conflict: true };
        }
        out += source.slice(cursor, edit.start);
        out += edit.replacement;
        cursor = edit.end;
    }
    out += source.slice(cursor);
    return { source: out, conflict: false };
}
