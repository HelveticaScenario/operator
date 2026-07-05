import { describe, expect, test } from 'vitest';

import { resolveScopeCallRange, type ScopeCallModel } from '../scopeCallRange';

function makeModel(lines: string[]): ScopeCallModel {
    return {
        getLineContent: (lineNumber: number) => {
            if (lineNumber < 1 || lineNumber > lines.length) {
                throw new Error(`Illegal value for lineNumber: ${lineNumber}`);
            }
            return lines[lineNumber - 1];
        },
        getLineCount: () => lines.length,
    };
}

describe('resolveScopeCallRange', () => {
    test('covers the full call span when the analyzed lines exist', () => {
        const model = makeModel(['$sine(1)', '    .scope()', '    .out();']);
        const range = resolveScopeCallRange(
            model,
            { column: 5, line: 2 },
            { endLine: 3, startLine: 2 },
        );
        expect(range).toEqual({
            endColumn: '    .out();'.length + 1,
            endLineNumber: 3,
            startColumn: 5,
            startLineNumber: 2,
        });
    });

    test('skips a span whose start line the model no longer contains', () => {
        // The analysis saw a longer document than the model has (lines were
        // deleted during the submit round-trip); resolving must not throw.
        const model = makeModel(['$sine(1).out();']);
        expect(resolveScopeCallRange(model, { column: 1, line: 5 })).toBeNull();
    });

    test('clamps the end line to the current document', () => {
        const model = makeModel(['$sine(1)', '    .scope().out();']);
        const range = resolveScopeCallRange(
            model,
            { column: 1, line: 1 },
            { endLine: 4, startLine: 1 },
        );
        expect(range).toEqual({
            endColumn: '    .scope().out();'.length + 1,
            endLineNumber: 2,
            startColumn: 1,
            startLineNumber: 1,
        });
    });
});
