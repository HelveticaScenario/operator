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

    test('Note with flat, no octave', () => {
        const r = $p('eb');
        const atom = firstPureAtom(r.ast);
        expect(atom).toEqual({
            Pure: {
                node: { Note: { letter: 'e', accidental: 'b', octave: null } },
                span: { start: 0, end: 2 },
            },
        });
    });

    test('Note b-flat, no octave (b-letter / b-accidental collision)', () => {
        const r = $p('bb');
        const atom = firstPureAtom(r.ast);
        expect(atom).toEqual({
            Pure: {
                node: { Note: { letter: 'b', accidental: 'b', octave: null } },
                span: { start: 0, end: 2 },
            },
        });
    });

    test('Note with f-alias flat, no octave', () => {
        const r = $p('cf');
        const atom = firstPureAtom(r.ast);
        expect(atom).toEqual({
            Pure: {
                node: { Note: { letter: 'c', accidental: 'b', octave: null } },
                span: { start: 0, end: 2 },
            },
        });
    });

    test('bare note letter b stays a plain note (no accidental)', () => {
        const r = $p('b');
        const atom = firstPureAtom(r.ast);
        expect(atom).toEqual({
            Pure: {
                node: { Note: { letter: 'b', accidental: null, octave: null } },
                span: { start: 0, end: 1 },
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

// The Rust pest grammar's element/modifier rules are non-atomic, so pest
// inserts implicit WHITESPACE between an element_base and its modifiers and
// between a modifier sigil and its operand. The Peggy port must accept the
// same whitespace or otherwise-valid patterns regress to syntax errors —
// e.g. `<...> / 4`, which the Rust parser accepted as a Slow modifier.
describe('whitespace around modifiers (Rust pest parity)', () => {
    test('space before and after the slow sigil attaches to preceding element', () => {
        for (const src of ['0/2', '0 /2', '0/ 2', '0 / 2']) {
            const r = $p(src);
            if (!('Slow' in r.ast)) return expect.fail(`expected Slow for "${src}"`);
            expect(r.ast.Slow[1]).toEqual({
                Pure: { node: 2, span: expect.anything() },
            });
        }
    });

    test('slow modifier applies to a slowcat group with surrounding spaces', () => {
        const r = $p('<1 -1> / 4');
        if (!('Slow' in r.ast)) return expect.fail('expected Slow');
        expect('SlowCat' in r.ast.Slow[0]).toBe(true);
        expect(r.ast.Slow[1]).toEqual({
            Pure: { node: 4, span: expect.anything() },
        });
    });

    test('fast *n tolerates whitespace', () => {
        for (const src of ['0 *4', '0* 4', '0 * 4']) {
            expect('Fast' in $p(src).ast).toBe(true);
        }
    });

    test('weight @n tolerates whitespace', () => {
        const r = $p('0 @ 3 1');
        if (!('Sequence' in r.ast)) return expect.fail('expected Sequence');
        expect(r.ast.Sequence[0][1]).toBeCloseTo(3);
        expect(r.ast.Sequence[1][1]).toBeNull();
    });

    test('replicate / degrade / euclidean tolerate whitespace', () => {
        expect('Replicate' in $p('0 ! 3').ast).toBe(true);
        expect('Degrade' in $p('0 ? 0.3').ast).toBe(true);
        expect('Euclidean' in $p('0 (3,8)').ast).toBe(true);
    });

    test('reported regression: slowcat group divided by 4', () => {
        const src =
            '<[[1,-1,-4]@7 [[1,-1,-4] [1,-3,-4]@3]] [[1,-3,-4]@7 [[1,-3,-4] [1,-1,-4]@3]]> / 4';
        const r = $p(src);
        if (!('Slow' in r.ast)) return expect.fail('expected top-level Slow');
        expect('SlowCat' in r.ast.Slow[0]).toBe(true);
    });
});

describe('modifiers inside operand subsequences', () => {
    test('replicate !n inside a euclidean pulses subsequence', () => {
        const r = $p('0(<16!2 12>,8)');
        if (!('Euclidean' in r.ast)) return expect.fail('expected Euclidean');
        const pulses = r.ast.Euclidean.pulses;
        if (!('SlowCat' in pulses)) return expect.fail('expected SlowCat pulses');
        const first = pulses.SlowCat[0][0];
        expect('Replicate' in first).toBe(true);
        if ('Replicate' in first) {
            expect(first.Replicate[1]).toBe(2);
        }
    });

    test('full reported pattern parses', () => {
        expect(() =>
            $p('0(<16!2 12>,[16 <8 12> 16])'),
        ).not.toThrow();
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

    test('| chooses between whole bracketed subsequences', () => {
        // Regression: `|` must alternate whole subsequences, not bind to a
        // single neighbouring atom. `[0,0,0]` / `[0,-7,0]` are comma-chords.
        const r = $p('[0,0,0] | [0,-7,0]');
        if (!('RandomChoice' in r.ast))
            return expect.fail('expected RandomChoice');
        const [choices] = r.ast.RandomChoice;
        expect(choices.length).toBe(2);
        for (const c of choices) {
            if (!('FastCat' in c))
                return expect.fail('each choice should be a FastCat');
            expect('Stack' in c.FastCat[0][0]).toBe(true);
        }
    });

    test('| chooses between whole space-separated sequences', () => {
        const r = $p('0 1 | 2 3');
        if (!('RandomChoice' in r.ast))
            return expect.fail('expected RandomChoice');
        const [choices] = r.ast.RandomChoice;
        expect(choices.length).toBe(2);
        expect('Sequence' in choices[0]).toBe(true);
        expect('Sequence' in choices[1]).toBe(true);
    });

    test('seeds are assigned depth-first, left-to-right', () => {
        // `[0|1]` is parsed before `2?`, so the choice gets seed 0 and the
        // degrade gets seed 1.
        const r = $p('[0|1] 2?');
        if (!('Sequence' in r.ast)) return expect.fail('expected Sequence');
        const [first, second] = r.ast.Sequence;
        if (!('FastCat' in first[0]))
            return expect.fail('first element should be FastCat');
        const inner = first[0].FastCat[0][0];
        if (!('RandomChoice' in inner))
            return expect.fail('FastCat should wrap a RandomChoice');
        expect(inner.RandomChoice[1]).toBe(0);
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

    test('whitespace-only source is rejected', () => {
        expect(() => $p('   ')).toThrow(MiniParseError);
    });

    test('unclosed bracket is rejected', () => {
        expect(() => $p('[0 1')).toThrow(MiniParseError);
    });

    test('unknown operator is rejected', () => {
        expect(() => $p('0 & 1')).toThrow(MiniParseError);
    });
});

describe('rest alternatives', () => {
    test('`-` parses as a rest', () => {
        const r = $p('-');
        expect(r.ast).toEqual({ Rest: { start: 0, end: 1 } });
    });

    test('`-` and `~` rests interchangeable inside a sequence', () => {
        const r1 = $p('c4 - e4');
        const r2 = $p('c4 ~ e4');
        expect('Sequence' in r1.ast).toBe(true);
        expect('Sequence' in r2.ast).toBe(true);
        if ('Sequence' in r1.ast && 'Sequence' in r2.ast) {
            expect('Rest' in r1.ast.Sequence[1][0]).toBe(true);
            expect('Rest' in r2.ast.Sequence[1][0]).toBe(true);
        }
    });

    test('`-` followed by digit is a negative number, not a rest', () => {
        const r = $p('-1');
        const atom = firstPureAtom(r.ast);
        expect(atom).toEqual({
            Pure: { node: { Number: -1 }, span: { start: 0, end: 2 } },
        });
    });
});

describe('elongation `_`', () => {
    test('`_` extends preceding step weight by 1 (equivalent to @n)', () => {
        const elongated = $p('0 _ _');
        const weighted = $p('0@3');
        // Both should yield the same effective duration via different shapes.
        // Cheap structural check: elongated is a Sequence whose first entry
        // carries weight 3.
        expect('Sequence' in elongated.ast).toBe(true);
        if ('Sequence' in elongated.ast) {
            expect(elongated.ast.Sequence.length).toBe(1);
            expect(elongated.ast.Sequence[0][1]).toBe(3);
        }
        expect('Sequence' in weighted.ast || 'Pure' in weighted.ast).toBe(true);
    });

    test('multiple `_` after a step', () => {
        const r = $p('c4 _ _ _');
        expect('Sequence' in r.ast).toBe(true);
        if ('Sequence' in r.ast) {
            expect(r.ast.Sequence[0][1]).toBe(4);
        }
    });
});

describe('polymeter `{...}`', () => {
    test('basic polymeter wraps children in Polymeter node', () => {
        const r = $p('{c4 e4, g4 b4 d5}');
        expect('Polymeter' in r.ast).toBe(true);
        if ('Polymeter' in r.ast) {
            expect(r.ast.Polymeter.children.length).toBe(2);
            expect(r.ast.Polymeter.steps_per_cycle).toBe(null);
        }
    });

    test('polymeter with explicit step count `%n`', () => {
        const r = $p('{c4 e4 g4}%4');
        expect('Polymeter' in r.ast).toBe(true);
        if ('Polymeter' in r.ast) {
            expect(r.ast.Polymeter.steps_per_cycle).not.toBe(null);
        }
    });
});

describe('feet `.`', () => {
    test('feet split sequence elements at `.` boundaries', () => {
        // `.` alignment is mainly meaningful inside polymeter children, but
        // parser should accept it without error.
        const r = $p('{c4 . e4 g4, f4 a4 . b4}');
        expect('Polymeter' in r.ast).toBe(true);
    });

    test('bare feet wrap dot-separated sub-sequences in a Sequence node', () => {
        // Pins the AST shape convert.rs lowers identically to a fastcat — a
        // future FastCat-vs-Sequence change here would silently mis-render
        // feet (the Rust descent parser cannot parse `.`, so the parity gate
        // does not cover this construct).
        const r = $p('0 1 . 2 3');
        expect('Sequence' in r.ast).toBe(true);
        if ('Sequence' in r.ast) {
            expect(r.ast.Sequence).toHaveLength(2);
            expect(r.ast.Sequence.every(([, w]) => w === null)).toBe(true);
        }
    });
});

