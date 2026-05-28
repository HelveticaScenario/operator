import { describe, expect, test } from 'vitest';

import { $p, MiniParseError, isParsedPattern } from '../index';
import type { MiniAST } from '../ast';

function firstPureAtom(ast: MiniAST): MiniAST {
    // Recursively drill into Sequence/FastCat/SlowCat to find the first Pure.
    if ('Pure' in ast) return ast;
    if ('Sequence' in ast) return firstPureAtom(ast.Sequence[0][0]);
    if ('FastCat' in ast) return firstPureAtom(ast.FastCat[0][0]);
    if ('SlowCat' in ast) return firstPureAtom(ast.SlowCat[0][0]);
    if ('Stack' in ast) return firstPureAtom(ast.Stack[0]);
    if ('Fast' in ast) return firstPureAtom(ast.Fast[0]);
    if ('Slow' in ast) return firstPureAtom(ast.Slow[0]);
    if ('Replicate' in ast) return firstPureAtom(ast.Replicate[0]);
    if ('Degrade' in ast) return firstPureAtom(ast.Degrade[0]);
    if ('Euclidean' in ast) return firstPureAtom(ast.Euclidean.pattern);
    if ('RandomChoice' in ast) return firstPureAtom(ast.RandomChoice[0][0]);
    throw new Error('No Pure atom found');
}

describe('$p', () => {
    test('returns a ParsedPattern wrapper', () => {
        const r = $p('0');
        expect(isParsedPattern(r)).toBe(true);
        expect(r.__kind).toBe('ParsedPattern');
        expect(r.source).toBe('0');
        expect(Array.isArray(r.ast)).toBe(false);
    });

    test('rejects non-string input', () => {
        // @ts-expect-error intentional runtime check
        expect(() => $p(42)).toThrow(MiniParseError);
    });
});

describe('atom kinds', () => {
    test('Number', () => {
        const r = $p('42');
        expect(r.ast).toEqual({
            Pure: { node: { Number: 42 }, span: { start: 0, end: 2 } },
        });
    });

    test('negative Number', () => {
        const r = $p('-1.5');
        expect(r.ast).toEqual({
            Pure: { node: { Number: -1.5 }, span: { start: 0, end: 4 } },
        });
    });

    test('Hz', () => {
        const r = $p('440hz');
        expect(r.ast).toEqual({
            Pure: { node: { Hz: 440 }, span: { start: 0, end: 5 } },
        });
    });

    test('Hz is case-insensitive', () => {
        const r = $p('880Hz');
        expect(r.ast).toEqual({
            Pure: { node: { Hz: 880 }, span: { start: 0, end: 5 } },
        });
    });

    test('Note with octave', () => {
        const r = $p('c4');
        expect(r.ast).toEqual({
            Pure: {
                node: {
                    Note: { letter: 'c', accidental: null, octave: 4 },
                },
                span: { start: 0, end: 2 },
            },
        });
    });

    test('Note with sharp', () => {
        const r = $p('d#4');
        const atom = firstPureAtom(r.ast);
        expect(atom).toEqual({
            Pure: {
                node: { Note: { letter: 'd', accidental: '#', octave: 4 } },
                span: { start: 0, end: 3 },
            },
        });
    });

    test('Note with flat', () => {
        const r = $p('eb4');
        const atom = firstPureAtom(r.ast);
        expect(atom).toEqual({
            Pure: {
                node: { Note: { letter: 'e', accidental: 'b', octave: 4 } },
                span: { start: 0, end: 3 },
            },
        });
    });

    test('Note with s-alias sharp', () => {
        const r = $p('cs4');
        const atom = firstPureAtom(r.ast);
        expect(atom).toEqual({
            Pure: {
                node: { Note: { letter: 'c', accidental: '#', octave: 4 } },
                span: { start: 0, end: 3 },
            },
        });
    });

    test('Rest', () => {
        const r = $p('~');
        expect(r.ast).toEqual({ Rest: { start: 0, end: 1 } });
    });
});

describe('sequences and groupings', () => {
    test('space-separated sequence', () => {
        const r = $p('0 1 2');
        expect('Sequence' in r.ast).toBe(true);
        if ('Sequence' in r.ast) {
            expect(r.ast.Sequence.length).toBe(3);
            for (const [, weight] of r.ast.Sequence) {
                expect(weight).toBeNull();
            }
        }
    });

    test('fast subsequence [...]', () => {
        const r = $p('[0 1]');
        expect('FastCat' in r.ast).toBe(true);
    });

    test('slow subsequence <...>', () => {
        const r = $p('<0 1 2>');
        expect('SlowCat' in r.ast).toBe(true);
    });

    test('stack via comma', () => {
        const r = $p('0 1, 2 3');
        expect('Stack' in r.ast).toBe(true);
        if ('Stack' in r.ast) {
            expect(r.ast.Stack.length).toBe(2);
        }
    });

    test('nested stack inside subsequence', () => {
        const r = $p('[0 1, 2 3]');
        expect('FastCat' in r.ast).toBe(true);
    });
});

