/** Minimal Monaco text-model surface needed to resolve a scope call range. */
export interface ScopeCallModel {
    getLineCount(): number;
    getLineContent(lineNumber: number): string;
}

/**
 * Resolve the document range a scope() call decoration should cover.
 *
 * The line numbers come from DSL analysis of the submit-time source, while
 * `model` is the live editor document — the user may have edited it during
 * the async submit round-trip. Lines past the end of the document resolve to
 * null (the scope gets no document anchor) and the end line is clamped, so a
 * stale location can never address a line the model no longer has.
 */
export function resolveScopeCallRange(
    model: ScopeCallModel,
    loc: { line: number; column: number },
    callSpan?: { startLine: number; endLine: number },
): {
    startLineNumber: number;
    startColumn: number;
    endLineNumber: number;
    endColumn: number;
} | null {
    const lineCount = model.getLineCount();
    if (loc.line > lineCount) {
        return null;
    }

    const endLine = Math.min(callSpan?.endLine ?? loc.line, lineCount);
    const endLineContent = model.getLineContent(endLine);
    return {
        endColumn: endLineContent.length + 1,
        endLineNumber: endLine,
        startColumn: loc.column,
        startLineNumber: loc.line,
    };
}
