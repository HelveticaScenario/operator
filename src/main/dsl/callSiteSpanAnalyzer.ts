/**
 * Call Site Span Analyzer
 *
 * Tracks the full source span (start line through end line) of DSL method
 * calls like .scope() and standalone calls like $slider(). This information
 * is used to position view zones after the closing paren of multi-line calls.
 *
 * Designed to be extensible — any DSL function whose view zone needs to know
 * its full call expression span can be added to the tracking sets.
 */

import { Node, type SourceFile } from 'ts-morph';

import type { CallSiteKey, CallSiteSpanRegistry } from './sourceAnalysisTypes';

/** Method names tracked for call expression spans (property access calls) */
const DSL_METHODS_TO_TRACK = new Set(['scope']);

/** Standalone function names tracked for call expression spans */
const DSL_FUNCTIONS_TO_TRACK = new Set(['$slider']);

/**
 * Analyze call site expression spans in a parsed source file.
 *
 * Walks all call expressions and records the full span (start/end lines)
 * for tracked DSL methods and functions. Keys are computed in user-source
 * coordinates so the renderer can look up directly from
 * captureSourceLocation's {line, column} values.
 *
 * @param sourceFile - The ts-morph SourceFile to analyze
 * @param firstLineColumnOffset - Column offset for the first line
 * @returns Registry mapping call site keys to expression spans
 */
export function analyzeCallSiteSpans(
    sourceFile: SourceFile,
    firstLineColumnOffset: number,
): CallSiteSpanRegistry {
    const callSiteSpans: CallSiteSpanRegistry = new Map();

    sourceFile.forEachDescendant((node: Node) => {
        if (!Node.isCallExpression(node)) {
            return;
        }

        const call = node;
        const expression = call.getExpression();

        let callStartPos: number;

        if (Node.isPropertyAccessExpression(expression)) {
            // Method call like .scope() — V8 reports position of the method name
            const methodName = expression.getName();
            if (!DSL_METHODS_TO_TRACK.has(methodName)) {
                return;
            }
            callStartPos = expression.getNameNode().getStart();
        } else if (Node.isIdentifier(expression)) {
            // Standalone call like $slider()
            const funcName = expression.getText();
            if (!DSL_FUNCTIONS_TO_TRACK.has(funcName)) {
                return;
            }
            callStartPos = call.getStart();
        } else {
            return;
        }

        // Compute key in user-source coordinates so the renderer can look up
        // Directly from captureSourceLocation's {line, column} values.
        // Key format: "${userLine}:${v8Column}" where:
        //   UserLine = tsMorphLine (1-based, same as captureSourceLocation().line)
        //   V8Column = tsMorphCol + firstLineColumnOffset for line 1, else tsMorphCol
        //              (1-based, same as captureSourceLocation().column)
        const { line: startTsMorphLine, column: startTsMorphCol } =
            sourceFile.getLineAndColumnAtPos(callStartPos);
        const v8Column =
            startTsMorphCol +
            (startTsMorphLine === 1 ? firstLineColumnOffset : 0);
        const key: CallSiteKey = `${startTsMorphLine}:${v8Column}`;

        // Compute the end line of the full call expression (including closing paren)
        const callEnd = call.getEnd();
        const { line: endTsMorphLine } = sourceFile.getLineAndColumnAtPos(
            callEnd - 1,
        );

        callSiteSpans.set(key, {
            endLine: endTsMorphLine,
            startLine: startTsMorphLine,
        });
    });

    return callSiteSpans;
}