describe('modifiers', () => {
    test('fast *n with integer', () => {
        const r = $p('0*4');
        expect('Fast' in r.ast).toBe(true);
    });

    test('slow /n', () => {
        const r = $p('0/2');
        expect('Slow' in r.ast).toBe(true);
    });

    test('replicate !n', () => {
        const r = $p('0!3');
        expect('Replicate' in r.ast).toBe(true);
        if ('Replicate' in r.ast) {
            expect(r.ast.Replicate[1]).toBe(3);
        }
    });

    test('replicate ! defaults to 2', () => {
        const r = $p('0!');
        if ('Replicate' in r.ast) {
            expect(r.ast.Replicate[1]).toBe(2);
        } else {
            expect.fail('expected Replicate');
        }
    });

    test('degrade ? with probability', () => {
        const r = $p('0?0.3');
        expect('Degrade' in r.ast).toBe(true);
        if ('Degrade' in r.ast) {
            expect(r.ast.Degrade[1]).toBeCloseTo(0.3);
        }
    });

    test('degrade ? default probability (null)', () => {
        const r = $p('0?');
        if ('Degrade' in r.ast) {
            expect(r.ast.Degrade[1]).toBeNull();
        } else {
            expect.fail('expected Degrade');
        }
    });

    test('euclidean (k,n)', () => {
        const r = $p('0(3,8)');
        expect('Euclidean' in r.ast).toBe(true);
        if ('Euclidean' in r.ast) {
            expect(r.ast.Euclidean.rotation).toBeNull();
        }
    });

    test('euclidean with rotation', () => {
        const r = $p('0(3,8,2)');
        expect('Euclidean' in r.ast).toBe(true);
        if ('Euclidean' in r.ast) {
            expect(r.ast.Euclidean.rotation).not.toBeNull();
        }
    });

    test('fast factor as subsequence c*[1 2]', () => {
        const r = $p('c*[1 2]');
        if (!('Fast' in r.ast)) return expect.fail('expected Fast');
        const [, factor] = r.ast.Fast;
        expect('FastCat' in factor).toBe(true);
    });

    test('weight @n as positional metadata', () => {
        const r = $p('0@3 1');
        if (!('Sequence' in r.ast)) return expect.fail('expected Sequence');
        const entries = r.ast.Sequence;
        expect(entries.length).toBe(2);
        expect(entries[0][1]).toBeCloseTo(3);
        expect(entries[1][1]).toBeNull();
    });
});

describe('random choice', () => {
    test('|-separated choices collapse into RandomChoice', () => {
        const r = $p('0|1|2');
        expect('RandomChoice' in r.ast).toBe(true);
        if ('RandomChoice' in r.ast) {
            expect(r.ast.RandomChoice[0].length).toBe(3);
            expect(r.ast.RandomChoice[1]).toBe(0); // first seed
        }
    });

    test('rest allowed inside choice', () => {
        const r = $p('0|~');
        if (!('RandomChoice' in r.ast))
            return expect.fail('expected RandomChoice');
        expect('Rest' in r.ast.RandomChoice[0][1]).toBe(true);
    });

    test('seeds are assigned depth-first, left-to-right', () => {
        const r = $p('0|1 2?');
        // Walk the tree to collect seeds; expect RandomChoice first (0), Degrade second (1).
        if (!('Sequence' in r.ast)) return expect.fail('expected Sequence');
        const [first, second] = r.ast.Sequence;
        if (!('RandomChoice' in first[0]))
            return expect.fail('first element should be RandomChoice');
        expect(first[0].RandomChoice[1]).toBe(0);
        if (!('Degrade' in second[0]))
            return expect.fail('second element should be Degrade');
        expect(second[0].Degrade[2]).toBe(1);
    });
});

describe('leaf spans', () => {
    test('"c*[1 2]" collects c, 1, 2 spans', () => {
        const r = $p('c*[1 2]');
        expect(r.all_spans).toContainEqual([0, 1]);
        expect(r.all_spans).toContainEqual([3, 4]);
        expect(r.all_spans).toContainEqual([5, 6]);
        expect(r.all_spans.length).toBe(3);
    });

    test('"0 1 2" collects three spans', () => {
        const r = $p('0 1 2');
        expect(r.all_spans.length).toBe(3);
        expect(r.all_spans).toContainEqual([0, 1]);
        expect(r.all_spans).toContainEqual([2, 3]);
        expect(r.all_spans).toContainEqual([4, 5]);
    });

    test('"~ 0 ~ 1" collects four spans including rests', () => {
        const r = $p('~ 0 ~ 1');
        expect(r.all_spans.length).toBe(4);
    });
});

describe('negative cases (dropped atom kinds)', () => {
    test('midi shorthand m60 is a parse error', () => {
        expect(() => $p('m60')).toThrow(MiniParseError);
    });

    test('identifier bd is a parse error', () => {
        expect(() => $p('bd')).toThrow(MiniParseError);
    });

    test('module reference is a parse error', () => {
        expect(() => $p('module(osc1:out:0)')).toThrow(MiniParseError);
    });

    test('voltage shorthand 2v is a parse error', () => {
        expect(() => $p('2v')).toThrow(MiniParseError);
    });
});

describe('whitespace and edge cases', () => {
    test('leading/trailing whitespace is tolerated', () => {
        const r = $p('  0 1  ');
        expect('Sequence' in r.ast).toBe(true);
    });

    test('empty source is rejected', () => {
        expect(() => $p('')).toThrow(MiniParseError);
    });

    test('unclosed bracket is rejected', () => {
        expect(() => $p('[0 1')).toThrow(MiniParseError);
    });

    test('unknown operator is rejected', () => {
        expect(() => $p('0 & 1')).toThrow(MiniParseError);
    });
});
