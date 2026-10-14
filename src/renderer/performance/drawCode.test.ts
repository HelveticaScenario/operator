import { describe, expect, it } from 'vitest';
import { expandTabs, spansOnLine, styledRuns } from './drawCode';

describe('expandTabs', () => {
    it('expands each tab to the next tab stop and maps characters to columns', () => {
        expect(expandTabs('a\tb', 4)).toEqual({
            columns: [0, 1, 4, 5],
            text: 'a   b',
        });
    });

    it('leaves a line without tabs as it is', () => {
        expect(expandTabs('abc', 2)).toEqual({
            columns: [0, 1, 2, 3],
            text: 'abc',
        });
    });
});

describe('spansOnLine', () => {
    const range = {
        end: { column: 3, line: 2 },
        start: { column: 5, line: 0 },
    };

    it('covers from the start to the end of the line on the first line', () => {
        expect(spansOnLine([range], 0, 8)).toEqual([[5, 8]]);
    });

    it('covers whole lines in the middle', () => {
        expect(spansOnLine([range], 1, 6)).toEqual([[0, 6]]);
    });

    it('covers up to the end column on the last line', () => {
        expect(spansOnLine([range], 2, 10)).toEqual([[0, 3]]);
    });

    it('covers nothing outside the range', () => {
        expect(spansOnLine([range], 3, 10)).toEqual([]);
    });
});

describe('styledRuns', () => {
    const tokens = [
        { offset: 0, type: 'identifier.js' },
        { offset: 5, type: 'delimiter.js' },
        { offset: 6, type: 'string.js' },
    ];

    it('splits a line at token boundaries', () => {
        expect(styledRuns(10, tokens, [])).toEqual([
            { from: 0, highlighted: false, to: 5, token: 0 },
            { from: 5, highlighted: false, to: 6, token: 1 },
            { from: 6, highlighted: false, to: 10, token: 2 },
        ]);
    });

    it('splits a token where a highlight starts and ends', () => {
        expect(styledRuns(10, tokens, [[7, 9]])).toEqual([
            { from: 0, highlighted: false, to: 5, token: 0 },
            { from: 5, highlighted: false, to: 6, token: 1 },
            { from: 6, highlighted: false, to: 7, token: 2 },
            { from: 7, highlighted: true, to: 9, token: 2 },
            { from: 9, highlighted: false, to: 10, token: 2 },
        ]);
    });
});
