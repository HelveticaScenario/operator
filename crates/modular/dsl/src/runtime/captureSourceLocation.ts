/**
 * Line offset for DSL code wrapper.
 * The executePatchScript creates a function body with 'use strict' which adds
 * lines before user code.  This offset is configured by executor.ts at
 * runtime via `setDSLWrapperLineOffset`.
 */
let dslWrapperLineOffset = 4;

export function setDSLWrapperLineOffset(offset: number): void {
    dslWrapperLineOffset = offset;
}

export function getDSLWrapperLineOffset(): number {
    return dslWrapperLineOffset;
}

/**
 * Capture source location from the current stack trace.
 * Looks for the `<anonymous>` frame which corresponds to DSL code executed
 * inside `new Function(...)` by `executePatchScript`.
 *
 * Returns `undefined` if the source location cannot be determined.
 */
export function captureSourceLocation():
    | { line: number; column: number }
    | undefined {
    const stackHolder: { stack?: string } = {};
    Error.captureStackTrace(stackHolder, captureSourceLocation);

    if (!stackHolder.stack) {
        return undefined;
    }

    // Stack frames from evaluated code look like:
    // "    at eval (eval at executePatchScript ..., <anonymous>:5:12)"
    // Or in some V8 versions:
    // "    at <anonymous>:5:12"
    const lines = stackHolder.stack.split('\n');

    for (const line of lines) {
        const anonymousMatch = line.match(/<anonymous>:(\d+):(\d+)/);
        if (anonymousMatch) {
            const rawLine = parseInt(anonymousMatch[1], 10);
            const column = parseInt(anonymousMatch[2], 10);
            const adjustedLine = rawLine - dslWrapperLineOffset;
            if (adjustedLine > 0) {
                return { column, line: adjustedLine };
            }
        }
    }

    return undefined;
}
